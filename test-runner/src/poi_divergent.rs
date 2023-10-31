use subgraph_radio::{operator::attestation::ComparisonResultType, state::PersistedState};
use test_utils::{
    config::{test_config, TestSenderConfig},
    setup, teardown,
};
use tokio::time::{sleep, Duration};
use tracing::debug;

pub async fn poi_divergent_test() {
    let test_file_name = "poi_divergent";
    let store_path = format!("./test-runner/state/{}.json", test_file_name);

    let radio_topics = vec!["Qmdefault1AbcDEFghijKLmnoPQRstUVwxYzABCDEFghijklmnopq".to_string()];

    let test_sender_topics =
        vec!["Qmdefault1AbcDEFghijKLmnoPQRstUVwxYzABCDEFghijklmnopq".to_string()];

    let mut config = test_config();
    config.radio_setup.persistence_file_path = Some(store_path.clone());
    config.radio_setup.topics = radio_topics.clone();

    let mut test_sender_config = TestSenderConfig {
        topics: test_sender_topics,
        radio_name: String::new(),
        block_hash: None,
        staked_tokens: None,
        nonce: None,
        radio_payload: None,
        poi: Some("0x8a937e93f72bf4396214fd519e3ded51a7f3b4316ada7b87d246b4626f7e9e8d".to_string()),
    };

    let process_manager = setup(&config, test_file_name, &mut test_sender_config).await;

    sleep(Duration::from_secs(550)).await;

    let persisted_state = PersistedState::load_cache(&store_path);
    debug!("persisted state {:?}", persisted_state);

    let comparison_results = persisted_state.comparison_results();

    assert!(
        !comparison_results.is_empty(),
        "The comparison results should not be empty"
    );

    let has_divergent_result = comparison_results.iter().any(|result| {
        result.1.deployment == "Qmdefault1AbcDEFghijKLmnoPQRstUVwxYzABCDEFghijklmnopq"
            && result.1.result_type == ComparisonResultType::Divergent
    });

    assert!(
        has_divergent_result,
        "No comparison result found with deployment 'Qmdefault1AbcDEFghijKLmnoPQRstUVwxYzABCDEFghijklmnopq' and result type 'Divergent'"
    );

    teardown(process_manager, &store_path);
}
