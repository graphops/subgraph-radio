use serde::{Deserialize, Serialize};

use std::panic::PanicInfo;
use std::path::Path;

use std::str::FromStr;
use std::sync::{Arc, Mutex as SyncMutex};
use std::{
    collections::HashMap,
    fs::{remove_file, File},
    io::{BufReader, Write},
};
use std::{fs, panic};
use tracing::{debug, info, trace, warn};

use graphcast_sdk::graphcast_agent::message_typing::GraphcastMessage;

use crate::messages::upgrade::UpgradeIntentMessage;
use crate::metrics::CACHED_PPOI_MESSAGES;
use crate::operator::notifier::NotificationMode;
use crate::{
    messages::poi::PublicPoiMessage,
    operator::attestation::{
        clear_local_attestation, Attestation, ComparisonResult, ComparisonResultType,
    },
    operator::notifier::Notifier,
    RADIO_OPERATOR,
};

type Local = Arc<SyncMutex<HashMap<String, HashMap<u64, Attestation>>>>;
type Remote = Arc<SyncMutex<Vec<GraphcastMessage<PublicPoiMessage>>>>;
type UpgradeMessages = Arc<SyncMutex<HashMap<String, GraphcastMessage<UpgradeIntentMessage>>>>;
type ComparisonResults = Arc<SyncMutex<HashMap<String, ComparisonResult>>>;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct PersistedState {
    pub local_attestations: Local,
    pub remote_ppoi_messages: Remote,
    pub upgrade_intent_messages: UpgradeMessages,
    pub comparison_results: ComparisonResults,
}

impl PersistedState {
    pub fn new(
        local: Option<Local>,
        remote: Option<Remote>,
        upgrade_intent_messages: Option<UpgradeMessages>,
        comparison_results: Option<ComparisonResults>,
    ) -> PersistedState {
        let local_attestations = local.unwrap_or(Arc::new(SyncMutex::new(HashMap::new())));
        let remote_ppoi_messages = remote.unwrap_or(Arc::new(SyncMutex::new(vec![])));
        let upgrade_intent_messages =
            upgrade_intent_messages.unwrap_or(Arc::new(SyncMutex::new(HashMap::new())));
        let comparison_results =
            comparison_results.unwrap_or(Arc::new(SyncMutex::new(HashMap::new())));

        PersistedState {
            local_attestations,
            remote_ppoi_messages,
            upgrade_intent_messages,
            comparison_results,
        }
    }

    /// Optional updates for either local_attestations, remote_ppoi_messages or comparison_results without requiring either to be in-scope
    pub async fn update(
        &mut self,
        local_attestations: Option<Local>,
        remote_ppoi_messages: Option<Remote>,
        upgrade_intent_messages: Option<UpgradeMessages>,
        comparison_results: Option<ComparisonResults>,
    ) -> PersistedState {
        let local_attestations = match local_attestations {
            None => self.local_attestations.clone(),
            Some(l) => l,
        };
        let remote_ppoi_messages = match remote_ppoi_messages {
            None => self.remote_ppoi_messages.clone(),
            Some(r) => r,
        };
        let upgrade_intent_messages = match upgrade_intent_messages {
            None => self.upgrade_intent_messages.clone(),
            Some(r) => r,
        };
        let comparison_results = match comparison_results {
            None => self.comparison_results.clone(),
            Some(r) => r,
        };
        PersistedState {
            local_attestations,
            remote_ppoi_messages,
            upgrade_intent_messages,
            comparison_results,
        }
    }

    /// Getter for local_attestations
    pub fn local_attestations(&self) -> HashMap<String, HashMap<u64, Attestation>> {
        self.local_attestations.lock().unwrap().clone()
    }

    /// Getter for one local_attestation
    pub fn local_attestation(&self, deployment: String, block_number: u64) -> Option<Attestation> {
        match self.local_attestations.lock().unwrap().get(&deployment) {
            None => None,
            Some(blocks_map) => blocks_map.get(&block_number).cloned(),
        }
    }

    /// Getter for Public POI messages
    pub fn remote_ppoi_messages(&self) -> Vec<GraphcastMessage<PublicPoiMessage>> {
        self.remote_ppoi_messages.lock().unwrap().clone()
    }

    /// Getter for upgrade intent messages
    pub fn upgrade_intent_messages(
        &self,
    ) -> HashMap<String, GraphcastMessage<UpgradeIntentMessage>> {
        self.upgrade_intent_messages.lock().unwrap().clone()
    }

    /// Getter for upgrade intent messages for subgraph
    pub fn upgrade_intent_message(
        &self,
        subgraph_id: &str,
    ) -> Option<GraphcastMessage<UpgradeIntentMessage>> {
        self.upgrade_intent_messages
            .lock()
            .unwrap()
            .get(subgraph_id)
            .cloned()
    }

    /// Getter for comparison_results
    pub fn comparison_results(&self) -> HashMap<String, ComparisonResult> {
        self.comparison_results.lock().unwrap().clone()
    }

    /// Getter for comparison result
    pub fn comparison_result(&self, deployment: String) -> Option<ComparisonResult> {
        self.comparison_results
            .lock()
            .unwrap()
            .get(&deployment)
            .cloned()
    }

    /// Getter for comparison results with a certain result type
    pub fn comparison_result_typed(
        &self,
        result_type: ComparisonResultType,
    ) -> Vec<ComparisonResult> {
        let mut matched_type = vec![];
        for (_key, value) in self.comparison_results() {
            if value.result_type == result_type {
                matched_type.push(value.clone());
            }
        }
        matched_type
    }

    /// Update local_attestations
    pub async fn update_local(&mut self, local_attestations: Local) {
        self.local_attestations = local_attestations;
    }

    /// Update remote_ppoi_messages
    pub async fn update_remote(
        &mut self,
        remote_ppoi_messages: Vec<GraphcastMessage<PublicPoiMessage>>,
    ) -> Vec<GraphcastMessage<PublicPoiMessage>> {
        self.remote_ppoi_messages = Arc::new(SyncMutex::new(remote_ppoi_messages));
        self.remote_ppoi_messages()
    }

    /// Add message to remote_ppoi_messages
    /// Generalize PublicPoiMessage
    pub fn add_remote_ppoi_message(&self, msg: GraphcastMessage<PublicPoiMessage>) {
        trace!(
            msg = tracing::field::debug(&msg),
            "Adding remote ppoi message"
        );
        self.remote_ppoi_messages.lock().unwrap().push(msg)
    }

    /// Add message to remote_ppoi_messages
    pub fn add_upgrade_intent_message(&self, msg: GraphcastMessage<UpgradeIntentMessage>) {
        let key = msg.payload.subgraph_id.clone();
        if let Some(_existing) = self.upgrade_intent_message(&key) {
            // replace the existing "outdated" record of ratelimit
            debug!(
                msg = tracing::field::debug(&msg),
                "Replace the outdated upgrade message with new message"
            );
            let mut msgs = self.upgrade_intent_messages.lock().unwrap();
            msgs.insert(key, msg);
        } else {
            trace!(
                msg = tracing::field::debug(&msg),
                "Adding upgrade intent message"
            );
            self.upgrade_intent_messages
                .lock()
                .unwrap()
                .entry(key.clone())
                .or_insert(msg);
        }
    }

    /// Check if there is a recent upgrade message for the subgraph
    pub fn recent_upgrade(&self, msg: &UpgradeIntentMessage, upgrade_threshold: i64) -> bool {
        self.upgrade_intent_messages()
            .iter()
            .any(|(matching_id, existing)| {
                // there is a upgrade msg of the same subgraph id within the upgrade threshold
                matching_id == &msg.subgraph_id && existing.nonce > msg.nonce - upgrade_threshold
            })
    }

    /// Add entry to comparison_results
    pub fn add_comparison_result(&self, comparison_result: ComparisonResult) {
        let deployment = comparison_result.clone().deployment;

        self.comparison_results
            .lock()
            .unwrap()
            .insert(deployment, comparison_result);
    }

    pub async fn valid_ppoi_messages(
        &mut self,
        graph_node_endpoint: &str,
    ) -> Vec<GraphcastMessage<PublicPoiMessage>> {
        let remote_ppoi_messages = self.remote_ppoi_messages();
        let mut valid_messages = vec![];

        for message in remote_ppoi_messages {
            let is_valid = message
                .payload
                .validity_check(&message, graph_node_endpoint)
                .await;

            if is_valid.is_ok() {
                valid_messages.push(message);
            }
        }

        self.update_remote(valid_messages).await
    }

    pub async fn handle_comparison_result(
        &self,
        new_comparison_result: ComparisonResult,
        notifier: Notifier,
        notification_mode: NotificationMode,
    ) -> ComparisonResultType {
        let (should_notify, updated_comparison_result, result_type) = {
            let mut results = self.comparison_results.lock().unwrap();
            let deployment = &new_comparison_result.deployment;

            // Only notify users if there is a switch between match<->diverged, and if notFound become diverged
            let mut should_notify = false;
            // let current_result = results.get(deployment).cloned();
            let result_type = match results.get(deployment).cloned() {
                // If there's no existing result, simply update and return new result
                None => {
                    results.insert(deployment.clone(), new_comparison_result.clone());
                    new_comparison_result.result_type
                }

                // If previous type and current type switch
                // update and return result type
                Some(current_result)
                    if current_result.result_type != new_comparison_result.result_type
                        && new_comparison_result.result_type != ComparisonResultType::NotFound =>
                {
                    results.insert(deployment.clone(), new_comparison_result.clone());
                    // Skip notification if notFound becomes match, otherwise notify for
                    // diverged<->match, notFound->diverged
                    if new_comparison_result.result_type != ComparisonResultType::Match {
                        should_notify = true;
                    }
                    new_comparison_result.result_type
                }
                // New result is not found or same as previous result
                Some(current_result) => {
                    // Do not update result if the type is divergence so we keep track of the first diverged block
                    if let ComparisonResultType::Match | ComparisonResultType::NotFound =
                        new_comparison_result.result_type
                    {
                        results.insert(deployment.clone(), new_comparison_result.clone());
                    }
                    current_result.result_type
                }
            };

            (should_notify, new_comparison_result.clone(), result_type)
        };

        if notification_mode == NotificationMode::Live && should_notify {
            notifier.notify(updated_comparison_result.to_string()).await;
        }

        result_type
    }

    /// Clean remote_ppoi_messages
    pub fn clean_remote_ppoi_messages(&self, block_number: u64, deployment: String) {
        trace!(
            msgs = tracing::field::debug(&self.remote_ppoi_messages.lock().unwrap()),
            "cleaning these messages"
        );
        self.remote_ppoi_messages
            .lock()
            .unwrap()
            .retain(|msg| msg.payload.block_number >= block_number || msg.identifier != deployment);

        CACHED_PPOI_MESSAGES.with_label_values(&[&deployment]).set(
            self.remote_ppoi_messages
                .lock()
                .unwrap()
                .iter()
                .filter(|m: &&GraphcastMessage<PublicPoiMessage>| m.identifier == deployment)
                .collect::<Vec<&GraphcastMessage<PublicPoiMessage>>>()
                .len()
                .try_into()
                .unwrap(),
        );
    }

    /// Clean local_attestations
    // TODO: Refactor with attestations operations
    pub fn clean_local_attestations(&self, block_number: u64, ipfs_hash: String) {
        clear_local_attestation(self.local_attestations.clone(), ipfs_hash, block_number)
    }

    /// Update file cache
    pub fn update_cache(&self, path: &str) {
        // Attempt to serialize state to JSON
        let state_json = serde_json::to_string(&self.clone())
            .unwrap_or_else(|_| "Could not serialize state to JSON".to_owned());

        let path = Path::new(path);

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        // Write state to file
        let mut file = File::create(path).unwrap();
        file.write_all(state_json.as_bytes()).unwrap();
    }

    /// Load cache into persisted state
    pub fn load_cache(path: &str) -> PersistedState {
        info!(path, "load cache from path");
        let file = match File::open(path) {
            Ok(f) => f,
            Err(e) => {
                warn!(
                    err = tracing::field::debug(&e),
                    "No persisted state file provided, create an empty state"
                );
                // No state persisted, create new
                let state = PersistedState::new(None, None, None, None);
                state.update_cache(path);
                return state;
            }
        };

        let reader: BufReader<File> = BufReader::new(file);
        let state: PersistedState = match serde_json::from_reader(reader) {
            Ok(s) => s,
            Err(e) => {
                // Persisted state can't be parsed, create a new one
                warn!(
                    err = e.to_string(),
                    "Could not parse persisted state file, created an empty state",
                );
                PersistedState::new(None, None, None, None)
            }
        };
        state
    }

    /// Clean up
    pub fn delete_cache(path: &str) {
        _ = remove_file(path);
    }
}

// TODO: panic hook for updating the cache file before exiting the program
/// Set up panic hook to store persisted state
pub fn panic_hook(file_path: &str) {
    let path = String::from_str(file_path).expect("Invalid file path provided");
    panic::set_hook(Box::new(move |panic_info| panic_cache(panic_info, &path)));
}

pub fn panic_cache(panic_info: &PanicInfo<'_>, file_path: &str) {
    RADIO_OPERATOR
        .get()
        .unwrap()
        .state()
        .update_cache(file_path);
    // Log panic information and program state
    eprintln!("Panic occurred! Panic info: {:?}", panic_info);
}

#[cfg(test)]
mod tests {
    use super::*;
    use graphcast_sdk::networks::NetworkName;

    use crate::operator::attestation::{save_local_attestation, ComparisonResultType};

    /// Tests for load, update, and store cache
    #[tokio::test]
    async fn test_state_cache() {
        let path = "test-state.json";
        PersistedState::delete_cache(path);

        let mut state = PersistedState::load_cache(path);
        assert!(state.local_attestations().is_empty());
        assert!(state.remote_ppoi_messages().is_empty());
        assert!(state.comparison_results().is_empty());

        let local_attestations = Arc::new(SyncMutex::new(HashMap::new()));
        let ppoi_messages = Arc::new(SyncMutex::new(Vec::new()));
        let comparison_results = Arc::new(SyncMutex::new(HashMap::new()));

        save_local_attestation(
            local_attestations.clone(),
            "ppoi-x".to_string(),
            "0xa1".to_string(),
            0,
        );

        save_local_attestation(
            local_attestations.clone(),
            "ppoi-y".to_string(),
            "0xa1".to_string(),
            1,
        );

        save_local_attestation(
            local_attestations.clone(),
            "ppoi-z".to_string(),
            "0xa2".to_string(),
            2,
        );

        let test_comparison_result = ComparisonResult {
            deployment: "test_deployment".to_string(),
            block_number: 42,
            result_type: ComparisonResultType::Match,
            local_attestation: None,
            attestations: vec![],
        };
        comparison_results
            .lock()
            .unwrap()
            .insert("test_deployment".to_string(), test_comparison_result);

        let hash: String = "QmWECgZdP2YMcV9RtKU41GxcdW8EGYqMNoG98ubu5RGN6U".to_string();
        let content: String =
            "0xa6008cea5905b8b7811a68132feea7959b623188e2d6ee3c87ead7ae56dd0eae".to_string();
        let nonce: i64 = 123321;
        let block_number: u64 = 0;
        let block_hash: String = "0xblahh".to_string();
        let ppoi_msg = PublicPoiMessage::build(
            hash.clone(),
            content,
            nonce,
            NetworkName::Goerli,
            block_number,
            block_hash,
            String::from("0xe9a1cabd57700b17945fd81feefba82340d9568f"),
        );
        let sig: String = "4be6a6b7f27c4086f22e8be364cbdaeddc19c1992a42b08cbe506196b0aafb0a68c8c48a730b0e3155f4388d7cc84a24b193d091c4a6a4e8cd6f1b305870fae61b".to_string();
        let msg = GraphcastMessage::new(
            hash.clone(),
            nonce,
            String::from("0xe9a1cabd57700b17945fd81feefba82340d9568f"),
            ppoi_msg,
            sig,
        )
        .expect("Shouldn't get here since the message is purposefully constructed for testing");
        ppoi_messages.lock().unwrap().push(msg);

        state = state
            .update(
                Some(local_attestations.clone()),
                Some(ppoi_messages.clone()),
                None,
                Some(comparison_results.clone()),
            )
            .await;

        let ui_msg = UpgradeIntentMessage {
            subgraph_id: String::from("CnJMdCkW3pr619gsJVtUPAWxspALPdCMw6o7obzYBNp3"),
            new_hash: String::from("QmacQnSgia4iDPWHpeY6aWxesRFdb8o5DKZUx96zZqEWAA"),
            nonce,
            graph_account: String::from("0xe9a1cabd57700b17945fd81feefba82340d9568f"),
        };
        let sig: String = "4be6a6b7f27c4086f22e8be364cbdaeddc19c1992a42b08cbe506196b0aafb0a68c8c48a730b0e3155f4388d7cc84a24b193d091c4a6a4e8cd6f1b305870fae61b".to_string();
        let msg = GraphcastMessage::new(
            "QmacQnSgia4iDPWHpeY6aWxesRFdb8o5DKZUx96zZqEWrB".to_string(),
            nonce,
            String::from("0xe9a1cabd57700b17945fd81feefba82340d9568f"),
            ui_msg,
            sig,
        )
        .expect("Shouldn't get here since the message is purposefully constructed for testing");
        state.add_upgrade_intent_message(msg);
        state.update_cache(path);

        let state = PersistedState::load_cache(path);
        assert_eq!(state.remote_ppoi_messages.lock().unwrap().len(), 1);
        assert_eq!(state.upgrade_intent_messages.lock().unwrap().len(), 1);
        assert!(!state.local_attestations.lock().unwrap().is_empty());
        assert!(state.local_attestations.lock().unwrap().len() == 2);
        assert!(
            state
                .local_attestations
                .lock()
                .unwrap()
                .get("0xa1")
                .unwrap()
                .len()
                == 2
        );
        assert!(
            state
                .local_attestations
                .lock()
                .unwrap()
                .get("0xa2")
                .unwrap()
                .len()
                == 1
        );
        assert!(
            state
                .local_attestations
                .lock()
                .unwrap()
                .get("0xa1")
                .unwrap()
                .get(&0)
                .unwrap()
                .ppoi
                == *"ppoi-x"
        );

        assert_eq!(state.comparison_results.lock().unwrap().len(), 1);
        assert_eq!(
            state
                .comparison_results
                .lock()
                .unwrap()
                .get("test_deployment")
                .unwrap()
                .block_number,
            42
        );
        assert_eq!(
            state
                .comparison_results
                .lock()
                .unwrap()
                .get("test_deployment")
                .unwrap()
                .result_type,
            ComparisonResultType::Match
        );

        PersistedState::delete_cache(path);
    }

    #[tokio::test]
    async fn handle_comparison_result_new_deployment() {
        let notifier = Notifier::new("not-a-real-radio".to_string(), None, None, None, None, None);
        let local_attestations = Arc::new(SyncMutex::new(HashMap::new()));
        let remote_ppoi_messages = Arc::new(SyncMutex::new(Vec::new()));
        let upgrade_intent_messages = Arc::new(SyncMutex::new(HashMap::new()));
        let comparison_results = Arc::new(SyncMutex::new(HashMap::new()));
        let state = PersistedState {
            local_attestations,
            remote_ppoi_messages,
            upgrade_intent_messages,
            comparison_results,
        };

        let new_result = ComparisonResult {
            deployment: String::from("new_deployment"),
            block_number: 1,
            result_type: ComparisonResultType::Match,
            local_attestation: None,
            attestations: Vec::new(),
        };

        state
            .handle_comparison_result(new_result, notifier, NotificationMode::Live)
            .await;

        let comparison_results = state.comparison_results.lock().unwrap();
        assert!(comparison_results.contains_key(&String::from("new_deployment")));
    }

    #[tokio::test]
    async fn handle_comparison_result_change_result_type() {
        let notifier = Notifier::new("not-a-real-radio".to_string(), None, None, None, None, None);
        let local_attestations = Arc::new(SyncMutex::new(HashMap::new()));
        let remote_ppoi_messages = Arc::new(SyncMutex::new(Vec::new()));
        let upgrade_intent_messages = Arc::new(SyncMutex::new(HashMap::new()));
        let comparison_results = Arc::new(SyncMutex::new(HashMap::new()));
        let state = PersistedState {
            local_attestations,
            remote_ppoi_messages,
            upgrade_intent_messages,
            comparison_results,
        };

        let old_result = ComparisonResult {
            deployment: String::from("existing_deployment"),
            block_number: 1,
            result_type: ComparisonResultType::Match,
            local_attestation: None,
            attestations: Vec::new(),
        };

        let new_result = ComparisonResult {
            deployment: String::from("existing_deployment"),
            block_number: 1,
            result_type: ComparisonResultType::Divergent,
            local_attestation: None,
            attestations: Vec::new(),
        };

        state
            .comparison_results
            .lock()
            .unwrap()
            .insert(String::from("existing_deployment"), old_result.clone());
        state
            .handle_comparison_result(new_result, notifier, NotificationMode::Live)
            .await;

        let comparison_results = state.comparison_results.lock().unwrap();
        let result = comparison_results
            .get(&String::from("existing_deployment"))
            .unwrap();
        assert_eq!(result.result_type, ComparisonResultType::Divergent);
    }

    #[tokio::test]
    async fn upgrade_ratelimiting() {
        let upgrade_threshold = 86400;
        let local_attestations = Arc::new(SyncMutex::new(HashMap::new()));
        let remote_ppoi_messages = Arc::new(SyncMutex::new(Vec::new()));
        let upgrade_intent_messages = Arc::new(SyncMutex::new(HashMap::new()));
        let comparison_results = Arc::new(SyncMutex::new(HashMap::new()));
        let test_id = "AAAMdCkW3pr619gsJVtUPAWxspALPdCMw6o7obzYBNp3".to_string();
        let state = PersistedState {
            local_attestations,
            remote_ppoi_messages,
            upgrade_intent_messages,
            comparison_results,
        };

        // Make 2 msgs
        let msg0 = UpgradeIntentMessage {
            subgraph_id: test_id.clone(),
            new_hash: "QmVVfLWowm1xkqc41vcygKNwFUvpsDSMbHdHghxmDVmH9x".to_string(),
            nonce: 1692307513,
            graph_account: "0xe9a1cabd57700b17945fd81feefba82340d9568f".to_string(),
        };
        let gc_msg0 = GraphcastMessage {
            identifier: "A0".to_string(),
            nonce: 1692307513,
            graph_account: "0xe9a1cabd57700b17945fd81feefba82340d9568f".to_string(),
            payload: msg0,
            signature: "0xA".to_string(),
        };
        let msg1 = UpgradeIntentMessage {
            subgraph_id: "BBBMdCkW3pr619gsJVtUPAWxspALPdCMw6o7obzYBNp3".to_string(),
            new_hash: "QmVVfLWowm1xkqc41vcygKNwFUvpsDSMbHdHghxmDVmH9x".to_string(),
            nonce: 1691307513,
            graph_account: "0xe9a1cabd57700b17945fd81feefba82340d9568f".to_string(),
        };
        let gc_msg1 = GraphcastMessage {
            identifier: "B".to_string(),
            nonce: 1691307513,
            graph_account: "0xe9a1cabd57700b17945fd81feefba82340d9568f".to_string(),
            payload: msg1,
            signature: "0xB".to_string(),
        };

        state.add_upgrade_intent_message(gc_msg0);
        state.add_upgrade_intent_message(gc_msg1);

        assert_eq!(state.upgrade_intent_messages().len(), 2);

        // Ratelimited by nonce
        let msg0 = UpgradeIntentMessage {
            subgraph_id: test_id.clone(),
            new_hash: "QmVVfLWowm1xkqc41vcygKNwFUvpsDSMbHdHghxmDVmH9x".to_string(),
            nonce: 1692307600,
            graph_account: "0xe9a1cabd57700b17945fd81feefba82340d9568f".to_string(),
        };

        assert!(state.recent_upgrade(&msg0, upgrade_threshold));

        // Update to new upgrade message
        let msg0 = UpgradeIntentMessage {
            subgraph_id: test_id.clone(),
            new_hash: "QmAAfLWowm1xkqc41vcygKNwFUvpsDSMbHdHghxmDVmH9x".to_string(),
            nonce: 1692407600,
            graph_account: "0xe9a1cabd57700b17945fd81feefba82340d9568f".to_string(),
        };
        assert!(!state.recent_upgrade(&msg0, upgrade_threshold));

        let gc_msg0 = GraphcastMessage {
            identifier: "A2".to_string(),
            nonce: 1692407600,
            graph_account: "0xe9a1cabd57700b17945fd81feefba82340d9568f".to_string(),
            payload: msg0,
            signature: "0xA".to_string(),
        };
        state.add_upgrade_intent_message(gc_msg0);

        assert_eq!(
            state.upgrade_intent_message(&test_id).unwrap().nonce,
            1692407600
        );
        assert_eq!(
            state
                .upgrade_intent_message(&test_id)
                .unwrap()
                .payload
                .new_hash,
            "QmAAfLWowm1xkqc41vcygKNwFUvpsDSMbHdHghxmDVmH9x".to_string()
        );
    }

    #[test]
    fn test_comparison_result_typed_not_found() {
        let mut comparison_results = HashMap::new();
        comparison_results.insert(
            "a".to_string(),
            ComparisonResult {
                result_type: ComparisonResultType::NotFound,
                deployment: String::from("Qmhash"),
                block_number: 100,
                local_attestation: None,
                attestations: vec![],
            },
        );
        comparison_results.insert(
            "b".to_string(),
            ComparisonResult {
                result_type: ComparisonResultType::Match,
                deployment: String::from("Qmhash"),
                block_number: 100,
                local_attestation: None,
                attestations: vec![],
            },
        );
        comparison_results.insert(
            "c".to_string(),
            ComparisonResult {
                result_type: ComparisonResultType::Match,
                deployment: String::from("Qmhash"),
                block_number: 100,
                local_attestation: None,
                attestations: vec![],
            },
        );
        comparison_results.insert(
            "d".to_string(),
            ComparisonResult {
                result_type: ComparisonResultType::Match,
                deployment: String::from("Qmhash"),
                block_number: 100,
                local_attestation: None,
                attestations: vec![],
            },
        );
        comparison_results.insert(
            "e".to_string(),
            ComparisonResult {
                result_type: ComparisonResultType::Divergent,
                deployment: String::from("Qmhash"),
                block_number: 100,
                local_attestation: None,
                attestations: vec![],
            },
        );
        comparison_results.insert(
            "f".to_string(),
            ComparisonResult {
                result_type: ComparisonResultType::NotFound,
                deployment: String::from("Qmhash"),
                block_number: 100,
                local_attestation: None,
                attestations: vec![],
            },
        );

        let state = PersistedState {
            comparison_results: Arc::new(SyncMutex::new(comparison_results)),
            local_attestations: Arc::new(SyncMutex::new(HashMap::new())),
            remote_ppoi_messages: Arc::new(SyncMutex::new(Vec::new())),
            upgrade_intent_messages: Arc::new(SyncMutex::new(HashMap::new())),
        };

        let results = state.comparison_result_typed(ComparisonResultType::Match);
        assert_eq!(results.len(), 3);
        let results = state.comparison_result_typed(ComparisonResultType::NotFound);
        assert_eq!(results.len(), 2);
        let results = state.comparison_result_typed(ComparisonResultType::Divergent);
        assert_eq!(results.len(), 1);
        let results = state.comparison_result_typed(ComparisonResultType::BuildFailed);
        assert_eq!(results.len(), 0);
    }
}
