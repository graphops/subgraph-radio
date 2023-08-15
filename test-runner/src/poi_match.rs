use serde::Deserialize;
use serde_json::json;
use subgraph_radio::{operator::attestation::ComparisonResultType, state::PersistedState};
use test_utils::{
    config::{test_config, TestSenderConfig},
    setup, teardown,
};
use tokio::time::{sleep, Duration};
use tracing::debug;

#[derive(Debug, Deserialize)]
pub struct GraphQlResponse {
    pub data: GraphQlData,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphQlData {
    pub comparison_results: Vec<GraphQlComparisonResult>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphQlComparisonResult {
    pub deployment: String,
    pub block_number: u64,
    pub result_type: ComparisonResultType,
    pub local_attestation: Option<GraphQlAttestation>,
    pub attestations: Vec<GraphQlAttestation>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphQlAttestation {
    pub ppoi: String,
}

pub async fn poi_match_test() {
    let test_file_name = "poi_match";
    let store_path = format!("./test-runner/state/{}.json", test_file_name);

    let radio_topics = vec!["Qmdefault1AbcDEFghijKLmnoPQRstUVwxYzABCDEFghijklmnopq".to_string()];

    let test_sender_topics =
        vec!["Qmdefault1AbcDEFghijKLmnoPQRstUVwxYzABCDEFghijklmnopq".to_string()];

    let mut config = test_config();

    config.radio_infrastructure.persistence_file_path = Some(store_path.clone());
    config.radio_infrastructure.topics = radio_topics.clone();
    config.radio_infrastructure.server_port = Some(3012);

    let mut test_sender_config = TestSenderConfig {
        topics: test_sender_topics,
        radio_name: String::new(),
        block_hash: None,
        staked_tokens: None,
        nonce: None,
        radio_payload: None,
        poi: None,
        private_key: None,
    };

    let process_manager = setup(&config, test_file_name, &mut test_sender_config).await;

    sleep(Duration::from_secs(550)).await;

    let persisted_state = PersistedState::load_cache(&store_path);
    debug!("persisted state {:?}", persisted_state);

    let comparison_results = persisted_state.comparison_results();
    let remote_ppoi_messages = persisted_state.remote_ppoi_messages();

    assert!(
        !comparison_results.is_empty(),
        "The comparison results should not be empty"
    );

    let has_match_result = comparison_results.iter().any(|result| {
        result.1.deployment == "Qmdefault1AbcDEFghijKLmnoPQRstUVwxYzABCDEFghijklmnopq"
            && result.1.result_type == ComparisonResultType::Match
    });

    assert!(
        has_match_result,
        "No comparison result found with deployment 'Qmdefault1AbcDEFghijKLmnoPQRstUVwxYzABCDEFghijklmnopq' and result type 'Match'"
    );

    // GraphQL query
    let graphql_url = "http://localhost:3012/api/v1/graphql";
    let query = r#"
     query {
         comparisonResults(identifier: "Qmdefault1AbcDEFghijKLmnoPQRstUVwxYzABCDEFghijklmnopq") {
             deployment
             blockNumber
             resultType
             localAttestation {
                 ppoi
             }
             attestations {
                 ppoi
             }
         }
     }
 "#;

    let request_body = json!({
        "query": query,
    });

    let client = reqwest::Client::new();
    let res = client
        .post(graphql_url)
        .json(&request_body)
        .send()
        .await
        .expect("Failed to send request");

    let res_text = res.text().await.expect("Failed to read response");

    let graphql_res: GraphQlResponse =
        serde_json::from_str(&res_text).expect("Failed to parse GraphQL response");

    // Count unique deployments
    let unique_deployments_count = graphql_res
        .data
        .comparison_results
        .iter()
        .map(|result| &result.deployment)
        .collect::<std::collections::HashSet<_>>()
        .len();

    // Assert there is only one result per deployment
    assert_eq!(
        unique_deployments_count,
        graphql_res.data.comparison_results.len(),
        "There is more than one result for a deployment"
    );

    for graphql_result in &graphql_res.data.comparison_results {
        // find a corresponding local result
        let local_result = comparison_results
            .get(&graphql_result.deployment)
            .expect("Local result not found");

        assert_eq!(
            graphql_result.deployment, local_result.deployment,
            "Deployments do not match"
        );
        assert_eq!(
            graphql_result.result_type.to_string().to_lowercase(),
            local_result.result_type.to_string().to_lowercase(),
            "Result types do not match"
        );

        // compare local and graphql attestations
        if let Some(local_attestation) = &local_result.local_attestation {
            if let Some(graphql_attestation) = &graphql_result.local_attestation {
                assert_eq!(
                    graphql_attestation.ppoi, local_attestation.ppoi,
                    "Local attestation ppoi does not match"
                );
            } else {
                panic!("GraphQL result lacks local attestation");
            }
        }

        // compare all attestations
        assert_eq!(
            graphql_result.attestations.len(),
            local_result.attestations.len(),
            "Number of attestations do not match"
        );
        for (idx, graphql_attestation) in graphql_result.attestations.iter().enumerate() {
            let local_attestation = &local_result.attestations[idx];
            assert_eq!(
                graphql_attestation.ppoi, local_attestation.ppoi,
                "Attestation ppoi does not match"
            );
        }

        debug!("remote 55 {:?}", &remote_ppoi_messages);

        for remote_message in &remote_ppoi_messages {
            assert!(
                remote_message.payload.block_number >= graphql_result.block_number,
                "Remote message found with block number earlier than comparison result's attested block"
            );
        }
    }

    teardown(process_manager, &store_path);
}
