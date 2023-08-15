use subgraph_radio::state::PersistedState;
use test_utils::{
    config::{test_config, TestSenderConfig},
    messages_are_equal, payloads_are_equal, setup, teardown,
};
use tokio::time::{sleep, Duration};
use tracing::{debug, trace};

use serde::Deserialize;
use serde_json::json;

use crate::poi_match::GraphQlAttestation;

// Add these structs right below your other ones:
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct GraphQlResponseLocal {
    data: GraphQlDataLocal,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
pub struct GraphQlDataLocal {
    local_attestations: Vec<GraphQlLocalAttestation>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
pub struct GraphQlLocalAttestation {
    deployment: String,
    block_number: u64,
    attestation: GraphQlAttestation,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
pub struct GraphQlDataRemote {
    pub radio_payload_messages: Vec<GraphQlRemoteMessage>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
pub struct GraphQlRemoteMessage {
    identifier: String,
    pub payload: GraphQlPayload,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
pub struct GraphQlPayload {
    pub content: String,
    pub block_number: u64,
}

#[derive(Debug, Deserialize)]
pub struct GraphQlResponseRemote {
    pub data: GraphQlDataRemote,
}

pub async fn send_and_receive_test() {
    let test_file_name = "message_handling";
    let store_path = format!("./test-runner/state/{}.json", test_file_name);

    let radio_topics = vec![
        "Qmdefault1AbcDEFghijKLmnoPQRstUVwxYzABCDEFghijklmnopq".to_string(),
        "Qmdefault2XyZABCdefGHIjklMNOpqrstuvWXYZabcdefGHIJKLMN".to_string(),
    ];

    let test_sender_topics =
        vec!["Qmdefault1AbcDEFghijKLmnoPQRstUVwxYzABCDEFghijklmnopq".to_string()];

    let mut config = test_config();

    config.radio_infrastructure.persistence_file_path = Some(store_path.clone());
    config.radio_infrastructure.topics = radio_topics.clone();
    config.radio_infrastructure.topic_update_interval = 10;
    config.radio_infrastructure.server_port = Some(3013);

    let mut test_sender_config = TestSenderConfig {
        topics: test_sender_topics,
        radio_name: String::new(),
        block_hash: None,
        staked_tokens: None,
        nonce: None,
        radio_payload: Some("all".to_string()),
        poi: None,
        private_key: None,
    };

    let process_manager = setup(&config, test_file_name, &mut test_sender_config).await;

    sleep(Duration::from_secs(85)).await;

    let persisted_state = PersistedState::load_cache(&store_path);
    debug!("persisted state {:?}", persisted_state);

    let local_attestations = persisted_state.local_attestations();
    debug!(
        "local attestations {:#?}, \nchecking result: {:#?}",
        local_attestations,
        !local_attestations.is_empty()
    );

    let remote_ppoi_messages = persisted_state.remote_ppoi_messages();

    // GraphQL query for local attestations
    let graphql_url = "http://localhost:3013/api/v1/graphql";
    let local_attestation_query = r#"
        query {
            localAttestations {
                deployment
                blockNumber
                attestation {
                    ppoi
                }
            }
        }
    "#;

    let request_body = json!({
        "query": local_attestation_query,
    });

    let client = reqwest::Client::new();
    let res = client
        .post(graphql_url)
        .json(&request_body)
        .send()
        .await
        .expect("Failed to send request");

    let res_text = res.text().await.expect("Failed to read response");

    // If you want to parse it into your defined structs (not necessary for this step but might be useful later)
    let graphql_res: GraphQlResponseLocal =
        serde_json::from_str(&res_text).expect("Failed to parse GraphQL response");

    debug!("GraphQL response {:?}", graphql_res);

    // Now we fetch remote messages.
    let remote_message_query = r#"
    query {
        radioPayloadMessages {
            identifier
            payload {
                content
                blockNumber
            }
        }
    }
    "#;

    let request_body = json!({
        "query": remote_message_query,
    });

    let client = reqwest::Client::new();
    let res = client
        .post(graphql_url)
        .json(&request_body)
        .send()
        .await
        .expect("Failed to send request");

    let res_text = res.text().await.expect("Failed to read response");

    let graphql_res_remote: GraphQlResponseRemote =
        serde_json::from_str(&res_text).expect("Failed to parse GraphQL response");

    debug!("GraphQL response {:?}", graphql_res_remote.data);

    assert!(
        !local_attestations.is_empty(),
        "There should be at least one element in local_attestations"
    );

    for test_hash in radio_topics {
        assert!(
            local_attestations.contains_key(&test_hash),
            "No attestation found with ipfs hash {}",
            test_hash
        );
    }

    let test_hashes_remote = vec!["Qmdefault1AbcDEFghijKLmnoPQRstUVwxYzABCDEFghijklmnopq"];

    for target_id in test_hashes_remote {
        let has_target_id = remote_ppoi_messages
            .iter()
            .any(|msg| msg.identifier == *target_id);
        assert!(
            has_target_id,
            "No remote message found with identifier {}",
            target_id
        );
    }

    trace!("Num of remote messages {}", remote_ppoi_messages.len());

    assert!(
        remote_ppoi_messages.len() >= 5,
        "The number of remote messages should at least 5. Actual: {}",
        remote_ppoi_messages.len()
    );

    for (index, message1) in remote_ppoi_messages.iter().enumerate() {
        for message2 in remote_ppoi_messages.iter().skip(index + 1) {
            if messages_are_equal(message1, message2)
                && payloads_are_equal(&message1.payload, &message2.payload)
            {
                panic!(
                    "Duplicate remote message found with identifier {}",
                    message1.identifier
                );
            }
        }
    }

    // We do the teardown after we've run all the tests.
    teardown(process_manager, &store_path);
}
