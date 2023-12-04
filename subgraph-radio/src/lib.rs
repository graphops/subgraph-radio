use async_graphql::{Error, ErrorExtensions};
use autometrics::autometrics;

use axum_server::Handle;
use derive_getters::Getters;
use once_cell::sync::OnceCell;
use std::{
    collections::HashMap,
    fmt,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};
use tokio::signal;
use tracing::{debug, error, info};

use crate::operator::{attestation::AttestationError, RadioOperator};
use graphcast_sdk::{
    graphcast_agent::{GraphcastAgent, GraphcastAgentError},
    graphql::{
        client_graph_node::get_indexing_statuses, client_network::query_network_subgraph,
        QueryError,
    },
    networks::NetworkName,
    waku_set_event_callback, BlockPointer,
};
use sqlx::{sqlite::SqliteError, Error as CoreSqlxError, SqlitePool};

pub mod config;
pub mod entities;
pub mod graphql;
pub mod messages;
pub mod metrics;
pub mod operator;
pub mod server;

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
        .map_err(|e| {
            error!(err = tracing::field::debug(&e), "Topic generation error");
        })
        .unwrap_or_default()
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

    waku_set_event_callback(|_| {});
    // Signal the server to shutdown using Handle.
    control
        .metrics_handle
        .graceful_shutdown(Some(Duration::from_secs(1)));
    control
        .server_handle
        .graceful_shutdown(Some(Duration::from_secs(3)));
}

#[derive(Debug)]
pub enum DatabaseError {
    Sqlite(SqliteError),
    CoreSqlx(CoreSqlxError),
    SerializationError(serde_json::Error),
}

impl From<SqliteError> for DatabaseError {
    fn from(err: SqliteError) -> Self {
        DatabaseError::Sqlite(err)
    }
}

impl From<CoreSqlxError> for DatabaseError {
    fn from(err: CoreSqlxError) -> Self {
        DatabaseError::CoreSqlx(err)
    }
}

impl fmt::Display for DatabaseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DatabaseError::Sqlite(err) => write!(f, "SQLite error: {}", err),
            DatabaseError::CoreSqlx(err) => write!(f, "SQLx Core error: {}", err),
            DatabaseError::SerializationError(err) => write!(f, "Serialization error: {}", err),
        }
    }
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
    #[error("Database error: {0}")]
    Database(DatabaseError),
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
    update_event: Duration,
    gossip_event: Duration,
    compare_event: Duration,
    iteration_timeout: Duration,
    update_timeout: Duration,
    gossip_timeout: Duration,
}

impl ControlFlow {
    /// Create basic control flow settings
    fn new() -> Self {
        let running = Arc::new(AtomicBool::new(true));
        let skip_iteration = Arc::new(AtomicBool::new(false));

        let metrics_handle = Handle::new();
        let server_handle = Handle::new();

        let update_event = Duration::from_secs(10);
        let gossip_event = Duration::from_nanos(60);
        let compare_event = Duration::from_secs(300);

        let iteration_timeout = Duration::from_secs(120);
        let update_timeout = Duration::from_secs(5);
        let gossip_timeout = Duration::from_nanos(120);

        ControlFlow {
            running,
            skip_iteration,
            metrics_handle,
            server_handle,
            update_event,
            gossip_event,
            compare_event,
            iteration_timeout,
            update_timeout,
            gossip_timeout,
        }
    }
}
pub async fn create_test_db(connection_string: Option<&str>) -> SqlitePool {
    let pool = SqlitePool::connect(connection_string.unwrap_or("sqlite::memory:"))
        .await
        .expect("Failed to connect to the in-memory database");

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS local_attestations (
            identifier VARCHAR(255) NOT NULL,
            block_number INTEGER NOT NULL,
            ppoi VARCHAR(255) NOT NULL,
            stake_weight INTEGER NOT NULL,
            sender_group_hash VARCHAR(255) NOT NULL,
            UNIQUE (block_number, identifier, ppoi)
        );
        "#,
    )
    .execute(&pool)
    .await
    .unwrap();

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS local_attestation_senders (
            attestation_block_number INTEGER NOT NULL,
            attestation_identifier VARCHAR(255) NOT NULL,
            attestation_ppoi VARCHAR(255) NOT NULL,
            sender VARCHAR(255) NOT NULL,
            FOREIGN KEY (attestation_block_number, attestation_identifier, attestation_ppoi) REFERENCES local_attestations(block_number, identifier, ppoi)
        );
        CREATE INDEX IF NOT EXISTS idx_senders_on_foreign_key ON local_attestation_senders(attestation_block_number, attestation_identifier, attestation_ppoi);
        "#
    )
    .execute(&pool)
    .await
    .unwrap();

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS local_attestation_timestamps (
            attestation_block_number INTEGER NOT NULL,
            attestation_identifier VARCHAR(255) NOT NULL,
            attestation_ppoi VARCHAR(255) NOT NULL,
            timestamp BIGINT NOT NULL,
            FOREIGN KEY (attestation_block_number, attestation_identifier, attestation_ppoi) REFERENCES local_attestations(block_number, identifier, ppoi)
        );
        CREATE INDEX IF NOT EXISTS idx_timestamps_on_foreign_key ON local_attestation_timestamps(attestation_block_number, attestation_identifier, attestation_ppoi);
        "#
    )
    .execute(&pool)
    .await
    .unwrap();

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS remote_ppoi_messages (
            identifier VARCHAR(255) NOT NULL,
            nonce BIGINT NOT NULL,
            graph_account VARCHAR(255) NOT NULL,
            content VARCHAR(255) NOT NULL,
            network VARCHAR(255) NOT NULL,
            block_number BIGINT NOT NULL,
            block_hash VARCHAR(255) NOT NULL,
            signature VARCHAR(255) NOT NULL
        );
        "#,
    )
    .execute(&pool)
    .await
    .unwrap();

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS upgrade_intent_messages (
            deployment VARCHAR(255) NOT NULL,
            nonce BIGINT NOT NULL,
            graph_account VARCHAR(255) NOT NULL,
            subgraph_id VARCHAR(255) NOT NULL PRIMARY KEY,
            new_hash VARCHAR(255) NOT NULL,
            signature VARCHAR(255) NOT NULL
        );
        "#,
    )
    .execute(&pool)
    .await
    .unwrap();

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS comparison_results (
            deployment VARCHAR(255) NOT NULL PRIMARY KEY,
            block_number BIGINT NOT NULL,
            result_type VARCHAR(255) NOT NULL,
            local_attestation_json TEXT NOT NULL,
            attestations_json TEXT NOT NULL
        );
        "#,
    )
    .execute(&pool)
    .await
    .unwrap();

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS notifications (
            deployment VARCHAR(255) UNIQUE NOT NULL,
            message TEXT NOT NULL
        );
        "#,
    )
    .execute(&pool)
    .await
    .unwrap();

    pool
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
