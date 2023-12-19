use axum::{response::IntoResponse, Json};
use axum::{routing::post, Router};
use chrono::Utc;
use ethers::signers::Signer;
use graphcast_sdk::{build_wallet, wallet_address};
use serde_json::{json, Value};
use std::sync::Arc;
use std::{convert::Infallible, net::SocketAddr, str::FromStr};
use tokio::sync::Mutex;
use tower::ServiceBuilder;
use tower_http::trace::TraceLayer;

use crate::{
    is_address_derived_from_keys, GRAPHCAST_REGISTERED_ACCOUNTS, GRAPH_NETWORK_ACCOUNTS, INDEXERS,
};

pub struct ServerState {
    subgraphs: Arc<Mutex<Vec<String>>>,
}

impl ServerState {
    pub async fn update_subgraphs(&self, new_subgraphs: Vec<String>) {
        let mut subgraphs = self.subgraphs.lock().await;
        *subgraphs = new_subgraphs;
    }
}

pub async fn start_mock_server(address: String, initial_subgraphs: Vec<String>) -> ServerState {
    let subgraphs = Arc::new(Mutex::new(initial_subgraphs));

    let subgraphs_for_graphql = Arc::clone(&subgraphs);
    let subgraphs_for_network_subgraph = Arc::clone(&subgraphs);

    let app = Router::new()
        .route(
            "/graphql",
            post(move || handler_graphql(subgraphs_for_graphql.clone())),
        )
        .route("/registry-subgraph", post(handler_graphcast_registry))
        .route(
            "/network-subgraph",
            post(move |json_payload: Json<Value>| {
                handler_network_subgraph(subgraphs_for_network_subgraph.clone(), json_payload)
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

    let indexing_statuses: Vec<String> = subgraphs
        .iter()
        .map(|hash| format!(
            r#"{{"subgraph": "{}", "synced": true, "health": "healthy", "node": "default", "fatalError": null, "chains": [{{"network": "mainnet", "latestBlock": {{"number": "{}", "hash": "b30395958a317ccc06da46782f660ce674cbe6792e5573dc630978c506114a0a"}}, "chainHeadBlock": {{"number": "{}", "hash": "b30395958a317ccc06da46782f660ce674cbe6792e5573dc630978c506114a0a"}}}}]}}"#,
            hash, block_number, block_number
        ))
        .collect();

    let indexing_statuses = indexing_statuses.join(",");

    let response_body = format!(
        r#"{{"data": {{"proofOfIndexing": "0x25331f98b82ca7f3966256bf508a7ede52e715b631dfa3d73b846bb7617f6b9e", "blockHashFromNumber": "4dbba1ba9fb18b0034965712598be1368edcf91ae2c551d59462aab578dab9c5", "indexingStatuses": [{}]}}}}"#,
        indexing_statuses
    );

    Ok(response_body)
}

async fn handler_graphcast_registry(
    Json(payload): Json<Value>,
) -> Result<impl IntoResponse, Infallible> {
    let graph_account = payload["variables"]["address"].as_str().unwrap_or_default();

    let mut response_data = json!({});

    for &pk in GRAPHCAST_REGISTERED_ACCOUNTS.iter() {
        if let Ok(wallet) = build_wallet(pk) {
            if wallet_address(&wallet) == graph_account {
                let graphcast_id = wallet.address();

                response_data = json!({
                    "indexer": graph_account,
                    "graphcastID": graphcast_id
                });

                break;
            }
        }
    }

    let response = if response_data != json!({}) {
        json!({
            "data": {
                "graphcast_ids": [response_data]
            }
        })
    } else {
        json!({
            "data": {
                "graphcast_ids": []
            }
        })
    };

    Ok(Json(response))
}

async fn handler_network_subgraph(
    subgraphs: Arc<Mutex<Vec<String>>>,
    Json(payload): Json<Value>,
) -> Result<String, Infallible> {
    let subgraphs = subgraphs.lock().await;

    let allocations: Vec<String> = subgraphs
        .iter()
        .map(|hash| format!(r#"{{"subgraphDeployment": {{"ipfsHash": "{}"}}}}"#, hash))
        .collect();

    let allocations = allocations.join(",");

    let mut graph_account = payload["variables"]["account_addr"]
        .as_str()
        .unwrap_or_default();

    if graph_account.is_empty() {
        graph_account = payload["variables"]["address"].as_str().unwrap();
    }

    if is_address_derived_from_keys(graph_account, &GRAPH_NETWORK_ACCOUNTS) {
        let response_body = format!(
            r#"{{
                "data": {{
                    "graphAccounts": [{{
                        "id": "{}",
                        "operators": [],
                        "subgraphs": [],
                        "indexer": {{
                            "id": "{}",
                            "stakedTokens": "200000000000000000000000",
                            "allocations": [{}]
                        }}
                    }}]
                }},
                "errors": null
            }}"#,
            graph_account, graph_account, allocations
        );

        return Ok(response_body);
    }

    if is_address_derived_from_keys(graph_account, &INDEXERS) {
        let response_body = format!(
            r#"{{
                "data": {{
                    "graphAccounts": [{{
                        "id": "{}",
                        "operators": [],
                        "subgraphs": [],
                        "indexer": {{
                            "id": "{}",
                            "stakedTokens": "200000000000000000000000",
                            "allocations": [{}]
                        }}
                    }}],
                    "indexer": {{
                        "stakedTokens": "200000000000000000000000",
                        "allocations": [{}]
                    }},
                    "graphNetwork": {{
                        "minimumIndexerStake": "10000000000000000000000"
                    }}
                }},
                "errors": null
            }}"#,
            graph_account, graph_account, allocations, allocations
        );

        return Ok(response_body);
    }

    let response_body = r#"
        {{
            "data": {{
                "indexer": {{
                    "stakedTokens": "0",
                    "allocations": []
                }},
                "graphNetwork": {{
                    "minimumIndexerStake": "10000000000000000000000"
                }}
            }},
            "errors": null
        }}"#
    .to_string();

    Ok(response_body)
}
