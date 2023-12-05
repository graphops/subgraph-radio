use subgraph_radio::{create_test_db, database::get_remote_ppoi_messages};
use tempfile::NamedTempFile;
use test_utils::{
    config::{test_config, TestSenderConfig},
    dummy_msg::DummyMsg,
    setup, teardown,
};
use tokio::time::{sleep, Duration};

pub async fn invalid_payload_test() {
    let test_file_name = "invalid_payload";

    let temp_file =
        NamedTempFile::new().expect("Failed to create a temporary file for the database.");
    let db_path = temp_file.path().to_str().unwrap().to_string();
    let radio_topics = vec!["Qmdefault1AbcDEFghijKLmnoPQRstUVwxYzABCDEFghijklmnopq".to_string()];

    let test_sender_topics =
        vec!["Qmdefault1AbcDEFghijKLmnoPQRstUVwxYzABCDEFghijklmnopq".to_string()];

    let mut config = test_config();
    config.radio_setup.sqlite_file_path = Some(db_path.clone());
    config.radio_setup.topics = radio_topics.clone();

    let dummy_radio_payload = DummyMsg::new("hello".to_string(), 42);

    let mut test_sender_config = TestSenderConfig {
        topics: test_sender_topics,
        radio_name: String::new(),
        block_hash: Some(
            "3071f4b00c2fc57c7f97d18e77b0e9c377cebae15959e6254b1a1d48a83b92e8".to_string(),
        ),
        staked_tokens: None,
        nonce: None,
        radio_payload: Some(DummyMsg::to_json(&dummy_radio_payload)),
        poi: None,
    };

    let connection_string = format!("sqlite:{}", db_path);
    let pool = create_test_db(Some(&connection_string)).await;

    let process_manager = setup(&config, test_file_name, &mut test_sender_config).await;

    sleep(Duration::from_secs(89)).await;

    teardown(process_manager);

    let remote_ppoi_messages = get_remote_ppoi_messages(&pool).await.unwrap();
    assert!(
        remote_ppoi_messages.is_empty(),
        "Remote messages should be empty"
    );
}
