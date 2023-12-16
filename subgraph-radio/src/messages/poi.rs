use async_graphql::SimpleObject;
use autometrics::autometrics;
use chrono::Utc;
use ethers_contract::EthAbiType;
use ethers_core::types::transaction::eip712::Eip712;
use ethers_derive_eip712::*;
use graphcast_sdk::callbook::CallBook;
use graphcast_sdk::{
    graphcast_agent::message_typing::{GraphcastMessage, MessageError, RadioPayload},
    graphql::client_graph_node::query_graph_node_network_block_hash,
    networks::NetworkName,
};

use prost::Message;
use serde::{Deserialize, Serialize};

use sqlx::SqlitePool;
use tracing::{error, info, trace, warn};

use graphcast_sdk::{
    graphcast_agent::{GraphcastAgent, GraphcastAgentError},
    BlockPointer,
};

use crate::database::{
    count_remote_ppoi_messages, get_local_attestation, get_remote_ppoi_messages_by_identifier,
    insert_local_attestation, insert_remote_ppoi_message,
};
use crate::operator::attestation::process_ppoi_message;
use crate::{
    metrics::CACHED_PPOI_MESSAGES,
    operator::{
        attestation::{
            compare_attestations, local_comparison_point, Attestation, ComparisonResult,
        },
        callbook::CallBookRadioExtensions,
    },
    OperationError,
};

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
    #[prost(uint64, tag = "3")]
    pub nonce: u64,
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

impl RadioPayload for PublicPoiMessage {
    /// Check duplicated fields: payload message has duplicated fields with GraphcastMessage, the values must be the same
    fn valid_outer(&self, outer: &GraphcastMessage<Self>) -> Result<&Self, MessageError> {
        if self.nonce == outer.nonce
            && self.graph_account == outer.graph_account
            && self.identifier == outer.identifier
        {
            Ok(self)
        } else {
            Err(MessageError::InvalidFields(anyhow::anyhow!(
                "Radio message wrapped by inconsistent GraphcastMessage: {:#?} <- {:#?}\nnonce check: {:#?}\naccount check: {:#?}\nidentifier check: {:#?}",
                &self,
                &outer,
                self.nonce == outer.nonce,
                self.graph_account == outer.graph_account,
                self.identifier == outer.identifier,
            )))
        }
    }
}

impl PublicPoiMessage {
    pub fn new(
        identifier: String,
        content: String,
        nonce: u64,
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
        nonce: u64,
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
    pub async fn valid_hash(&self, graph_node_endpoint: &str) -> Result<&Self, MessageError> {
        let block_hash: String = query_graph_node_network_block_hash(
            graph_node_endpoint,
            &self.network,
            self.block_number,
        )
        .await
        .map_err(MessageError::FieldDerivations)?;

        trace!(
            network = tracing::field::debug(self.network.clone()),
            block_number = self.block_number,
            block_hash = block_hash,
            "Queried block hash from graph node",
        );

        if self.block_hash == block_hash {
            Ok(self)
        } else {
            Err(MessageError::InvalidFields(anyhow::anyhow!(
                "Message hash ({}) differ from trusted provider response ({}), drop message",
                self.block_hash,
                block_hash
            )))
        }
    }

    /// Make sure all messages stored are valid
    pub async fn validity_check(
        &self,
        gc_msg: &GraphcastMessage<Self>,
        graph_node_endpoint: &str,
    ) -> Result<&Self, MessageError> {
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
    graphcast_agent: &GraphcastAgent,
    db: SqlitePool,
) -> Result<String, OperationError> {
    trace!(
        message_block = message_block,
        latest_block = latest_block.number,
        "Check message send requirement",
    );

    info!("are we never sending msgs??");

    // Deployment did not sync to message_block
    if latest_block.number < message_block {
        let err_msg = format!(
            "Did not send message for deployment {}: latest_block ({}) syncing status must catch up to the message block ({})",
            id.clone(),
            latest_block.number, message_block,
        );
        trace!(err = err_msg, "Skip send",);
        return Err(OperationError::SendTrigger(err_msg));
    }

    //Message has already been sent
    if let Ok(Some(_)) = get_local_attestation(&db, &id, message_block).await {
        let err_msg = format!(
            "Repeated message for deployment {}, skip sending message for block: {}",
            id.clone(),
            message_block
        );
        trace!(err = err_msg, "Skip send");
        return Err(OperationError::SkipDuplicate(err_msg));
    }

    let block_hash = match callbook
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
        Ok(content) => {
            let nonce = Utc::now().timestamp() as u64;
            let radio_message = PublicPoiMessage::build(
                id.clone(),
                content.clone(),
                nonce,
                network_name,
                message_block,
                block_hash.clone(),
                graphcast_agent.graphcast_identity.graph_account.clone(),
            );

            match graphcast_agent
                .send_message(&id, radio_message, nonce)
                .await
            {
                Ok(msg_id) => {
                    // After successfully sending, save the attestation
                    let new_attestation = Attestation {
                        identifier: id.clone(),
                        block_number: message_block,
                        ppoi: content.clone(),
                        stake_weight: 0,
                        timestamp: vec![nonce],
                        senders: vec![],
                        sender_group_hash: String::new(),
                    };
                    if let Err(e) = insert_local_attestation(&db, new_attestation).await {
                        error!(
                            err = tracing::field::debug(&e),
                            "Failed to save local attestation"
                        );
                        return Err(OperationError::Database(e));
                    }

                    trace!("Saved local attestation for deployment: {}", id.clone());
                    Ok(msg_id)
                }
                Err(e) => {
                    error!(err = tracing::field::debug(&e), "Failed to send message");
                    Err(OperationError::Agent(e))
                }
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
pub async fn process_valid_message(msg: GraphcastMessage<PublicPoiMessage>, pool: &SqlitePool) {
    let identifier = msg.identifier.clone();

    if let Err(e) = insert_remote_ppoi_message(pool, &msg).await {
        error!("Error adding remote ppoi message to database: {:?}", e);
        return;
    }

    // If insertion is successful, proceed to count the messages
    if let Ok(message_count) = count_remote_ppoi_messages(pool, &identifier).await {
        // Update the metrics
        CACHED_PPOI_MESSAGES
            .with_label_values(&[&identifier])
            .set(message_count.into());
    } else {
        error!("Error counting remote ppoi messages.");
    }
}

#[allow(clippy::too_many_arguments)]
#[autometrics(track_concurrency)]
pub async fn poi_message_comparison(
    id: String,
    collect_window_duration: u64,
    callbook: CallBook,
    db: SqlitePool,
) -> Result<ComparisonResult, OperationError> {
    let time = Utc::now().timestamp() as u64;

    // Determine the comparison point
    let (compare_block, collect_window_end) =
        match local_comparison_point(&id, collect_window_duration, db.clone()).await {
            Ok((block, window)) if time >= window => (block, window),
            Ok((block, _window)) => {
                // Construct error for early comparison attempt
                return Err(OperationError::CompareTrigger(
                    id,
                    block,
                    "Comparison window has not yet ended".to_string(),
                ));
            }
            Err(e) => {
                return Err(OperationError::ComparisonError(e));
            }
        };

    let remote_ppoi_messages = match get_remote_ppoi_messages_by_identifier(&db, &id).await {
        Ok(messages) => messages,
        Err(e) => {
            return Err(OperationError::Database(e));
        }
    };

    // Filter messages for the current comparison
    let filtered_messages = remote_ppoi_messages
        .into_iter()
        .filter(|m| m.payload.block_number == compare_block && m.nonce <= collect_window_end)
        .collect::<Vec<_>>();

    // Process the filtered POI messages to get remote attestations
    let remote_attestations = process_ppoi_message(filtered_messages, &callbook)
        .await
        .map_err(OperationError::Attestation)?;

    let local_attestation = get_local_attestation(&db, &id, compare_block)
        .await
        .map_err(OperationError::Database)?
        .ok_or(OperationError::Others(
            "Local attestation record not found".to_string(),
        ))?;

    // Perform the comparison
    let comparison_result = compare_attestations(
        Some(local_attestation),
        compare_block,
        &remote_attestations,
        &id,
    );

    Ok(comparison_result)
}
