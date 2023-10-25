use async_graphql::{Error, ErrorExtensions};
use autometrics::autometrics;

use axum_server::Handle;
use derive_getters::Getters;
use once_cell::sync::OnceCell;
use std::{
    collections::HashMap,
    process,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread::sleep,
    time::Duration,
};
use tokio::signal;
use tracing::{debug, error, info};

use graphcast_sdk::{
    graphcast_agent::GraphcastAgent, graphql::client_network::query_network_subgraph,
    networks::NetworkName, BlockPointer,
};
use graphcast_sdk::{
    graphcast_agent::GraphcastAgentError,
    graphql::{client_graph_node::get_indexing_statuses, QueryError},
};

use crate::operator::{attestation::AttestationError, RadioOperator};

pub mod config;
pub mod graphql;
pub mod messages;
pub mod metrics;
pub mod operator;
pub mod server;
pub mod state;

/// A global static (singleton) instance of GraphcastAgent. It is useful to ensure that we have only one GraphcastAgent
/// per Radio instance, so that we can keep track of state and more easily test our Radio application.
pub static RADIO_OPERATOR: OnceCell<RadioOperator> = OnceCell::new();

/// A global static (singleton) instance of GraphcastAgent. It is useful to ensure that we have only one GraphcastAgent
/// per Radio instance, so that we can keep track of state and more easily test our Radio application.
pub static GRAPHCAST_AGENT: OnceCell<Arc<GraphcastAgent>> = OnceCell::new();

pub fn radio_name() -> &'static str {
    "subgraph-radio"
}

/// Generate default topics that is operator address resolved to indexer address
/// and then its active on-chain allocations -> function signature should just return
/// A vec of strings for subtopics
pub async fn active_allocation_hashes(
    network_subgraph: &str,
    indexer_address: &str,
) -> Vec<String> {
    query_network_subgraph(network_subgraph, indexer_address)
        .await
        .map(|result| result.indexer_allocations())
        .unwrap_or_else(|e| {
            error!(err = tracing::field::debug(&e), "Failed to generate topics");
            vec![]
        })
}

/// Generate content topics for all deployments that are syncing on Graph node
/// filtering for deployments on an index node
pub async fn syncing_deployment_hashes(
    graph_node_endpoint: &str,
    // graphQL filter
) -> Vec<String> {
    get_indexing_statuses(graph_node_endpoint)
        .await
        .map_err(|e| -> Vec<String> {
            error!(err = tracing::field::debug(&e), "Topic generation error");
            [].to_vec()
        })
        .unwrap()
        .iter()
        .filter(|&status| status.node.is_some() && status.node != Some(String::from("removed")))
        .map(|s| s.subgraph.clone())
        .collect::<Vec<String>>()
}

/// This function returns the string representation of a set of network mapped to their chainhead blocks
#[autometrics]
pub fn chainhead_block_str(
    network_chainhead_blocks: &HashMap<NetworkName, BlockPointer>,
) -> String {
    let mut blocks_str = String::new();
    blocks_str.push_str("{ ");
    for (i, (network, block_pointer)) in network_chainhead_blocks.iter().enumerate() {
        if i > 0 {
            blocks_str.push_str(", ");
        }
        blocks_str.push_str(&format!("{}: {}", network, block_pointer.number));
    }
    blocks_str.push_str(" }");
    blocks_str
}

/// Graceful shutdown when receive signal
pub async fn shutdown(control: ControlFlow) {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {
            info!("Ctrl+C received! Shutting down...");
        }
        _ = terminate => {
            info!("SIGTERM received! Shutting down...");
        }
    }
    // Set running boolean to false
    debug!("Finish the current running processes...");
    control.running.store(false, Ordering::SeqCst);
    // Signal the server to shutdown using Handle.
    control
        .metrics_handle
        .graceful_shutdown(Some(Duration::from_secs(1)));
    control
        .server_handle
        .graceful_shutdown(Some(Duration::from_secs(3)));

    sleep(Duration::from_secs(5));
    debug!("Allowed 5 seconds for graceful shutdown, force exit");
    process::exit(1);
}

#[derive(Debug, thiserror::Error)]
pub enum OperationError {
    #[error("Send message trigger isn't met: {0}")]
    SendTrigger(String),
    #[error("Message sent already, skip to avoid duplicates: {0}")]
    SkipDuplicate(String),
    #[error("Comparison trigger isn't met: {0}")]
    CompareTrigger(String, u64, String),
    #[error("Agent encountered problems: {0}")]
    Agent(GraphcastAgentError),
    #[error("Failed to query: {0}")]
    Query(QueryError),
    #[error("Attestation failure: {0}")]
    Attestation(AttestationError),
    #[error("Others: {0}")]
    Others(String),
}

impl OperationError {
    pub fn clone_with_inner(&self) -> Self {
        match self {
            OperationError::SendTrigger(msg) => OperationError::SendTrigger(msg.clone()),
            OperationError::SkipDuplicate(msg) => OperationError::SkipDuplicate(msg.clone()),
            OperationError::CompareTrigger(d, b, m) => {
                OperationError::CompareTrigger(d.clone(), *b, m.clone())
            }
            e => OperationError::Others(e.to_string()),
        }
    }
}

impl ErrorExtensions for OperationError {
    fn extend(&self) -> Error {
        Error::new(format!("{}", self))
    }
}

/// Aggregated control flow configurations
#[derive(Getters, Debug, Clone)]
pub struct ControlFlow {
    running: Arc<AtomicBool>,
    skip_iteration: Arc<AtomicBool>,
    metrics_handle: Handle,
    server_handle: Handle,
}

impl ControlFlow {
    /// Create basic control flow settings
    fn new() -> Self {
        let running = Arc::new(AtomicBool::new(true));
        let skip_iteration = Arc::new(AtomicBool::new(false));

        let metrics_handle = Handle::new();
        let server_handle = Handle::new();

        //TODO: Test for effectiveness
        // let iteration_timeout = Duration::from_micros(1);
        // let update_timeout = Duration::from_secs(5);
        // let gossip_timeout = Duration::from_nanos(1);

        ControlFlow {
            running,
            skip_iteration,
            metrics_handle,
            server_handle,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::messages::poi::PublicPoiMessage;
    use graphcast_sdk::graphcast_agent::message_typing::{GraphcastMessage, RadioPayload};

    fn simple_message() -> PublicPoiMessage {
        PublicPoiMessage::new(
            String::from("QmHash"),
            String::from("0x0"),
            1,
            String::from("goerli"),
            0,
            String::from("0xa"),
            String::from("0xaaa"),
        )
    }

    fn wrong_outer_message(payload: PublicPoiMessage) -> GraphcastMessage<PublicPoiMessage> {
        GraphcastMessage {
            identifier: String::from("ping-pong-content-topic"),
            payload,
            nonce: 1687448729,
            graph_account: String::from("0xe9a1cabd57700b17945fd81feefba82340d9568f"),
            signature: String::from("2cd3fa305efd9c362bc71adee6e5a85c357a951af84c80667b8ddae23ac81c3821dac7d9c167e2776a9a56d8726b472312f40d9cc7461d1a6950d00e52d6e8521b")
        }
    }

    fn good_outer_message(payload: PublicPoiMessage) -> GraphcastMessage<PublicPoiMessage> {
        GraphcastMessage {
            identifier: payload.identifier.clone(),
            payload: payload.clone(),
            nonce: payload.nonce,
            graph_account: payload.graph_account,
            signature: String::from("2cd3fa305efd9c362bc71adee6e5a85c357a951af84c80667b8ddae23ac81c3821dac7d9c167e2776a9a56d8726b472312f40d9cc7461d1a6950d00e52d6e8521b")
        }
    }

    #[tokio::test]
    async fn test_message_wrap() {
        let wrong_msg = wrong_outer_message(simple_message());
        let right_msg = good_outer_message(simple_message());

        assert!(simple_message().valid_outer(&wrong_msg).is_err());
        assert!(simple_message().valid_outer(&right_msg).is_ok());
    }
}
