use graphcast_sdk::graphcast_agent::message_typing::IdentityValidation;
use sqlx::SqlitePool;
use subgraph_radio::database::get_remote_ppoi_messages;
use tempfile::NamedTempFile;
use test_utils::{
    config::{test_config, TestSenderConfig},
    setup, teardown,
};
use tokio::time::{sleep, Duration};
use tracing::info;

pub async fn id_validation_test_stricter_radio_level() {
    let test_file_name = "id_validation_test_stricter_radio_level";

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
    config.radio_setup.id_validation = IdentityValidation::Indexer;

    let mut test_sender_config = TestSenderConfig {
        topics: test_sender_topics,
        radio_name: String::new(),
        block_hash: None,
        nonce: None,
        poi: None,
        id_validation: Some(IdentityValidation::ValidAddress),
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

    let remote_ppoi_messages = get_remote_ppoi_messages(&pool).await.unwrap();
    assert!(
        remote_ppoi_messages.is_empty(),
        "The number of remote messages should be 0. Actual: {}",
        remote_ppoi_messages.len()
    );
}

pub async fn id_validation_test_looser_radio_level() {
    let test_file_name = "id_validation_test_looser_radio_level";

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
    config.radio_setup.id_validation = IdentityValidation::ValidAddress;

    let mut test_sender_config = TestSenderConfig {
        topics: test_sender_topics,
        radio_name: String::new(),
        block_hash: None,
        nonce: None,
        poi: None,
        id_validation: Some(IdentityValidation::GraphcastRegistered),
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

    let remote_ppoi_messages = get_remote_ppoi_messages(&pool).await.unwrap();
    assert!(
        remote_ppoi_messages.len() >= 5,
        "The number of remote messages should be at least 5. Actual: {}",
        remote_ppoi_messages.len()
    );
}

pub async fn id_validation_test_matching_radio_level() {
    let test_file_name = "id_validation_test_matching_radio_level";

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
    config.radio_setup.id_validation = IdentityValidation::GraphNetworkAccount;

    let mut test_sender_config = TestSenderConfig {
        topics: test_sender_topics,
        radio_name: String::new(),
        block_hash: None,
        nonce: None,
        poi: None,
        id_validation: Some(IdentityValidation::GraphNetworkAccount),
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

    let remote_ppoi_messages = get_remote_ppoi_messages(&pool).await.unwrap();
    assert!(
        remote_ppoi_messages.len() >= 5,
        "The number of remote messages should be at least 5. Actual: {}",
        remote_ppoi_messages.len()
    );
}

pub async fn id_validation_test_invalid_address_loose() {
    let test_file_name = "id_validation_test_invalid_address_loose";

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
    config.radio_setup.id_validation = IdentityValidation::NoCheck;

    let mut test_sender_config = TestSenderConfig {
        topics: test_sender_topics,
        radio_name: String::new(),
        block_hash: None,
        nonce: None,
        poi: None,
        id_validation: Some(IdentityValidation::NoCheck),
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

    let remote_ppoi_messages = get_remote_ppoi_messages(&pool).await.unwrap();
    info!("{:?}", remote_ppoi_messages);

    assert!(
        remote_ppoi_messages.len() >= 5,
        "The number of remote messages should be at least 5. Actual: {}",
        remote_ppoi_messages.len()
    );
}
