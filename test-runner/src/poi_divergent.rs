use sqlx::SqlitePool;
use subgraph_radio::{
    database::get_comparison_results, operator::attestation::ComparisonResultType,
};
use tempfile::NamedTempFile;
use test_utils::{
    config::{test_config, TestSenderConfig},
    setup, teardown,
};
use tokio::time::{sleep, Duration};

pub async fn poi_divergent_test() {
    let test_file_name = "poi_divergent";

    // Create a new temporary file for the database
    let temp_file =
        NamedTempFile::new().expect("Failed to create a temporary file for the database.");
    let db_path = temp_file.path().to_str().unwrap().to_string();

    let radio_topics = vec!["Qmdefault1AbcDEFghijKLmnoPQRstUVwxYzABCDEFghijklmnopq".to_string()];

    let test_sender_topics =
        vec!["Qmdefault1AbcDEFghijKLmnoPQRstUVwxYzABCDEFghijklmnopq".to_string()];

    let mut config = test_config();
    config.radio_setup.sqlite_file_path = Some(db_path.clone());
    config.radio_setup.topics = radio_topics.clone();

    let mut test_sender_config = TestSenderConfig {
        topics: test_sender_topics,
        radio_name: String::new(),
        block_hash: None,
        nonce: None,
        poi: Some("0x8a937e93f72bf4396214fd519e3ded51a7f3b4316ada7b87d246b4626f7e9e8d".to_string()),
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

    sleep(Duration::from_secs(550)).await;

    let comparison_results = get_comparison_results(&pool).await.unwrap();
    assert!(
        !comparison_results.is_empty(),
        "The comparison results should not be empty"
    );

    let has_divergent_result = comparison_results.iter().any(|result| {
        result.deployment == "Qmdefault1AbcDEFghijKLmnoPQRstUVwxYzABCDEFghijklmnopq"
            && result.result_type == ComparisonResultType::Divergent
    });

    assert!(
        has_divergent_result,
        "No comparison result found with deployment 'Qmdefault1AbcDEFghijKLmnoPQRstUVwxYzABCDEFghijklmnopq' and result type 'Divergent'"
    );

    teardown(process_manager);
}
