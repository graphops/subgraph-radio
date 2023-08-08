use subgraph_radio::state::PersistedState;
use test_utils::{
    config::{test_config, TestSenderConfig},
    setup, teardown,
};
use tokio::time::{sleep, Duration};
use tracing::debug;

pub async fn invalid_block_hash_test() {
    let test_file_name = "invalid_block_hash";
    let store_path = format!("./test-runner/state/{}.json", test_file_name);

    let radio_topics = vec!["Qmdefault1AbcDEFghijKLmnoPQRstUVwxYzABCDEFghijklmnopq".to_string()];

    let test_sender_topics =
        vec!["Qmdefault1AbcDEFghijKLmnoPQRstUVwxYzABCDEFghijklmnopq".to_string()];

    let mut config = test_config();
    config.radio_infrastructure.persistence_file_path = Some(store_path.clone());
    config.radio_infrastructure.topics = radio_topics.clone();

    let mut test_sender_config = TestSenderConfig {
        topics: test_sender_topics,
        radio_name: String::new(),
        block_hash: Some(
            "3071f4b00c2fc57c7f97d18e77b0e9c377cebae15959e6254b1a1d48a83b92e8".to_string(),
        ),
        staked_tokens: None,
        nonce: None,
        radio_payload: None,
        poi: None,
    };

    let process_manager = setup(&config, test_file_name, &mut test_sender_config).await;

    sleep(Duration::from_secs(89)).await;

    let persisted_state = PersistedState::load_cache(&store_path);
    debug!("persisted state {:?}", persisted_state);

    teardown(process_manager, &store_path);

    let remote_ppoi_messages = persisted_state.remote_ppoi_messages();
    assert!(
        remote_ppoi_messages.is_empty(),
        "Remote messages should be empty"
    );
}
