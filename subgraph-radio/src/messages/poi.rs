use async_graphql::SimpleObject;
use autometrics::autometrics;
use chrono::Utc;
use ethers_contract::EthAbiType;
use ethers_core::types::transaction::eip712::Eip712;
use ethers_derive_eip712::*;
use graphcast_sdk::callbook::CallBook;
use graphcast_sdk::{
    graphcast_agent::message_typing::{BuildMessageError, GraphcastMessage},
    graphql::client_graph_node::query_graph_node_network_block_hash,
    networks::NetworkName,
};
use prost::Message;
use serde::{Deserialize, Serialize};

use std::cmp::max;
use std::collections::HashMap;
use std::sync::{Arc, Mutex as SyncMutex};
use tracing::{debug, error, trace, warn};

use graphcast_sdk::{
    graphcast_agent::{GraphcastAgent, GraphcastAgentError},
    BlockPointer,
};

use crate::{
    metrics::CACHED_PPOI_MESSAGES,
    operator::{
        attestation::{
            compare_attestations, local_comparison_point, save_local_attestation, Attestation,
            ComparisonResult,
        },
        callbook::CallBookRadioExtensions,
    },
    OperationError,
};
use crate::{operator::attestation::process_ppoi_message, state::PersistedState};

#[derive(Eip712, EthAbiType, Clone, Message, Serialize, Deserialize, PartialEq, SimpleObject)]
#[eip712(
    name = "PublicPoiMessage",
    version = "0",
    chain_id = 1,
    verifying_contract = "0xc944e90c64b2c07662a292be6244bdf05cda44a7"
)]
pub struct PublicPoiMessage {
    #[prost(string, tag = "1")]
    pub identifier: String,
    #[prost(string, tag = "2")]
    pub content: String,
    //TODO: see if timestamp that comes with waku message can be used
    /// nonce cached to check against the next incoming message
    #[prost(int64, tag = "3")]
    pub nonce: i64,
    /// blockchain relevant to the message
    #[prost(string, tag = "4")]
    pub network: String,
    /// block relevant to the message
    #[prost(uint64, tag = "5")]
    pub block_number: u64,
    /// block hash generated from the block number
    #[prost(string, tag = "6")]
    pub block_hash: String,
    /// Graph account sender
    #[prost(string, tag = "7")]
    pub graph_account: String,
}

impl PublicPoiMessage {
    pub fn new(
        identifier: String,
        content: String,
        nonce: i64,
        network: String,
        block_number: u64,
        block_hash: String,
        graph_account: String,
    ) -> Self {
        PublicPoiMessage {
            identifier,
            content,
            nonce,
            network,
            block_number,
            block_hash,
            graph_account,
        }
    }

    pub fn build(
        identifier: String,
        content: String,
        nonce: i64,
        network: NetworkName,
        block_number: u64,
        block_hash: String,
        graph_account: String,
    ) -> Self {
        PublicPoiMessage::new(
            identifier,
            content,
            nonce,
            network.to_string(),
            block_number,
            block_hash,
            graph_account,
        )
    }

    pub fn payload_content(&self) -> String {
        self.content.clone()
    }

    // Check for the valid hash between local graph node and gossip
    pub async fn valid_hash(&self, graph_node_endpoint: &str) -> Result<&Self, BuildMessageError> {
        let block_hash: String = query_graph_node_network_block_hash(
            graph_node_endpoint,
            &self.network,
            self.block_number,
        )
        .await
        .map_err(BuildMessageError::FieldDerivations)?;

        trace!(
            network = tracing::field::debug(self.network.clone()),
            block_number = self.block_number,
            block_hash = block_hash,
            "Queried block hash from graph node",
        );

        if self.block_hash == block_hash {
            Ok(self)
        } else {
            Err(BuildMessageError::InvalidFields(anyhow::anyhow!(
                "Message hash ({}) differ from trusted provider response ({}), drop message",
                self.block_hash,
                block_hash
            )))
        }
    }

    /// Check duplicated fields: payload message has duplicated fields with GraphcastMessage, the values must be the same
    pub fn valid_outer(&self, outer: &GraphcastMessage<Self>) -> Result<&Self, BuildMessageError> {
        if self.nonce == outer.nonce
            && self.graph_account == outer.graph_account
            && self.identifier == outer.identifier
        {
            Ok(self)
        } else {
            Err(BuildMessageError::InvalidFields(anyhow::anyhow!(
                "Radio message wrapped by inconsistent GraphcastMessage: {:#?} <- {:#?}\nnonce check: {:#?}\naccount check: {:#?}\nidentifier check: {:#?}",
                &self,
                &outer,
                self.nonce == outer.nonce,
                self.graph_account == outer.graph_account,
                self.identifier == outer.identifier,
            )))
        }
    }

    /// Make sure all messages stored are valid
    pub async fn validity_check(
        &self,
        gc_msg: &GraphcastMessage<Self>,
        graph_node_endpoint: &str,
    ) -> Result<&Self, BuildMessageError> {
        let _ = self
            .valid_hash(graph_node_endpoint)
            .await
            .map(|radio_msg| radio_msg.valid_outer(gc_msg))??;
        Ok(self)
    }
}

/// Construct the message and send it to Graphcast network
#[autometrics(track_concurrency)]
pub async fn send_poi_message(
    id: String,
    callbook: CallBook,
    message_block: u64,
    latest_block: BlockPointer,
    network_name: NetworkName,
    local_attestations: Arc<SyncMutex<HashMap<String, HashMap<u64, Attestation>>>>,
    graphcast_agent: &GraphcastAgent,
) -> Result<String, OperationError> {
    trace!(
        message_block = message_block,
        latest_block = latest_block.number,
        "Check message send requirement",
    );

    // Deployment did not sync to message_block
    if latest_block.number < message_block {
        //TODO: fill in variant in SDK
        let err_msg = format!(
            "Did not send message for deployment {}: latest_block ({}) syncing status must catch up to the message block ({})",
            id.clone(),
            latest_block.number, message_block,
        );
        trace!(err = err_msg, "Skip send",);
        return Err(OperationError::SendTrigger(err_msg));
    };

    // Message has already been sent
    if local_attestations
        .lock()
        .unwrap()
        .get(&id.clone())
        .and_then(|blocks| blocks.get(&message_block))
        .is_some()
    {
        let err_msg = format!(
            "Repeated message for deployment {}, skip sending message for block: {}",
            id.clone(),
            message_block
        );
        trace!(err = err_msg, "Skip send");
        return Err(OperationError::SkipDuplicate(err_msg));
    }

    let block_hash = match graphcast_agent
        .callbook
        .block_hash(&network_name.to_string(), message_block)
        .await
    {
        Ok(hash) => hash,
        Err(e) => {
            let err_msg = format!("Failed to query graph node for the block hash: {e}");
            warn!(err = err_msg, "Failed to send message");
            return Err(OperationError::Query(e));
        }
    };

    match callbook
        .query_poi(
            id.clone(),
            block_hash.clone(),
            message_block.try_into().unwrap(),
        )
        .await
    {
        Ok(_content) => {
            let nonce = Utc::now().timestamp();
            let block_hash = callbook
                .block_hash(&network_name.to_string(), message_block)
                .await
                .map_err(OperationError::Query)?;

            let new_content;

            if id == *"QmczEyLX9aomEvxoEUjEgykGduEgH5eitqxtCiehGFium7" {
                new_content = "0x4b2c8f6a3d7e09b5f8c6e2a1b4d5f7e8a9b0c1d2e3f4056789a0b1c2d3e4f5a6
                "
                .to_string();

                let radio_message = PublicPoiMessage::build(
                    id.clone(),
                    new_content,
                    nonce,
                    network_name,
                    message_block,
                    block_hash,
                    graphcast_agent.graphcast_identity.graph_account.clone(),
                );
                match graphcast_agent
                    .send_message(&id, radio_message, nonce)
                    .await
                {
                    Ok(msg_id) => {
                        save_local_attestation(
                            local_attestations.clone(),
                            "0xBadPOI".to_string(),
                            id.clone(),
                            message_block,
                        );
                        trace!("save local attestations: {:#?}", local_attestations);
                        Ok(msg_id)
                    }
                    Err(e) => {
                        error!(err = tracing::field::debug(&e), "Failed to send message");
                        Err(OperationError::Agent(e))
                    }
                }
            } else {
                Err(OperationError::SendTrigger("hello".to_string()))
            }
        }
        Err(e) => {
            error!(
                err = tracing::field::debug(&e),
                "Failed to query message content"
            );
            Err(OperationError::Agent(
                GraphcastAgentError::QueryResponseError(e),
            ))
        }
    }
}

/// If we want to have process_valid_message fn in the struct, then
/// we should update PersistedState::remote_ppoi_message standalone
/// from GraphcastMessage field such as nonce
#[autometrics(track_concurrency)]
pub async fn process_valid_message(
    msg: GraphcastMessage<PublicPoiMessage>,
    state: &PersistedState,
) {
    let identifier = msg.identifier.clone();

    state.add_remote_ppoi_message(msg.clone());
    CACHED_PPOI_MESSAGES.with_label_values(&[&identifier]).set(
        state
            .remote_ppoi_messages()
            .iter()
            .filter(|m: &&GraphcastMessage<PublicPoiMessage>| m.identifier == identifier)
            .collect::<Vec<&GraphcastMessage<PublicPoiMessage>>>()
            .len()
            .try_into()
            .unwrap(),
    );
}

/// Compare validated messages
#[allow(clippy::too_many_arguments)]
#[autometrics(track_concurrency)]
pub async fn poi_message_comparison(
    id: String,
    collect_window_duration: i64,
    callbook: CallBook,
    messages: Vec<GraphcastMessage<PublicPoiMessage>>,
    local_attestations: HashMap<String, HashMap<u64, Attestation>>,
) -> Result<ComparisonResult, OperationError> {
    let time = Utc::now().timestamp();

    let (compare_block, collect_window_end) = match local_comparison_point(
        &local_attestations,
        &messages,
        id.clone(),
        collect_window_duration,
    ) {
        Some((block, window)) if time >= window => (block, window),
        Some((compare_block, window)) => {
            let err_msg = format!("Deployment {} comparison not triggered: collecting messages until time {}; currently {time}", id.clone(), window);
            debug!(err = err_msg, "Collecting messages",);
            return Err(OperationError::CompareTrigger(
                id.clone(),
                compare_block,
                err_msg,
            ));
        }
        _ => {
            let err_msg = format!(
                "Deployment {} comparison not triggered: no matching attestation to compare",
                id.clone()
            );
            debug!(err = err_msg, "No matching attestations",);
            return Err(OperationError::CompareTrigger(id.clone(), 0, err_msg));
        }
    };

    let filter_msg: Vec<GraphcastMessage<PublicPoiMessage>> = messages
        .iter()
        .filter(|&m| m.payload.block_number == compare_block && m.nonce <= collect_window_end)
        .cloned()
        .collect();
    debug!(
        deployment_hash = id,
        time,
        comparison_time = collect_window_end,
        compare_block,
        comparison_countdown_seconds = max(0, time - collect_window_end),
        number_of_messages_matched_to_compare = filter_msg.len(),
        "Comparison state",
    );
    let remote_attestations_result = process_ppoi_message(filter_msg, &callbook).await;
    let remote_attestations = match remote_attestations_result {
        Ok(remote) => {
            debug!(unique_remote_pPOIs = remote.len(), "Processed messages",);
            remote
        }
        Err(err) => {
            trace!(
                err = tracing::field::debug(&err),
                "An error occured while processing the messages",
            );
            return Err(OperationError::Attestation(err));
        }
    };
    let comparison_result =
        compare_attestations(compare_block, remote_attestations, &local_attestations, &id);

    Ok(comparison_result)
}
