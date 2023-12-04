use subgraph_radio::{
    create_test_db, entities::get_comparison_results, operator::attestation::ComparisonResultType,
};
use tempfile::NamedTempFile;
use test_utils::{
    config::{test_config, TestSenderConfig},
    setup, teardown,
};
use tokio::time::{sleep, Duration};

pub async fn poi_match_test() {
    let test_file_name = "poi_match";

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
        staked_tokens: None,
        nonce: None,
        radio_payload: None,
        poi: None,
    };

    // Connection string for the SQLite database using the temporary file
    let connection_string = format!("sqlite:{}", db_path);
    let pool = create_test_db(Some(&connection_string)).await;

    let process_manager = setup(&config, test_file_name, &mut test_sender_config).await;

    sleep(Duration::from_secs(550)).await;

    let comparison_results = get_comparison_results(&pool).await.unwrap();

    assert!(
        !comparison_results.is_empty(),
        "The comparison results should not be empty"
    );

    let has_match_result = comparison_results.iter().any(|result| {
        result.deployment == "Qmdefault1AbcDEFghijKLmnoPQRstUVwxYzABCDEFghijklmnopq"
            && result.result_type == ComparisonResultType::Match
    });

    assert!(
        has_match_result,
        "No comparison result found with deployment 'Qmdefault1AbcDEFghijKLmnoPQRstUVwxYzABCDEFghijklmnopq' and result type 'Match'"
    );

    teardown(process_manager);
}
