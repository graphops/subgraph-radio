use subgraph_radio::{
    create_test_db,
    entities::{get_all_local_attestations, get_remote_ppoi_messages},
};
use tempfile::NamedTempFile;
use test_utils::{
    config::{test_config, TestSenderConfig},
    setup, teardown,
};
use tokio::time::{sleep, Duration};

pub async fn topics_test() {
    let test_file_name = "topics";

    // Create a new temporary file for the database
    let temp_file =
        NamedTempFile::new().expect("Failed to create a temporary file for the database.");
    let db_path = temp_file.path().to_str().unwrap().to_string();

    let radio_topics = vec![
        "Qm22default1AbcDEFghijKLmnoPQRstUVwxYzABCDEFghijklmnopq".to_string(),
        "Qm22default2XyZABCdefGHIjklMNOpqrstuvWXYZabcdefGHIJKLMN".to_string(),
        "Qm22onlyinradioXyZABCdeFgHIjklMNOpqrstuvWXYZabcdefGHIJKL".to_string(),
    ];

    let test_sender_topics = vec![
        "Qm22default1AbcDEFghijKLmnoPQRstUVwxYzABCDEFghijklmnopq".to_string(),
        "Qm22default2XyZABCdefGHIjklMNOpqrstuvWXYZabcdefGHIJKLMN".to_string(),
        "Qmo22nlyintestsenderXyZABCdeFgHIjklMNOpqrstuvWXYZabcdEFG".to_string(),
    ];

    let mut config = test_config();
    config.radio_setup.sqlite_file_path = Some(db_path.clone());
    config.radio_setup.topics = radio_topics.clone();
    config.radio_setup.topic_update_interval = 90;

    let mut test_sender_config = TestSenderConfig {
        topics: test_sender_topics,
        radio_name: String::new(),
        block_hash: None,
        staked_tokens: None,
        nonce: None,
        radio_payload: None,
        poi: None,
    };

    // Connection string for the SQLite database using the temporary file
    let connection_string = format!("sqlite:{}", db_path);
    let pool = create_test_db(Some(&connection_string)).await;

    let process_manager = setup(&config, test_file_name, &mut test_sender_config).await;

    sleep(Duration::from_secs(100)).await;

    // Retrieve local attestations and verify them against radio topics
    let local_attestations = get_all_local_attestations(&pool).await.unwrap();
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

    // Retrieve remote PPOI messages and verify expected presence and count
    let remote_ppoi_messages = get_remote_ppoi_messages(&pool).await.unwrap();

    // Ensure we have received some remote PPOI messages
    assert!(
        !remote_ppoi_messages.is_empty(),
        "There should be at least one remote PPOI message"
    );

    // Assert that the remote PPOI messages contain expected identifiers
    let expected_remote_identifiers = vec![
        "Qm22default1AbcDEFghijKLmnoPQRstUVwxYzABCDEFghijklmnopq",
        "Qm22default2XyZABCdefGHIjklMNOpqrstuvWXYZabcdefGHIJKLMN",
        // Add more expected identifiers if necessary
    ];

    for expected_id in &expected_remote_identifiers {
        assert!(
            remote_ppoi_messages
                .iter()
                .any(|msg| &msg.identifier == expected_id),
            "Expected identifier {} not found in remote PPOI messages",
            expected_id
        );
    }

    // Assert that we do not have remote PPOI messages with identifiers that should not be present
    let unexpected_identifier = "Qm22onlyintestsenderXyZABCdeFgHIjklMNOpqrstuvWXYZabcdEFG";
    assert!(
        !remote_ppoi_messages
            .iter()
            .any(|msg| msg.identifier == unexpected_identifier),
        "Unexpected identifier {} found in remote PPOI messages",
        unexpected_identifier
    );

    // Cleanup after test
    teardown(process_manager);
}
