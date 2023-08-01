use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::OperationError;

#[derive(Debug, Serialize, Deserialize)]
struct IndexingRuleAttributes {
    id: i32,
    identifier: String,
    identifier_type: String,
    allocation_amount: Option<String>,
    allocation_lifetime: Option<i32>,
    auto_renewal: bool,
    parallel_allocations: Option<i32>,
    max_allocation_percentage: Option<i32>,
    min_signal: Option<String>,
    max_signal: Option<String>,
    min_stake: Option<String>,
    min_average_query_fees: Option<String>,
    custom: Option<String>,
    decision_basis: String,
    require_supported: bool,
    safety: bool,
}

pub async fn health_query(url: &str) -> Result<String, OperationError> {
    let client = Client::new();
    let response = client.get(url).send().await.unwrap();
    response
        .text()
        .await
        .map_err(|e| OperationError::Query(graphcast_sdk::graphql::QueryError::Transport(e)))
}

pub async fn indexing_rules(url: &str) -> Result<serde_json::Value, OperationError> {
    let graphql_query = json!({
        "query": r#"query indexingRules {
                indexingRules {
                    identifier
                    identifierType
                    allocationAmount
                    allocationLifetime
                    autoRenewal
                    parallelAllocations
                    maxAllocationPercentage
                    minSignal
                    maxSignal
                    minStake
                    minAverageQueryFees
                    custom
                    decisionBasis
                    requireSupported
                    safety
                }
            }"#
    });

    let client = Client::new();
    let response = client.post(url).json(&graphql_query).send().await.unwrap();

    response
        .json::<serde_json::Value>()
        .await
        .map_err(|e| OperationError::Query(graphcast_sdk::graphql::QueryError::Transport(e)))
}

pub async fn offchain_sync_indexing_rules(
    url: &str,
    deployment: &str,
) -> Result<serde_json::Value, OperationError> {
    let graphql_mutation = json!({
        "query": r#"mutation updateIndexingRule($rule: IndexingRuleInput!) {
            setIndexingRule(rule: $rule) {
                identifier
                identifierType
                allocationAmount
                allocationLifetime
                autoRenewal
                parallelAllocations
                maxAllocationPercentage
                minSignal
                maxSignal
                minStake
                minAverageQueryFees
                custom
                decisionBasis
                requireSupported
                safety
            }
        }"#,
        "variables": {
            "rule": {
                "identifier": deployment,
                "decisionBasis": "offchain",
                "identifierType": "deployment"
            }
        }
    });

    let client = Client::new();
    let response = client
        .post(url)
        .json(&graphql_mutation)
        .send()
        .await
        .map_err(|e| OperationError::Query(graphcast_sdk::graphql::QueryError::Transport(e)))?;

    response
        .json::<serde_json::Value>()
        .await
        .map_err(|e| OperationError::Query(graphcast_sdk::graphql::QueryError::Transport(e)))
}

// // NOTE: this set of tests can only run in context of running indexer_management server
// #[cfg(test)]
// mod tests {

//     use super::*;

//     // TODO: add setup and teardown functions

//     #[tokio::test]
//     async fn test_basic_request() {
//         let res = health_query("http://127.0.0.1:18000").await.unwrap();

//         assert_eq!(res, "Ready to roll!".to_string());
//     }

//     #[tokio::test]
//     async fn test_query_indexing_rule() {
//         let res_json = indexing_rules("http://127.0.0.1:18000").await;

//         assert!(res_json.is_ok())
//     }

//     #[tokio::test]
//     async fn test_set_offchain_sync() {
//         let res_json = offchain_sync_indexing_rules(
//             "http://127.0.0.1:18000",
//             "Qmb5Ysp5oCUXhLA8NmxmYKDAX2nCMnh7Vvb5uffb9n5vss",
//         )
//         .await;
//         assert!(res_json.is_ok());

//         let check_setting = indexing_rules("http://127.0.0.1:18000").await.unwrap();

//         assert!(check_setting
//             .as_object()
//             .unwrap()
//             .get("data")
//             .unwrap()
//             .as_object()
//             .unwrap()
//             .get("iiterles")
//             .unwrap()
//             .as_array()
//             .unwrap()
//             .into_iter()
//             .any(|o| o
//                 .as_object()
//                 .unwrap()
//                 .get("identifier")
//                 .unwrap()
//                 .as_str()
//                 .unwrap()
//                 == "Qmb5Ysp5oCUXhLA8NmxmYKDAX2nCMnh7Vvb5uffb9n5vss")
//             );
//     }
// }
