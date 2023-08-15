use graphcast_sdk::graphcast_agent::message_typing::IdentityValidation;
use subgraph_radio::state::PersistedState;
use test_utils::{
    config::{test_config, TestSenderConfig},
    setup, teardown,
};
use tokio::time::{sleep, Duration};
use tracing::debug;

pub async fn id_validation_registered_indexer_invalid_sender() {
    let test_file_name = "id_validation_registered_indexer_invalid_sender";
    let store_path = format!("./test-runner/state/{}.json", test_file_name);

    let radio_topics = vec!["Qmdefault1AbcDEFghijKLmnoPQRstUVwxYzABCDEFghijklmnopq".to_string()];

    let test_sender_topics =
        vec!["Qmdefault1AbcDEFghijKLmnoPQRstUVwxYzABCDEFghijklmnopq".to_string()];

    let mut config = test_config();
    config.radio_infrastructure.persistence_file_path = Some(store_path.clone());
    config.radio_infrastructure.topics = radio_topics.clone();
    config.radio_infrastructure.id_validation = IdentityValidation::RegisteredIndexer;

    let mut test_sender_config = TestSenderConfig {
        topics: test_sender_topics,
        radio_name: String::new(),
        block_hash: None,
        staked_tokens: None,
        nonce: None,
        radio_payload: None,
        poi: None,
        private_key: Some(
            "c649c5113a305a04cd9b342beccbbc4cf57198d84d2ef07cea2ce9b887c51f06".to_string(),
        ),
    };

    let process_manager = setup(&config, test_file_name, &mut test_sender_config).await;

    sleep(Duration::from_secs(85)).await;

    let persisted_state = PersistedState::load_cache(&store_path);
    debug!("persisted state {:?}", persisted_state);

    let remote_ppoi_messages = persisted_state.remote_ppoi_messages();
    assert!(
        remote_ppoi_messages.is_empty(),
        "Remote messages should be empty"
    );

    // We do the teardown after we've run all the tests.
    teardown(process_manager, &store_path);
}

pub async fn id_validation_registered_indexer_valid_sender() {
    let test_file_name = "id_validation_registered_indexer_valid_sender";
    let store_path = format!("./test-runner/state/{}.json", test_file_name);

    let radio_topics = vec!["Qmdefault1AbcDEFghijKLmnoPQRstUVwxYzABCDEFghijklmnopq".to_string()];

    let test_sender_topics =
        vec!["Qmdefault1AbcDEFghijKLmnoPQRstUVwxYzABCDEFghijklmnopq".to_string()];

    let mut config = test_config();
    config.radio_infrastructure.persistence_file_path = Some(store_path.clone());
    config.radio_infrastructure.topics = radio_topics.clone();
    config.radio_infrastructure.id_validation = IdentityValidation::RegisteredIndexer;

    let mut test_sender_config = TestSenderConfig {
        topics: test_sender_topics,
        radio_name: String::new(),
        block_hash: None,
        staked_tokens: None,
        nonce: None,
        radio_payload: None,
        poi: None,
        private_key: Some(
            "baf5c93f0c8aee3b945f33b9192014e83d50cec25f727a13460f6ef1eb6a5844".to_string(),
        ),
    };

    let process_manager = setup(&config, test_file_name, &mut test_sender_config).await;

    sleep(Duration::from_secs(89)).await;

    let persisted_state = PersistedState::load_cache(&store_path);
    debug!("persisted state {:?}", persisted_state);

    let remote_ppoi_messages = persisted_state.remote_ppoi_messages();
    assert!(
        !remote_ppoi_messages.is_empty(),
        "There should be at least 1 remote message"
    );

    // We do the teardown after we've run all the tests.
    teardown(process_manager, &store_path);
}

pub async fn id_validation_indexer_invalid_sender() {
    let test_file_name = "id_validation_indexer_invalid_sender";
    let store_path = format!("./test-runner/state/{}.json", test_file_name);

    let radio_topics = vec!["Qmdefault1AbcDEFghijKLmnoPQRstUVwxYzABCDEFghijklmnopq".to_string()];

    let test_sender_topics =
        vec!["Qmdefault1AbcDEFghijKLmnoPQRstUVwxYzABCDEFghijklmnopq".to_string()];

    let mut config = test_config();
    config.radio_infrastructure.persistence_file_path = Some(store_path.clone());
    config.radio_infrastructure.topics = radio_topics.clone();
    config.radio_infrastructure.id_validation = IdentityValidation::Indexer;

    let mut test_sender_config = TestSenderConfig {
        topics: test_sender_topics,
        radio_name: String::new(),
        block_hash: None,
        staked_tokens: None,
        nonce: None,
        radio_payload: None,
        poi: None,
        private_key: Some(
            "f1f5c93f0c8aee3b945f33b9192014e83d50cec25f727a13460f6ef1eb6a5844".to_string(),
        ),
    };

    let process_manager = setup(&config, test_file_name, &mut test_sender_config).await;

    sleep(Duration::from_secs(89)).await;

    let persisted_state = PersistedState::load_cache(&store_path);
    debug!("persisted state {:?}", persisted_state);

    let remote_ppoi_messages = persisted_state.remote_ppoi_messages();
    assert!(
        remote_ppoi_messages.is_empty(),
        "Remote messages should be empty"
    );

    // We do the teardown after we've run all the tests.
    teardown(process_manager, &store_path);
}

pub async fn id_validation_indexer_valid_sender() {
    let test_file_name = "id_validation_indexer_valid_sender";
    let store_path = format!("./test-runner/state/{}.json", test_file_name);

    let radio_topics = vec!["Qmdefault1AbcDEFghijKLmnoPQRstUVwxYzABCDEFghijklmnopq".to_string()];

    let test_sender_topics =
        vec!["Qmdefault1AbcDEFghijKLmnoPQRstUVwxYzABCDEFghijklmnopq".to_string()];

    let mut config = test_config();
    config.radio_infrastructure.persistence_file_path = Some(store_path.clone());
    config.radio_infrastructure.topics = radio_topics.clone();
    config.radio_infrastructure.id_validation = IdentityValidation::Indexer;

    let mut test_sender_config = TestSenderConfig {
        topics: test_sender_topics,
        radio_name: String::new(),
        block_hash: None,
        staked_tokens: None,
        nonce: None,
        radio_payload: None,
        poi: None,
        private_key: Some(
            "c649c5113a305a04cd9b342beccbbc4cf57198d84d2ef07cea2ce9b887c51f06".to_string(),
        ),
    };

    let process_manager = setup(&config, test_file_name, &mut test_sender_config).await;

    sleep(Duration::from_secs(89)).await;

    let persisted_state = PersistedState::load_cache(&store_path);
    debug!("persisted state {:?}", persisted_state);

    let remote_ppoi_messages = persisted_state.remote_ppoi_messages();
    assert!(
        !remote_ppoi_messages.is_empty(),
        "There should be at least 1 remote message"
    );

    // We do the teardown after we've run all the tests.
    teardown(process_manager, &store_path);
}

pub async fn id_validation_valid_address_valid_sender() {
    let test_file_name = "id_validation_valid_address_valid_sender";
    let store_path = format!("./test-runner/state/{}.json", test_file_name);

    let radio_topics = vec!["Qmdefault1AbcDEFghijKLmnoPQRstUVwxYzABCDEFghijklmnopq".to_string()];

    let test_sender_topics =
        vec!["Qmdefault1AbcDEFghijKLmnoPQRstUVwxYzABCDEFghijklmnopq".to_string()];

    let mut config = test_config();
    config.radio_infrastructure.persistence_file_path = Some(store_path.clone());
    config.radio_infrastructure.topics = radio_topics.clone();
    config.radio_infrastructure.id_validation = IdentityValidation::ValidAddress;

    let mut test_sender_config = TestSenderConfig {
        topics: test_sender_topics,
        radio_name: String::new(),
        block_hash: None,
        staked_tokens: None,
        nonce: None,
        radio_payload: None,
        poi: None,
        private_key: Some(
            "90f12133511460b30a4a0e907008a8fb45c035fb7b3b65a77f92e56344d59063".to_string(),
        ),
    };

    let process_manager = setup(&config, test_file_name, &mut test_sender_config).await;

    sleep(Duration::from_secs(89)).await;

    let persisted_state = PersistedState::load_cache(&store_path);
    debug!("persisted state {:?}", persisted_state);

    let remote_ppoi_messages = persisted_state.remote_ppoi_messages();
    assert!(
        !remote_ppoi_messages.is_empty(),
        "There should be at least 1 remote message"
    );

    // We do the teardown after we've run all the tests.
    teardown(process_manager, &store_path);
}
