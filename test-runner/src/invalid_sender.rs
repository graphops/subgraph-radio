use graphcast_sdk::graphcast_agent::message_typing::IdentityValidation;

use sqlx::SqlitePool;
use subgraph_radio::database::get_remote_ppoi_messages;
use tempfile::NamedTempFile;
use test_utils::{
    config::{test_config, TestSenderConfig},
    setup, teardown,
};
use tokio::time::{sleep, Duration};

pub async fn invalid_sender_test() {
    let test_file_name = "invalid_sender";
    let temp_file =
        NamedTempFile::new().expect("Failed to create a temporary file for the database.");
    let db_path = temp_file.path().to_str().unwrap().to_string();

    let radio_topics = vec!["Qmdefault1AbcDEFghijKLmnoPQRstUVwxYzABCDEFghijklmnopq".to_string()];

    let test_sender_topics =
        vec!["Qmdefault1AbcDEFghijKLmnoPQRstUVwxYzABCDEFghijklmnopq".to_string()];

    let mut config = test_config();
    config.radio_setup.sqlite_file_path = Some(db_path.clone());
    config.radio_setup.topics = radio_topics.clone();
    config.radio_setup.id_validation = IdentityValidation::RegisteredIndexer;

    let mut test_sender_config = TestSenderConfig {
        topics: test_sender_topics,
        radio_name: String::new(),
        block_hash: None,
        staked_tokens: Some("1".to_string()),
        nonce: None,
        radio_payload: None,
        poi: None,
    };

    let connection_string = format!("sqlite:{}", db_path);
    let pool = SqlitePool::connect(&connection_string)
        .await
        .expect("Failed to connect to the in-memory database");
    sqlx::migrate!("../migrations")
        .run(&pool)
        .await
        .expect("Could not run migration");

    let process_manager = setup(&config, test_file_name, &mut test_sender_config).await;

    sleep(Duration::from_secs(89)).await;

    teardown(process_manager);

    let remote_ppoi_messages = get_remote_ppoi_messages(&pool).await.unwrap();
    assert!(
        remote_ppoi_messages.is_empty(),
        "Remote messages should be empty"
    );
}
