use axum::{routing::post, Router};
use chrono::Utc;
use serde_json::json;
use std::sync::Arc;
use std::{convert::Infallible, net::SocketAddr, str::FromStr};
use tokio::sync::Mutex;
use tower::ServiceBuilder;
use tower_http::trace::TraceLayer;

use crate::{GRAPHCAST_REGISTERED, INDEXERS};

pub struct ServerState {
    subgraphs: Arc<Mutex<Vec<String>>>,
}

impl ServerState {
    pub async fn update_subgraphs(&self, new_subgraphs: Vec<String>) {
        let mut subgraphs = self.subgraphs.lock().await;
        *subgraphs = new_subgraphs;
    }
}

pub async fn start_mock_server(
    address: String,
    initial_subgraphs: Vec<String>,
    staked_tokens: Option<String>,
    sender: String,
) -> ServerState {
    let subgraphs = Arc::new(Mutex::new(initial_subgraphs));

    let subgraphs_for_graphql = Arc::clone(&subgraphs);
    let subgraphs_for_network_subgraph = Arc::clone(&subgraphs);

    let sender_for_network_subgraph = sender.clone();

    let app = Router::new()
        .route(
            "/graphql",
            post(move || handler_graphql(subgraphs_for_graphql.clone())),
        )
        .route(
            "/registry-subgraph",
            post(move || handler_graphcast_registry(sender.clone())),
        )
        .route(
            "/network-subgraph",
            post(move || {
                handler_network_subgraph(
                    subgraphs_for_network_subgraph.clone(),
                    staked_tokens.unwrap_or("10000000000000000000000".to_string()),
                    sender_for_network_subgraph,
                )
            }),
        )
        .layer(
            ServiceBuilder::new()
                .layer(TraceLayer::new_for_http())
                .into_inner(),
        );

    let addr = SocketAddr::from_str(&address).unwrap();
    tokio::spawn(axum::Server::bind(&addr).serve(app.into_make_service()));

    ServerState { subgraphs }
}

async fn handler_graphql(subgraphs: Arc<Mutex<Vec<String>>>) -> Result<String, Infallible> {
    let timestamp = Utc::now().timestamp();
    let block_number = (timestamp + 9) / 10 * 10;
    let subgraphs = subgraphs.lock().await;

    // Prepare indexingStatuses part of the response dynamically from the subgraphs vector
    let indexing_statuses: Vec<String> = subgraphs
        .iter()
        .map(|hash| format!(
            r#"{{"subgraph": "{}", "synced": true, "health": "healthy", "node": "default", "fatalError": null, "chains": [{{"network": "mainnet", "latestBlock": {{"number": "{}", "hash": "b30395958a317ccc06da46782f660ce674cbe6792e5573dc630978c506114a0a"}}, "chainHeadBlock": {{"number": "{}", "hash": "b30395958a317ccc06da46782f660ce674cbe6792e5573dc630978c506114a0a"}}}}]}}"#,
            hash, block_number, block_number
        ))
        .collect();

    // Prepare indexingStatuses part of the response by joining individual status strings with comma
    let indexing_statuses = indexing_statuses.join(",");

    let response_body = format!(
        r#"{{"data": {{"proofOfIndexing": "0x25331f98b82ca7f3966256bf508a7ede52e715b631dfa3d73b846bb7617f6b9e", "blockHashFromNumber": "4dbba1ba9fb18b0034965712598be1368edcf91ae2c551d59462aab578dab9c5", "indexingStatuses": [{}]}}}}"#,
        indexing_statuses
    );

    Ok(response_body)
}

async fn handler_graphcast_registry(sender: String) -> Result<String, Infallible> {
    let response_body = if GRAPHCAST_REGISTERED.contains(&sender) {
        format!(
            r#"{{
            "data": {{
                "graphcast_ids": [
                    {{
                        "indexer": "{}",
                        "graphcastID": "{}"
                    }}
                ]
            }},
            "errors": null,
            "extensions": null
        }}"#,
            sender, sender
        )
    } else {
        r#"{
            "data": {
                "graphcast_ids": []
            },
            "errors": null,
            "extensions": null
        }"#
        .to_string()
    };

    Ok(response_body)
}

async fn handler_network_subgraph(
    subgraphs: Arc<Mutex<Vec<String>>>,
    staked_tokens: String,
    sender: String,
) -> Result<String, Infallible> {
    let subgraphs = subgraphs.lock().await;

    // Prepare allocations part of the response dynamically from the subgraphs vector
    let allocations: Vec<serde_json::Value> = subgraphs
        .iter()
        .map(|hash| json!({"subgraphDeployment": {"ipfsHash": hash}}))
        .collect();

    let response_body = if INDEXERS.contains(&sender) {
        json!({
            "data": {
                "indexer": {
                    "stakedTokens": staked_tokens,
                    "allocations": allocations,
                },
                "graphNetwork": {
                    "minimumIndexerStake": "10000000000000000000000",
                },
                "graphAccounts": [
                    {
                        "id": sender,
                        "operators": [],
                        "subgraphs": []
                      },
                ],            },
            "errors": null,
        })
    } else {
        json!({
            "data": {
                "indexer": null,
                "graphNetwork": {
                    "minimumIndexerStake": "10000000000000000000000",
                },
                "graphAccounts": [],
            },
            "errors": null,
        })
    };

    Ok(response_body.to_string())
}
