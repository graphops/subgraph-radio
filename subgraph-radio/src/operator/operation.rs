use autometrics::autometrics;

use std::cmp::max;
use std::collections::HashMap;
use std::sync::Arc;
use tracing::{debug, trace, warn};

use graphcast_sdk::{
    determine_message_block, graphcast_agent::message_typing::BuildMessageError,
    networks::NetworkName, BlockPointer, NetworkBlockError, NetworkPointer,
};

use crate::messages::poi::{poi_message_comparison, send_poi_message};

use crate::{
    metrics::CACHED_PPOI_MESSAGES,
    operator::{attestation::ComparisonResult, RadioOperator},
    OperationError, GRAPHCAST_AGENT,
};

/// Determine the parameters for messages to send and compare
#[autometrics(track_concurrency)]
pub async fn gossip_set_up(
    id: String,
    network_chainhead_blocks: &HashMap<NetworkName, BlockPointer>,
    subgraph_network_latest_blocks: &HashMap<String, NetworkPointer>,
) -> Result<(NetworkName, BlockPointer, u64), BuildMessageError> {
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
            return Err(BuildMessageError::Network(NetworkBlockError::FailedStatus(
                err_msg,
            )));
        }
    };

    let message_block = match determine_message_block(network_chainhead_blocks, network_name) {
        Ok(block) => block,
        Err(e) => return Err(BuildMessageError::Network(e)),
    };

    debug!(
        deployment_hash = tracing::field::debug(&id),
        network = tracing::field::debug(&network_name),
        message_block = message_block,
        latest_block = latest_block.number,
        message_countdown_blocks = max(0, message_block as i64 - latest_block.number as i64),
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
                continue;
            };

            /* Send message */
            let id_cloned = id.clone();

            let callbook = self.config.callbook();
            let local_attestations = self.persisted_state.local_attestations.clone();
            let send_handle = tokio::spawn(async move {
                send_poi_message(
                    id_cloned,
                    callbook,
                    message_block,
                    latest_block,
                    network_name,
                    Arc::clone(&local_attestations),
                    GRAPHCAST_AGENT.get().unwrap(),
                )
                .await
            });

            send_handles.push(send_handle);
        }

        let mut send_ops = vec![];
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

        // Additional radio message check happens here since messages are synchronously stored to state cache in msg handler
        let remote_ppoi_messages = self
            .state()
            .valid_ppoi_messages(&self.config.graph_stack().graph_node_status_endpoint)
            .await;

        for id in identifiers.clone() {
            /* Set up */
            let collect_duration: i64 = self
                .config
                .radio_infrastructure
                .collect_message_duration
                .to_owned();
            let id_cloned = id.clone();
            let callbook = self.config.callbook();
            let local_attestations = self.state().local_attestations();
            let filtered_msg = remote_ppoi_messages
                .iter()
                .filter(|&m| m.identifier == id.clone())
                .cloned()
                .collect();

            let compare_handle = tokio::spawn(async move {
                poi_message_comparison(
                    id_cloned,
                    collect_duration,
                    callbook.clone(),
                    filtered_msg,
                    local_attestations,
                )
                .await
            });
            compare_handles.push(compare_handle);
        }

        let mut compare_ops = vec![];
        for handle in compare_handles {
            let res = handle.await;
            if let Ok(s) = res {
                // Skip clean up for comparisonResult for Error and buildFailed
                match s {
                    Ok(r) => {
                        compare_ops.push(Ok(r.clone()));

                        /* Clean up cache */
                        // Only clear the ones matching identifier and block number equal or less
                        // Retain the msgs with a different identifier, or if their block number is greater
                        // clear_local_attestation(&mut local_attestations, r.deployment_hash(), r.block());
                        self.persisted_state
                            .clean_local_attestations(r.block(), r.deployment_hash());
                        self.persisted_state
                            .clean_remote_ppoi_messages(r.block(), r.deployment_hash());
                        CACHED_PPOI_MESSAGES
                            .with_label_values(&[&r.deployment_hash()])
                            .set(
                                self.state()
                                    .remote_ppoi_messages()
                                    .len()
                                    .try_into()
                                    .unwrap(),
                            );
                    }
                    Err(e) => {
                        trace!(err = tracing::field::debug(&e), "Compare handles");

                        compare_ops.push(Err(e.clone_with_inner()));
                    }
                }
            }
        }
        compare_ops
    }
}
