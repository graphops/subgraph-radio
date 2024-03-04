use crate::database::{
    clear_all_notifications, create_notification, get_comparison_results_by_deployment,
    get_notifications, get_remote_ppoi_messages_by_identifier, save_comparison_result,
};
use crate::metrics::{
    ATTESTED_MAX_STAKE_WEIGHT, AVERAGE_PROCESSING_TIME, COMPARISON_RESULTS,
    FREQUENT_SENDERS_COUNTER, LATEST_MESSAGE_TIMESTAMP,
};
use crate::operator::notifier::NotificationMode;
use crate::DatabaseError;
use async_graphql::{Enum, Error as AsyncGraphqlError, ErrorExtensions, SimpleObject};
use autometrics::autometrics;
use chrono::Utc;
use serde_derive::{Deserialize, Serialize};
use sha3::{Digest, Sha3_256};
use sqlx::SqlitePool;
use std::error::Error;
use std::fmt;
use std::str::FromStr;
use std::time::Instant;
use std::{
    collections::{HashMap, HashSet},
    fmt::Display,
};
use thiserror::Error;

use tracing::{debug, error, info, trace, warn};

use graphcast_sdk::{
    callbook::CallBook,
    graphcast_agent::message_typing::{get_indexer_stake, GraphcastMessage, MessageError},
};

use crate::database::{get_local_attestations_by_identifier, insert_local_attestation};
use crate::{messages::poi::PublicPoiMessage, metrics::ACTIVE_INDEXERS, OperationError};

use super::notifier::Notification;
use super::Notifier;

#[derive(Clone, Debug, PartialEq, Eq, Hash, SimpleObject, Serialize, Deserialize)]
pub struct Attestation {
    pub identifier: String,
    pub block_number: u64,
    pub ppoi: String,
    pub stake_weight: u64,
    pub senders: Vec<String>,
    pub sender_group_hash: String,
    pub timestamp: Vec<u64>,
}

#[autometrics]
impl Attestation {
    pub fn new(
        identifier: String,
        block_number: u64,
        ppoi: String,
        stake_weight: u64,
        senders: Vec<String>,
        timestamp: Vec<u64>,
    ) -> Self {
        let addresses = &mut senders.clone();
        sort_addresses(addresses);
        let sender_group_hash = hash_addresses(addresses);
        Attestation {
            ppoi,
            stake_weight,
            senders,
            sender_group_hash,
            timestamp,
            identifier,
            block_number,
        }
    }

    /// Used whenever we receive a new attestation for an PPOI that already exists in the store
    pub fn update(
        base: &Self,
        address: String,
        stake: u64,
        timestamp: u64,
    ) -> Result<Self, AttestationError> {
        if base.senders.contains(&address) {
            Err(AttestationError::UpdateError(
                "There is already an attestation from this address. Skipping...".to_string(),
            ))
        } else {
            Ok(Self::new(
                base.identifier.clone(),
                base.block_number,
                base.ppoi.clone(),
                base.stake_weight + stake,
                [base.senders.clone(), vec![address]].concat(),
                [base.timestamp.clone(), vec![timestamp]].concat(),
            ))
        }
    }
}

impl fmt::Display for Attestation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "PPOI: {}\nsender addresses: {:#?}\nstake weight: {}",
            self.ppoi, self.senders, self.stake_weight
        )
    }
}

pub type RemoteAttestationsMap = HashMap<String, HashMap<u64, Vec<Attestation>>>;
pub type LocalAttestationsMap = HashMap<String, HashMap<u64, Attestation>>;

#[autometrics]
pub async fn process_ppoi_message(
    messages: Vec<GraphcastMessage<PublicPoiMessage>>,
    callbook: &CallBook,
) -> Result<RemoteAttestationsMap, AttestationError> {
    let start_time = Instant::now();

    let mut remote_attestations: RemoteAttestationsMap = HashMap::new();
    // Check if there are existing attestations for the block
    let first_message = messages.first();
    let first_msg = if first_message.is_none() {
        return Ok(remote_attestations);
    } else {
        first_message.unwrap()
    };

    for msg in messages.iter() {
        let radio_msg = &msg.payload.clone();
        // Message has passed GraphcastMessage validation, now check for radio validation
        let ppoi = radio_msg.payload_content().to_string();
        let sender_stake =
            get_indexer_stake(&radio_msg.graph_account.clone(), callbook.graph_network())
                .await
                .map_err(|e| AttestationError::BuildError(MessageError::FieldDerivations(e)))?
                as u64;

        FREQUENT_SENDERS_COUNTER
            .with_label_values(&[&radio_msg.graph_account])
            .inc();

        LATEST_MESSAGE_TIMESTAMP
            .with_label_values(&[&msg.identifier])
            .set(radio_msg.nonce as f64);

        let blocks = remote_attestations
            .entry(msg.identifier.to_string())
            .or_default();
        let attestations = blocks.entry(radio_msg.block_number).or_default();

        let existing_attestation = attestations.iter_mut().find(|a| a.ppoi == ppoi);

        if let Some(existing_attestation) = existing_attestation {
            if let Ok(updated_attestation) = Attestation::update(
                existing_attestation,
                msg.graph_account.clone(),
                sender_stake,
                msg.nonce,
            ) {
                // Replace the existing_attestation with the updated_attestation
                *existing_attestation = updated_attestation;
            }
        } else {
            attestations.push(Attestation::new(
                msg.identifier.clone(),
                msg.payload.block_number,
                radio_msg.payload_content().to_string(),
                sender_stake,
                vec![msg.graph_account.clone()],
                vec![msg.nonce],
            ));
        }
    }

    // update once at the end
    // active peers for each deployment
    debug!(
        num_msgs = messages.len(),
        num_attestation = remote_attestations.len(),
        "Process message into attestations",
    );
    // ppoi_hist by attestation - don't care for attestation but should be grouped together
    // so the summed up metrics should be ACTIVE_INDEXERS
    let blocks = remote_attestations
        .entry(first_msg.identifier.to_string())
        .or_default();

    let active_indexers = ACTIVE_INDEXERS.with_label_values(&[&first_msg.identifier.to_string()]);
    let senders = combine_senders(blocks.entry(first_msg.payload.block_number).or_default());
    active_indexers.set(senders.len().try_into().unwrap());

    let duration = start_time.elapsed();
    let average_time = duration.as_secs_f64() / messages.len() as f64;
    AVERAGE_PROCESSING_TIME.set(average_time);

    Ok(remote_attestations)
}

pub fn combine_senders(attestations: &[Attestation]) -> Vec<String> {
    <&[Attestation]>::clone(&attestations)
        .iter()
        .flat_map(|attestation| attestation.senders.clone())
        .collect()
}

#[derive(Debug, Error)]
pub enum ComparisonError {
    #[error("Database error: {0}")]
    DbError(DatabaseError),
    #[error("No comparison point found")]
    NoComparisonPoint,
}

/// Determine the comparison pointer on both block and time based on the local attestations
/// If they don't exist, then return default value that shall never be validated to trigger
pub async fn local_comparison_point(
    id: &str,
    collect_window_duration: u64,
    db: SqlitePool,
) -> Result<(u64, u64), ComparisonError> {
    let local_attestations = get_local_attestations_by_identifier(&db, id)
        .await
        .map_err(ComparisonError::DbError)?;
    let remote_messages = get_remote_ppoi_messages_by_identifier(&db, id)
        .await
        .map_err(ComparisonError::DbError)?;

    let remote_blocks: HashSet<u64> = remote_messages
        .iter()
        .filter(|m| m.identifier == id)
        .map(|m| m.payload.block_number)
        .collect();

    let min_attestation = local_attestations
        .into_iter()
        .filter(|attestation| remote_blocks.contains(&(attestation.block_number)))
        .min_by_key(|attestation| {
            attestation
                .timestamp
                .first()
                .map_or((u64::MAX, u64::MAX), |&timestamp| {
                    (attestation.block_number, timestamp)
                })
        });

    let comparison_point = min_attestation
        .map(|attestation| {
            let timestamp = *attestation.timestamp.first().unwrap_or(&0) + collect_window_duration;
            (attestation.block_number, timestamp)
        })
        .ok_or(ComparisonError::NoComparisonPoint)?;

    Ok(comparison_point)
}

/// Saves PPOIs that we've generated locally, in order to compare them with remote ones later
pub async fn save_local_attestation(
    pool: &SqlitePool,
    content: String,
    ipfs_hash: String,
    block_number: u64,
) -> Result<Attestation, OperationError> {
    let timestamp: Result<u64, _> = Utc::now().timestamp().try_into();
    let timestamp = match timestamp {
        Ok(t) => t,
        Err(e) => {
            return Err(OperationError::Others(format!(
                "Timestamp conversion failed: {}",
                e
            )));
        }
    };

    let attestation = Attestation {
        identifier: ipfs_hash,
        block_number,
        ppoi: content,
        stake_weight: 0,
        sender_group_hash: String::new(),
        senders: vec![],
        timestamp: vec![timestamp],
    };

    insert_local_attestation(pool, attestation)
        .await
        .map_err(OperationError::Database)
}

/// Tracks results indexed by deployment hash and block number
#[derive(Enum, Debug, PartialEq, Eq, Hash, Clone, Copy, Serialize, Deserialize)]
pub enum ComparisonResultType {
    NotFound,
    Divergent,
    Match,
    BuildFailed,
}

/// Keep track of the attestation result for a deployment and block
/// Can add block_hash and network fields for tracking if needed
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Hash, SimpleObject)]
pub struct ComparisonResult {
    pub deployment: String,
    pub block_number: u64,
    pub result_type: ComparisonResultType,
    pub local_attestation: Option<Attestation>,
    pub attestations: Vec<Attestation>,
}

impl ComparisonResult {
    pub fn deployment_hash(&self) -> String {
        self.deployment.clone()
    }

    pub fn block(&self) -> u64 {
        self.block_number
    }
}

pub async fn handle_comparison_result(
    pool: &SqlitePool,
    new_comparison_result: &ComparisonResult,
) -> Result<ComparisonResultType, DatabaseError> {
    let deployment_hash = new_comparison_result.deployment_hash();
    let existing_result = get_comparison_results_by_deployment(pool, &deployment_hash)
        .await?
        .into_iter()
        .next();

    COMPARISON_RESULTS
        .with_label_values(&[&deployment_hash])
        .inc();

    // Determine if the state has changed and if the database operation is successful
    let is_state_changed = existing_result.as_ref().map_or(true, |current_result| {
        current_result.result_type != new_comparison_result.result_type
    });
    let is_not_found_to_match = new_comparison_result.result_type == ComparisonResultType::Match
        && existing_result.map_or(false, |r| r.result_type == ComparisonResultType::NotFound);

    let rows_updated = save_comparison_result(pool, new_comparison_result).await?;

    // Only notify if there is a state change and an update in the database,
    // and exclude the case from NotFound to Match
    let should_notify = rows_updated > 0 && is_state_changed && !is_not_found_to_match;

    if should_notify {
        let new_notification = Notification {
            deployment: new_comparison_result.deployment.clone(),
            message: new_comparison_result.to_string(),
        };
        create_notification(pool, new_notification).await?;
    }

    Ok(new_comparison_result.result_type)
}

#[derive(Debug, Clone)]
pub struct ParseComparisonResultTypeError;

impl fmt::Display for ParseComparisonResultTypeError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "provided string did not match any ComparisonResultType variants"
        )
    }
}

impl Error for ParseComparisonResultTypeError {}

impl FromStr for ComparisonResultType {
    type Err = ParseComparisonResultTypeError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "NotFound" => Ok(ComparisonResultType::NotFound),
            "Divergent" => Ok(ComparisonResultType::Divergent),
            "Match" => Ok(ComparisonResultType::Match), // Accepting both for compatibility
            "BuildFailed" => Ok(ComparisonResultType::BuildFailed),
            _ => Err(ParseComparisonResultTypeError),
        }
    }
}

impl Display for ComparisonResultType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ComparisonResultType::NotFound => {
                write!(f, "NotFound")
            }
            ComparisonResultType::Divergent => {
                write!(f, "Divergent")
            }
            ComparisonResultType::Match => {
                write!(f, "Match")
            }
            ComparisonResultType::BuildFailed => write!(f, "Failed to build message"),
        }
    }
}

impl Display for ComparisonResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.result_type {
            ComparisonResultType::NotFound => {
                if self.local_attestation.is_none() {
                    write!(
                        f,
                        "{} for local attestation: deployment {} at block {}",
                        self.result_type,
                        self.deployment_hash(),
                        self.block()
                    )
                } else {
                    write!(
                        f,
                        "{} for remote attestations: deployment {} at block {}",
                        self.result_type,
                        self.deployment_hash(),
                        self.block()
                    )
                }
            }
            ComparisonResultType::Divergent => {
                write!(
                    f,
                    "{}: deployment {} at block {}",
                    self.result_type,
                    self.deployment_hash(),
                    self.block()
                )
            }
            ComparisonResultType::Match => {
                write!(
                    f,
                    "{}: deployment {} at block {}",
                    self.result_type,
                    self.deployment_hash(),
                    self.block()
                )
            }
            ComparisonResultType::BuildFailed => write!(
                f,
                "{}: deployment {} at block {}",
                self.result_type,
                self.deployment_hash(),
                self.block()
            ),
        }
    }
}

impl Clone for ComparisonResult {
    fn clone(&self) -> Self {
        ComparisonResult {
            deployment: self.deployment_hash(),
            block_number: self.block(),
            result_type: self.result_type,
            local_attestation: self.local_attestation.clone(),
            attestations: self.attestations.clone(),
        }
    }
}

/// Compares local attestations against remote ones using the attestation stores we populated while processing saved GraphcastMessage messages.
/// It takes our attestation (PPOI) for a given subgraph on a given block and compares it to the top-attested one from the remote attestations.
/// The top remote attestation is found by grouping attestations together and increasing their total stake-weight every time we see a new message
/// with the same PPOI from an Indexer (NOTE: one Indexer can only send 1 attestation per subgraph per block). The attestations are then sorted
/// - `local_attestation`: The local attestation data for a given block, if it exists.
/// - `attestation_block`: The specific block number we are comparing.
/// - `remote`: A map with a similar structure to what `local` was, but contains remote attestations.
/// - `ipfs_hash`: The identifier for the deployment whose attestations we are comparing.
pub fn compare_attestations(
    local_attestation: Option<Attestation>,
    attestation_block: u64,
    remote: &RemoteAttestationsMap,
    ipfs_hash: &str,
) -> ComparisonResult {
    // Attempt to retrieve remote attestations for the given IPFS hash and block number
    let remote_attestations = remote
        .get(ipfs_hash)
        .and_then(|blocks| blocks.get(&attestation_block))
        .cloned()
        .unwrap_or_default();

    // Sort remote attestations by stake weight in descending order
    let mut sorted_remote_attestations = remote_attestations;
    sorted_remote_attestations.sort_by(|a, b| {
        b.stake_weight
            .partial_cmp(&a.stake_weight)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let most_attested_poi = sorted_remote_attestations.last().cloned();

    let result_type = match (&most_attested_poi, &local_attestation) {
        (Some(most_attested), Some(local_att)) if most_attested.ppoi == local_att.ppoi => {
            ATTESTED_MAX_STAKE_WEIGHT
                .with_label_values(&[ipfs_hash])
                .set(most_attested.stake_weight as f64);
            ComparisonResultType::Match
        }
        (Some(most_attested), Some(_)) => {
            ATTESTED_MAX_STAKE_WEIGHT
                .with_label_values(&[ipfs_hash])
                .set(most_attested.stake_weight as f64);
            ComparisonResultType::Divergent
        }
        (Some(most_attested), None) => {
            ATTESTED_MAX_STAKE_WEIGHT
                .with_label_values(&[ipfs_hash])
                .set(most_attested.stake_weight as f64);
            ComparisonResultType::NotFound
        }
        (None, _) => ComparisonResultType::NotFound,
    };

    // Construct the comparison result
    ComparisonResult {
        deployment: ipfs_hash.to_string(),
        block_number: attestation_block,
        result_type,
        local_attestation,
        attestations: sorted_remote_attestations,
    }
}

/// Assume that local and remote has already been matched with the desired deployment and block
pub fn compare_attestation(
    local_attestation: Attestation,
    remote_attestations: Vec<Attestation>,
) -> ComparisonResult {
    let mut remote_attestations = remote_attestations;
    remote_attestations.sort_by(|a, b| {
        a.stake_weight
            .partial_cmp(&b.stake_weight)
            .expect("Could not compare stake values")
    });

    let most_attested_ppoi = &remote_attestations.last().unwrap().ppoi;
    if most_attested_ppoi == &local_attestation.ppoi {
        trace!(
            local_attestation.block_number,
            remote_attestations = tracing::field::debug(&remote_attestations),
            local_attestation = tracing::field::debug(&local_attestation),
            "pPOI matched",
        );
        ComparisonResult {
            deployment: local_attestation.identifier.to_string(),
            block_number: local_attestation.block_number,
            result_type: ComparisonResultType::Match,
            local_attestation: Some(local_attestation),
            attestations: remote_attestations,
        }
    } else {
        warn!(
            block = local_attestation.block_number,
            remote_attestations = tracing::field::debug(&remote_attestations),
            local_attestation = tracing::field::debug(&local_attestation),
            "Detected divergence",
        );
        ComparisonResult {
            deployment: local_attestation.identifier.to_string(),
            block_number: local_attestation.block_number,
            result_type: ComparisonResultType::Divergent,
            local_attestation: Some(local_attestation),
            attestations: remote_attestations,
        }
    }
}

/// Deterministically sort addresses
fn sort_addresses(addresses: &mut [String]) {
    addresses.sort_by(|a, b| {
        let bytes_a = hex::decode(&a[2..]).unwrap();
        let bytes_b = hex::decode(&b[2..]).unwrap();
        bytes_a.cmp(&bytes_b)
    });
}

/// Deterministically ordering the indexer addresses attesting to a pPOI, and then hashing that list
fn hash_addresses(addresses: &[String]) -> String {
    // create a SHA3-256 object
    let mut hasher = Sha3_256::new();
    // iteratively decode addresses to bytes
    let mut bytes = Vec::new();
    for address in addresses {
        let addr = address[2..].to_string();
        bytes.extend(hex::decode(addr).unwrap());
    }

    // write input message
    hasher.update(&bytes);
    // read hash digest
    let result = hasher.finalize();
    hex::encode(result)
}

/// This function logs the operational summary of the main event loop
#[allow(clippy::too_many_arguments)]
pub async fn log_gossip_summary(
    blocks_str: String,
    num_topics: usize,
    messages_sent: Vec<Result<String, OperationError>>,
) {
    // Generate gossip summary
    let mut send_success = vec![];
    let mut trigger_failed = vec![];
    let mut skip_repeated = vec![];
    let mut build_errors = vec![];
    for result in messages_sent {
        match result {
            Ok(s) => send_success.push(s),
            Err(OperationError::SendTrigger(e)) => trigger_failed.push(e),
            Err(OperationError::SkipDuplicate(e)) => skip_repeated.push(e),
            Err(e) => build_errors.push(e),
        }
    }

    info!(
        chainhead = blocks_str,
        num_topics,
        num_sent_success = send_success.len(),
        num_sent_previously = skip_repeated.len(),
        num_syncing_to_chainhead = trigger_failed.len(),
        build_errors = tracing::field::debug(&build_errors),
        "Gossip summary",
    );
}

/// This function logs the operational summary of the main event loop
pub async fn process_comparison_results(
    blocks_str: String,
    num_topics: usize,
    result_strings: Vec<Result<ComparisonResult, OperationError>>,
    notifier: Notifier,
    db: SqlitePool,
) {
    // Generate attestation summary
    let mut match_strings = vec![];
    let mut not_found_strings = vec![];
    let mut divergent_strings = vec![];
    let mut cmp_trigger_failed = vec![];
    let mut attestation_failed = vec![];
    let mut cmp_errors = vec![];

    for result in result_strings {
        match result {
            Ok(comparison_result) => {
                match handle_comparison_result(&db, &comparison_result.clone()).await {
                    Ok(result_type) => match result_type {
                        ComparisonResultType::Match => {
                            match_strings.push(comparison_result.to_string());
                        }
                        ComparisonResultType::NotFound => {
                            not_found_strings.push(comparison_result.to_string());
                        }
                        ComparisonResultType::Divergent => {
                            divergent_strings.push(comparison_result.to_string());
                        }
                        _ => attestation_failed.push(comparison_result.to_string()),
                    },
                    Err(e) => {
                        let operation_error = OperationError::Database(e);
                        cmp_errors.push(format!("{:?}", operation_error));
                    }
                }
            }
            Err(OperationError::CompareTrigger(_, _, e)) => cmp_trigger_failed.push(e.to_string()),
            Err(OperationError::Attestation(e)) => attestation_failed.push(e.to_string()),
            Err(e) => cmp_errors.push(format!("{:?}", e)),
        }
    }

    let notifications_result = get_notifications(&db).await;
    match notifications_result {
        Ok(notifications) => {
            if notifier.notification_mode == NotificationMode::Live && !notifications.is_empty() {
                let messages: Vec<String> = notifications
                    .iter()
                    .map(|notification| notification.message.clone())
                    .collect();
                notifier.notify(messages.join("\n")).await;

                if let Err(e) = clear_all_notifications(&db).await {
                    error!("Error clearing notifications: {:?}", e);
                }
            }
        }
        Err(e) => {
            error!("Error getting notifications: {:?}", e);
        }
    }

    info!(
        chainhead_blocks = blocks_str,
        num_topics,
        num_active_crosschecks = match_strings.len() + divergent_strings.len(),
        num_attestations_matched = match_strings.len(),
        num_topics_inactive = not_found_strings.len(),
        num_waiting_to_compare = cmp_trigger_failed.len(),
        diverged = tracing::field::debug(divergent_strings),
        attestation_failed = tracing::field::debug(attestation_failed),
        comparison_errors = tracing::field::debug(cmp_errors),
        "Comparison state",
    );
}

#[derive(Debug, thiserror::Error)]
pub enum AttestationError {
    #[error("Failed to build attestation: {0}")]
    BuildError(MessageError),
    #[error("Failed to update attestation: {0}")]
    UpdateError(String),
    #[error("Integer conversion error: {0}")]
    IntConversionError(String),
}

impl ErrorExtensions for AttestationError {
    fn extend(&self) -> AsyncGraphqlError {
        AsyncGraphqlError::new(format!("{}", self))
    }
}

impl From<std::num::TryFromIntError> for AttestationError {
    fn from(err: std::num::TryFromIntError) -> Self {
        AttestationError::IntConversionError(err.to_string())
    }
}
