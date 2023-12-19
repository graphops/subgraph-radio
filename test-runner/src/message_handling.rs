use graphcast_sdk::graphcast_agent::message_typing::GraphcastMessage;
use sqlx::SqlitePool;
use subgraph_radio::{
    database::{get_local_attestations, get_remote_ppoi_messages},
    messages::poi::PublicPoiMessage,
};
use tempfile::NamedTempFile;
use test_utils::{
    config::{test_config, TestSenderConfig},
    setup, teardown,
};
use tokio::time::{sleep, Duration};
pub fn assert_all_messages_are_public_poi(
    remote_ppoi_messages: &[GraphcastMessage<PublicPoiMessage>],
) {
    for message in remote_ppoi_messages {
        let _ = PublicPoiMessage {
            identifier: message.payload.identifier.clone(),
            content: message.payload.content.clone(),
            nonce: message.payload.nonce,
            network: message.payload.network.clone(),
            block_number: message.payload.block_number,
            block_hash: message.payload.block_hash.clone(),
            graph_account: message.payload.graph_account.clone(),
        };
    }
}

pub async fn send_and_receive_test() {
    let test_file_name = "message_handling";

    let temp_file =
        NamedTempFile::new().expect("Failed to create a temporary file for the database.");
    let db_path = temp_file.path().to_str().unwrap().to_string();

    let radio_topics = vec!["Qm11default1AbcDEFghijKLmnoPQRstUVwxYzABCDEFghijklmnopq".to_string()];

    let test_sender_topics =
        vec!["Qm11default1AbcDEFghijKLmnoPQRstUVwxYzABCDEFghijklmnopq".to_string()];

    let mut config = test_config();
    config.radio_setup.sqlite_file_path = Some(db_path.clone());
    config.radio_setup.topics = radio_topics.clone();
    config.radio_setup.topic_update_interval = 90;

    let mut test_sender_config = TestSenderConfig {
        topics: test_sender_topics,
        radio_name: String::new(),
        block_hash: None,
        nonce: None,
        poi: None,
        id_validation: None,
    };

    // Connection string for the SQLite database using the temporary file
    let connection_string = format!("sqlite:{}", db_path);

    let pool = SqlitePool::connect(&connection_string)
        .await
        .expect("Failed to connect to the in-memory database");
    sqlx::migrate!("../migrations")
        .run(&pool)
        .await
        .expect("Could not run migration");

    let process_manager = setup(&config, test_file_name, &mut test_sender_config).await;

    sleep(Duration::from_secs(100)).await;

    teardown(process_manager);

    let local_attestations = get_local_attestations(&pool).await.unwrap();
    assert!(
        !local_attestations.is_empty(),
        "There should be at least one element in local_attestations"
    );

    for test_hash in &radio_topics {
        let has_attestation_for_topic = local_attestations
            .iter()
            .any(|attestation| &attestation.identifier == test_hash);
        assert!(
            has_attestation_for_topic,
            "No attestation found for ipfs hash {}",
            test_hash
        );
    }

    let remote_ppoi_messages = get_remote_ppoi_messages(&pool).await.unwrap();
    assert!(
        remote_ppoi_messages.len() >= 5,
        "The number of remote messages should be at least 5. Actual: {}",
        remote_ppoi_messages.len()
    );
    assert_all_messages_are_public_poi(&remote_ppoi_messages);
}
