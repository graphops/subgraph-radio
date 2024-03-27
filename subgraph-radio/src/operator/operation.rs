use autometrics::autometrics;
use sqlx::SqlitePool;

use std::collections::HashMap;
use tracing::{debug, error, warn};

use graphcast_sdk::{
    determine_message_block, graphcast_agent::message_typing::MessageError, networks::NetworkName,
    BlockPointer, NetworkBlockError, NetworkPointer,
};

use crate::database::{clean_remote_ppoi_messages, delete_outdated_local_attestations};
use crate::DatabaseError;

use crate::messages::poi::{poi_message_comparison, send_poi_message};

use crate::{
    operator::{attestation::ComparisonResult, RadioOperator},
    OperationError, GRAPHCAST_AGENT,
};

/// Determine the parameters for messages to send and compare
#[autometrics(track_concurrency)]
pub async fn gossip_set_up(
    id: String,
    network_chainhead_blocks: &HashMap<NetworkName, BlockPointer>,
    subgraph_network_latest_blocks: &HashMap<String, NetworkPointer>,
) -> Result<(NetworkName, BlockPointer, u64), MessageError> {
    // Get the indexing network of the deployment
    // and update the NETWORK message block
    let (network_name, latest_block) = match subgraph_network_latest_blocks.get(&id.clone()) {
        Some(network_block) => (
            NetworkName::from_string(&network_block.network.clone()),
            network_block.block.clone(),
        ),
        None => {
            let err_msg = format!("Could not query the subgraph's indexing network, check Graph node's indexing statuses of subgraph deployment {}", id.clone());
            warn!(
                err = tracing::field::debug(&err_msg),
                "Failed to build message"
            );
            return Err(MessageError::Network(NetworkBlockError::FailedStatus(
                err_msg,
            )));
        }
    };

    let message_block = match determine_message_block(network_chainhead_blocks, network_name) {
        Ok(block) => block,
        Err(e) => return Err(MessageError::Network(e)),
    };

    debug!(
        deployment_hash = tracing::field::debug(&id),
        network = tracing::field::debug(&network_name),
        message_block = message_block,
        latest_block = latest_block.number,
        message_countdown_blocks = message_block.saturating_sub(latest_block.number),
        "Deployment status",
    );

    Ok((network_name, latest_block, message_block))
}

impl RadioOperator {
    pub async fn gossip_poi(
        &self,
        identifiers: Vec<String>,
        network_chainhead_blocks: &HashMap<NetworkName, BlockPointer>,
        subgraph_network_latest_blocks: &HashMap<String, NetworkPointer>,
    ) -> Vec<Result<String, OperationError>> {
        let mut send_handles = vec![];
        let mut send_ops = vec![];

        for id in identifiers.clone() {
            /* Set up */
            let (network_name, latest_block, message_block) = if let Ok(params) = gossip_set_up(
                id.clone(),
                network_chainhead_blocks,
                subgraph_network_latest_blocks,
            )
            .await
            {
                params
            } else {
                let err_msg = "Failed to set up message parameters".to_string();
                warn!(id, err_msg, "Gossip POI failed");
                send_ops.push(Err(OperationError::SendMessage(format!(
                    "Gossip POI failed, id: {} error: {}",
                    id, err_msg
                ))));
                continue;
            };

            /* Send message */
            let id_cloned = id.clone();
            let callbook = self.config.callbook();
            let db = self.db.clone();

            let send_handle = tokio::spawn(async move {
                send_poi_message(
                    id_cloned,
                    callbook,
                    message_block,
                    latest_block,
                    network_name,
                    GRAPHCAST_AGENT.get().unwrap(),
                    db,
                )
                .await
            });

            send_handles.push(send_handle);
        }

        for handle in send_handles {
            if let Ok(s) = handle.await {
                send_ops.push(s);
            }
        }
        send_ops
    }

    pub async fn compare_poi(
        &self,
        identifiers: Vec<String>,
    ) -> Vec<Result<ComparisonResult, OperationError>> {
        let mut compare_handles = vec![];

        for id in identifiers {
            let collect_duration = self.config.radio_setup.collect_message_duration;
            let callbook = self.config.callbook().clone();
            let db = self.db.clone();

            let compare_handle = tokio::spawn(async move {
                poi_message_comparison(id.clone(), collect_duration, callbook, db)
                    .await
                    .map_err(OperationError::from)
            });
            compare_handles.push(compare_handle);
        }

        let mut compare_ops = vec![];
        for handle in compare_handles {
            match handle.await {
                Ok(result) => match result {
                    Ok(r) => {
                        compare_ops.push(Ok(r.clone()));
                        if let Err(_e) = Self::cleanup_after_comparison(&self.db.clone(), &r).await
                        {
                            error!("Error clearing old db items");
                        }
                    }
                    Err(e) => {
                        compare_ops.push(Err(e));
                    }
                },
                Err(e) => {
                    compare_ops.push(Err(OperationError::Others(format!("Task failed: {:?}", e))));
                }
            }
        }

        compare_ops
    }

    async fn cleanup_after_comparison(
        db_pool: &SqlitePool,
        result: &ComparisonResult,
    ) -> Result<(), OperationError> {
        let block_number: u64 = result.block();

        delete_outdated_local_attestations(db_pool, &result.deployment_hash(), block_number)
            .await
            .map_err(|e| OperationError::Database(DatabaseError::CoreSqlx(e)))?;

        clean_remote_ppoi_messages(db_pool, &result.deployment_hash(), block_number)
            .await
            .map_err(|e| OperationError::Database(DatabaseError::CoreSqlx(e)))?;

        Ok(())
    }
}
