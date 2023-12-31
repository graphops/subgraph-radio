use async_graphql::SimpleObject;
use ethers_contract::EthAbiType;
use ethers_core::types::transaction::eip712::Eip712;
use ethers_derive_eip712::*;
use prost::Message;
use serde::{Deserialize, Serialize};

use sqlx::SqlitePool;
use tracing::{debug, info};

use graphcast_sdk::{
    graphcast_agent::message_typing::{GraphcastMessage, MessageError, RadioPayload},
    graphql::client_graph_account::{owned_subgraphs, subgraph_hash_by_id},
};

use crate::{config::Config, DatabaseError};
use crate::{database::recent_upgrade, operator::notifier::Notifier};
use crate::{
    operator::indexer_management::{check_decision_basis, offchain_sync_indexing_rules},
    OperationError,
};

#[derive(Eip712, EthAbiType, Clone, Message, Serialize, Deserialize, PartialEq, SimpleObject)]
#[eip712(
    name = "UpgradeIntentMessage",
    version = "0",
    chain_id = 1,
    verifying_contract = "0xc944e90c64b2c07662a292be6244bdf05cda44a7"
)]
pub struct UpgradeIntentMessage {
    /// current subgraph deployment hash
    #[prost(string, tag = "1")]
    pub deployment: String,
    /// subgraph id shared by both versions of the subgraph deployment
    #[prost(string, tag = "2")]
    pub subgraph_id: String,
    // new version of the subgraph has a new deployment hash
    #[prost(string, tag = "3")]
    pub new_hash: String,
    /// nonce cached to check against the next incoming message
    #[prost(uint64, tag = "4")]
    pub nonce: u64,
    /// Graph account sender - expect the sender to be subgraph owner
    #[prost(string, tag = "5")]
    pub graph_account: String,
}

impl RadioPayload for UpgradeIntentMessage {
    /// Check duplicated fields: payload message has duplicated fields with GraphcastMessage, the values must be the same
    fn valid_outer(&self, outer: &GraphcastMessage<Self>) -> Result<&Self, MessageError> {
        if self.nonce == outer.nonce && self.graph_account == outer.graph_account {
            Ok(self)
        } else {
            Err(MessageError::InvalidFields(anyhow::anyhow!(
                "Radio message wrapped by inconsistent GraphcastMessage: {:#?} <- {:#?}",
                &self,
                &outer,
            )))
        }
    }
}

impl UpgradeIntentMessage {
    /// Check message from valid sender: check for ownership for subgraph-owner messages
    pub async fn valid_owner(&self, network_subgraph: &str) -> Result<&Self, MessageError> {
        let subgraphs = owned_subgraphs(network_subgraph, &self.graph_account)
            .await
            .map_err(MessageError::FieldDerivations)?;
        if !subgraphs.contains(&self.subgraph_id) {
            return Err(MessageError::InvalidFields(anyhow::anyhow!(format!(
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
    ) -> Result<&Self, MessageError> {
        let _ = self
            .valid_owner(graph_network)
            .await
            .map(|radio_msg| radio_msg.valid_outer(gc_msg))??;
        Ok(self)
    }

    /// Format the notification for an UpgradeIntentMessage
    fn notification_formatter(&self) -> String {
        format!(
            "Subgraph owner for a deployment announced an upgrade intent:\nSubgraph ID: {}\nNew deployment hash: {}",
            self.subgraph_id,
            self.new_hash,
        )
    }

    /// Process the validated upgrade intent messages
    /// If notification is set up, then notify the indexer
    /// If indexer management server endpoint is set up, radio checks `auto_upgrade_coverage` for
    pub async fn process_valid_message(
        &self,
        config: &Config,
        notifier: &Notifier,
        db: &SqlitePool,
    ) -> Result<&Self, OperationError> {
        // ratelimit upgrades: return early if there was a recent upgrade
        let recent_upgrade_result =
            recent_upgrade(db, self, config.radio_setup.auto_upgrade_ratelimit)
                .await
                .map_err(|e| OperationError::Database(DatabaseError::CoreSqlx(e)))?;

        if recent_upgrade_result {
            info!(subgraph = &self.subgraph_id, "Received an Upgrade Intent Message for a recently upgraded subgraph, skipping notification and auto deployment");
            return Ok(self);
        }
        // send notifications
        notifier.notify(self.notification_formatter()).await;

        // auto-deployment
        // require configured indexer management server endpoint
        if let Some(url) = &config.graph_stack().indexer_management_server_endpoint {
            // If the identifier satisfy the config coverage level
            let covered_topics = config
                .generate_topics(&config.radio_setup().auto_upgrade_coverage)
                .await;
            // Get the current deployment hash by querying network subgraph and take the latest hash of the subgraph id
            // Should be able to assume valid identifier since the message is valid
            let identifier = subgraph_hash_by_id(
                config.graph_stack().network_subgraph(),
                &self.graph_account,
                &self.subgraph_id,
            )
            .await
            .map_err(OperationError::Query)?;

            if covered_topics
                .clone()
                .into_iter()
                .any(|x| x == identifier.clone())
            {
                // Get protocol network
                let network = config
                    .protocol_network()
                    .map_err(|e| OperationError::Others(e.to_string()))?;
                let res = offchain_sync_indexing_rules(url, &self.new_hash, &network).await;
                let decision_basis = check_decision_basis(url, &self.new_hash).await;
                debug!(
                    res = tracing::field::debug(&res),
                    decision_basis, "New deployment setting"
                );
            };
        }
        Ok(self)
    }
}
