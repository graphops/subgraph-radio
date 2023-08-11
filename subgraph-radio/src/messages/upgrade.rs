use async_graphql::SimpleObject;

use ethers_contract::EthAbiType;
use ethers_core::types::transaction::eip712::Eip712;
use ethers_derive_eip712::*;
use graphcast_sdk::graphql::client_graph_account::owned_subgraphs;
use graphcast_sdk::{
    graphcast_agent::message_typing::{BuildMessageError, GraphcastMessage},
    networks::NetworkName,
};
use prost::Message;
use serde::{Deserialize, Serialize};
use tracing::debug;

use crate::config::Config;
use crate::operator::indexer_management::{check_decision_basis, offchain_sync_indexing_rules};
use crate::operator::notifier::Notifier;

#[derive(Eip712, EthAbiType, Clone, Message, Serialize, Deserialize, PartialEq, SimpleObject)]
#[eip712(
    name = "VersionUpgradeMessage",
    version = "0",
    chain_id = 1,
    verifying_contract = "0xc944e90c64b2c07662a292be6244bdf05cda44a7"
)]
pub struct VersionUpgradeMessage {
    // identify through the current subgraph deployment
    #[prost(string, tag = "1")]
    pub identifier: String,
    // new version of the subgraph has a new deployment hash
    #[prost(string, tag = "2")]
    pub new_hash: String,
    /// subgraph id shared by both versions of the subgraph deployment
    #[prost(string, tag = "6")]
    pub subgraph_id: String,
    /// nonce cached to check against the next incoming message
    #[prost(int64, tag = "3")]
    pub nonce: i64,
    /// blockchain relevant to the message
    #[prost(string, tag = "4")]
    pub network: String,
    /// estimated timestamp for the usage to switch to the new version
    #[prost(int64, tag = "5")]
    pub migrate_time: i64,
    /// Graph account sender - expect the sender to be subgraph owner
    #[prost(string, tag = "7")]
    pub graph_account: String,
}

impl VersionUpgradeMessage {
    pub fn new(
        identifier: String,
        new_hash: String,
        subgraph_id: String,
        nonce: i64,
        network: String,
        migrate_time: i64,
        graph_account: String,
    ) -> Self {
        VersionUpgradeMessage {
            identifier,
            new_hash,
            subgraph_id,
            nonce,
            network,
            migrate_time,
            graph_account,
        }
    }

    pub fn build(
        identifier: String,
        new_hash: String,
        timestamp: i64,
        subgraph_id: String,
        network: NetworkName,
        migrate_time: i64,
        graph_account: String,
    ) -> Self {
        VersionUpgradeMessage::new(
            identifier,
            new_hash,
            subgraph_id,
            timestamp,
            network.to_string(),
            migrate_time,
            graph_account,
        )
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
                "Radio message wrapped by inconsistent GraphcastMessage: {:#?} <- {:#?}",
                &self,
                &outer,
            )))
        }
    }

    /// Check message from valid sender: check for ownership for subgraph-owner messages
    pub async fn valid_owner(&self, network_subgraph: &str) -> Result<&Self, BuildMessageError> {
        let subgraphs = owned_subgraphs(network_subgraph, &self.graph_account)
            .await
            .map_err(BuildMessageError::FieldDerivations)?;
        if !subgraphs.contains(&self.subgraph_id) {
            return Err(BuildMessageError::InvalidFields(anyhow::anyhow!(format!(
                "Verified account failed to be subgraph owner. Verified account: {:#?}",
                self.graph_account
            ))));
        }
        Ok(self)
    }

    /// Make sure all messages stored are valid
    pub async fn validity_check(
        &self,
        gc_msg: &GraphcastMessage<Self>,
        graph_network: &str,
    ) -> Result<&Self, BuildMessageError> {
        let _ = self
            .valid_owner(graph_network)
            .await
            .map(|radio_msg| radio_msg.valid_outer(gc_msg))??;
        Ok(self)
    }

    /// process the validated version upgrade messages, currently just notify
    pub async fn process_valid_message(&self, config: &Config, notifier: &Notifier) {
        // send notifications
        notifier.notify(format!(
            "Subgraph owner for a deployment has shared version upgrade info:\nold deployment: {}\nnew deployment: {}\nplanned migrate time: {}\nnetwork: {}",
            self.identifier,
            self.new_hash,
            self.migrate_time,
            self.network
        )).await;
        // auto deployment
        // If the identifier satisfy the config coverage level
        // and if indexer management server endpoint is provided
        if let Some(url) = &config.graph_stack().indexer_management_server_endpoint {
            let covered_topics = config
                .generate_topics(config.graph_stack().indexer_address.clone())
                .await;

            if covered_topics
                .clone()
                .into_iter()
                .any(|x| x == self.identifier.clone())
            {
                let res = offchain_sync_indexing_rules(url, &self.new_hash).await;
                let decision_basis = check_decision_basis(url, &self.new_hash).await;
                debug!(
                    res = tracing::field::debug(&res),
                    decision_basis, "New deployment setting"
                );
            }
        }
    }
}
