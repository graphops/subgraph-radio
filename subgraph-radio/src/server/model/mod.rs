use async_graphql::{Context, EmptyMutation, EmptySubscription, Object, Schema, SimpleObject};
use chrono::Utc;

use std::{collections::HashMap, sync::Arc};
use thiserror::Error;

use crate::{
    config::Config,
    messages::{poi::PublicPoiMessage, upgrade::UpgradeIntentMessage},
    operator::attestation::{
        self, attestations_to_vec, compare_attestation, process_ppoi_message, Attestation,
        AttestationEntry, AttestationError, ComparisonResult, ComparisonResultType,
        LocalAttestationsMap,
    },
    state::PersistedState,
};
use graphcast_sdk::{graphcast_agent::message_typing::GraphcastMessage, graphql::QueryError};

pub(crate) type SubgraphRadioSchema = Schema<QueryRoot, EmptyMutation, EmptySubscription>;

// Unified query object for resolvers
#[derive(Default)]
pub struct QueryRoot;

#[Object]
impl QueryRoot {
    async fn public_poi_messages(
        &self,
        ctx: &Context<'_>,
        identifier: Option<String>,
        block: Option<u64>,
    ) -> Result<Vec<GraphcastMessage<PublicPoiMessage>>, HttpServiceError> {
        let msgs = ctx
            .data_unchecked::<Arc<SubgraphRadioContext>>()
            .remote_ppoi_messages_filtered(&identifier, &block);
        Ok(msgs)
    }

    async fn upgrade_intent_messages(
        &self,
        ctx: &Context<'_>,
        subgraph_id: Option<String>,
    ) -> Result<Vec<UpgradeIntentMessage>, HttpServiceError> {
        let msgs = ctx
            .data_unchecked::<Arc<SubgraphRadioContext>>()
            .upgrade_intent_messages_filtered(&subgraph_id)
            .into_iter()
            .map(|m| m.payload)
            .collect();
        Ok(msgs)
    }

    async fn local_attestations(
        &self,
        ctx: &Context<'_>,
        identifier: Option<String>,
        block: Option<u64>,
    ) -> Result<Vec<AttestationEntry>, HttpServiceError> {
        let attestations = ctx
            .data_unchecked::<Arc<SubgraphRadioContext>>()
            .local_attestations(identifier, block);
        let filtered = attestations_to_vec(&attestations);

        Ok(filtered)
    }

    /// Function that optionally takes in identifier and block filters.
    async fn comparison_results(
        &self,
        ctx: &Context<'_>,
        identifier: Option<String>,
        block: Option<u64>,
        result_type: Option<ComparisonResultType>,
    ) -> Result<Vec<ComparisonResult>, HttpServiceError> {
        let res = &ctx
            .data_unchecked::<Arc<SubgraphRadioContext>>()
            .comparison_results(identifier, block, result_type)
            .await;

        Ok(res.to_vec())
    }

    /// Function to grab the latest relevant comparison result of a deployment
    async fn comparison_result(
        &self,
        ctx: &Context<'_>,
        identifier: String,
    ) -> Result<Option<ComparisonResult>, HttpServiceError> {
        let res = &ctx
            .data_unchecked::<Arc<SubgraphRadioContext>>()
            .comparison_result(identifier);
        Ok(res.clone())
    }

    /// Return the sender ratio for remote attestations, with a "*" for the attestation matching local
    async fn comparison_ratio(
        &self,
        ctx: &Context<'_>,
        deployment: Option<String>,
        block: Option<u64>,
        result_type: Option<ComparisonResultType>,
    ) -> Result<Vec<CompareRatio>, HttpServiceError> {
        let res = self
            .comparison_results(ctx, deployment, block, result_type)
            .await?;
        let local_info = self.indexer_info(ctx).await?;

        let mut ratios = vec![];
        for r in res {
            let (aggregated_attestations, local_ppoi) =
                aggregate_attestation(r.clone(), &local_info);

            let (stake_ratio, sender_ratio) = calc_ratios(
                &aggregated_attestations,
                local_ppoi.as_deref(),
                local_info.stake,
            );
            ratios.push(CompareRatio::new(
                r.deployment,
                r.block_number,
                sender_ratio,
                stake_ratio,
            ));
        }
        Ok(ratios)
    }

    /// Return indexer info
    async fn indexer_info(&self, ctx: &Context<'_>) -> Result<IndexerInfo, HttpServiceError> {
        let config = ctx
            .data_unchecked::<Arc<SubgraphRadioContext>>()
            .radio_config();
        let basic_info = config
            .basic_info()
            .await
            .map_err(HttpServiceError::QueryError)?;
        Ok(IndexerInfo {
            address: basic_info.0.to_string(),
            stake: basic_info.1,
        })
    }
}

/// Helper function to order attestations by stake weight and then calculate stake and sender ratios
pub fn calc_ratios(
    attestations: &[Attestation],
    local_ppoi: Option<&str>,
    local_stake: f32,
) -> (String, String) {
    // Clone and sort the attestations by descending stake weight
    let mut temp_attestations = attestations.to_owned();
    temp_attestations.sort_by(|a, b| b.stake_weight.cmp(&a.stake_weight));

    let mut stakes: Vec<String> = Vec::new();
    let mut senders: Vec<String> = Vec::new();

    for att in temp_attestations.iter() {
        if local_ppoi.is_some() && att.ppoi.as_str() == local_ppoi.unwrap() {
            stakes.push(format!("{}*", att.stake_weight + local_stake as i64));
            senders.push(format!("{}*", att.senders.len() + 1));
        } else {
            // There is a local ppoi, but it's different compared to att.ppoi
            stakes.push(att.stake_weight.to_string());
            senders.push(att.senders.len().to_string());
        };
    }

    // Add zeros at the end if there is no local attestation
    if local_ppoi.is_none() {
        stakes.push("0*".to_string());
        senders.push("0*".to_string());
    }

    let stake_ratio: String = stakes.to_vec().join(":");
    let sender_ratio: String = senders.to_vec().join(":");

    (stake_ratio, sender_ratio)
}

pub async fn build_schema(ctx: Arc<SubgraphRadioContext>) -> SubgraphRadioSchema {
    Schema::build(QueryRoot, EmptyMutation, EmptySubscription)
        .data(ctx.persisted_state)
        .finish()
}

pub struct SubgraphRadioContext {
    pub radio_config: Config,
    pub persisted_state: &'static PersistedState,
}

impl SubgraphRadioContext {
    pub fn init(radio_config: Config, persisted_state: &'static PersistedState) -> Self {
        Self {
            radio_config,
            persisted_state,
        }
    }

    pub fn local_attestations(
        &self,
        identifier: Option<String>,
        block: Option<u64>,
    ) -> LocalAttestationsMap {
        let attestations = self.persisted_state.local_attestations();
        let mut empty_attestations: LocalAttestationsMap = HashMap::new();

        if let Some(deployment) = identifier {
            if let Some(deployment_attestations) = attestations.get(&deployment) {
                if let Some(block) = block {
                    if let Some(attestation) = deployment_attestations.get(&block) {
                        let single_entry = (block, attestation.clone());
                        let inner_map = vec![single_entry].into_iter().collect();

                        vec![(deployment, inner_map)].into_iter().collect()
                    } else {
                        // Return empty hashmap if no entry satisfy the supplied identifier and block
                        empty_attestations
                    }
                } else {
                    // Return all blocks since no block was specified
                    empty_attestations.insert(deployment, deployment_attestations.clone());
                    empty_attestations
                }
            } else {
                empty_attestations
            }
        } else {
            attestations
        }
    }

    pub fn remote_ppoi_messages(&self) -> Vec<GraphcastMessage<PublicPoiMessage>> {
        self.persisted_state.remote_ppoi_messages()
    }

    pub fn remote_ppoi_messages_filtered(
        &self,
        identifier: &Option<String>,
        block: &Option<u64>,
    ) -> Vec<GraphcastMessage<PublicPoiMessage>> {
        let msgs = self.remote_ppoi_messages();
        let filtered = msgs
            .iter()
            .filter(|&message| filter_remote_ppoi_messages(message, identifier, block))
            .cloned()
            .collect::<Vec<_>>();
        filtered
    }

    pub fn upgrade_intent_messages(
        &self,
    ) -> HashMap<String, GraphcastMessage<UpgradeIntentMessage>> {
        self.persisted_state.upgrade_intent_messages()
    }

    pub fn upgrade_intent_messages_filtered(
        &self,
        subgraph_id: &Option<String>,
    ) -> Vec<GraphcastMessage<UpgradeIntentMessage>> {
        subgraph_id
            .as_ref()
            .and_then(|id| self.upgrade_intent_messages().get(id).cloned())
            .map_or(vec![], |m| vec![m])
    }

    pub fn comparison_result(&self, identifier: String) -> Option<ComparisonResult> {
        let cmp_results = self.persisted_state.comparison_results();
        cmp_results.get(&identifier).cloned()
    }

    pub async fn comparison_results(
        &self,
        identifier: Option<String>,
        block: Option<u64>,
        result_type: Option<ComparisonResultType>,
    ) -> Vec<ComparisonResult> {
        // Simply take from persisted state if block is not specified
        if block.is_none() {
            let cmp_results = self.persisted_state.comparison_results();

            cmp_results
                .iter()
                .filter(|&(deployment, cmp_res)| {
                    (identifier.is_none() | (Some(deployment.clone()) == identifier))
                        && (result_type.is_none() | (Some(cmp_res.result_type) == result_type))
                })
                .map(|(_, cmp_res)| cmp_res.clone())
                .collect::<Vec<ComparisonResult>>()
        } else {
            // Calculate for the block if specified
            let locals = attestations_to_vec(&self.local_attestations(identifier.clone(), block));

            let config = self.radio_config();

            let mut res = vec![];
            for entry in locals {
                let deployment_identifier = entry.deployment.clone();
                let msgs = self.remote_ppoi_messages_filtered(&identifier, &block);
                let remote_attestations = process_ppoi_message(msgs, &config.callbook())
                    .await
                    .ok()
                    .and_then(|r| {
                        r.get(&deployment_identifier)
                            .and_then(|deployment_attestations| {
                                deployment_attestations.get(&entry.block_number).cloned()
                            })
                    })
                    .unwrap_or_default();

                let r = compare_attestation(entry, remote_attestations);
                if result_type.is_none() | (result_type.unwrap() == r.result_type) {
                    res.push(r);
                }
            }

            res
        }
    }

    pub fn radio_config(&self) -> Config {
        self.radio_config.clone()
    }
}

/// Filter funciton for Attestations on deployment and block
fn filter_remote_ppoi_messages(
    entry: &GraphcastMessage<PublicPoiMessage>,
    identifier: &Option<String>,
    block: &Option<u64>,
) -> bool {
    let is_matching_identifier = match identifier {
        Some(id) => entry.identifier == id.clone(),
        None => true, // Skip check
    };
    let is_matching_block = match block {
        Some(b) => entry.payload.block_number == *b,
        None => true, // Skip check
    };
    is_matching_identifier && is_matching_block
}

// Return attestation aggregated between remote and local, and the local ppoi
fn aggregate_attestation(
    r: ComparisonResult,
    local_info: &IndexerInfo,
) -> (Vec<Attestation>, Option<String>) {
    // Double check for local attestations to ensure there will be no divide by 0 during the ratio
    let local_attestation = if let Some(local_attestation) = r.local_attestation {
        local_attestation
    } else {
        // Simply return remote attestations
        return (r.attestations, None);
    };

    let local_ppoi = local_attestation.ppoi.clone();
    // Aggregate remote attestations with the local attestations
    let mut aggregated_attestations: Vec<Attestation> = vec![];
    let mut matched = false;
    for a in r.attestations {
        if a.ppoi == local_ppoi {
            let updated_attestation = attestation::Attestation::update(
                &a,
                local_info.address.clone(),
                local_info.stake,
                Utc::now().timestamp(),
            );
            if let Ok(updated_a) = updated_attestation {
                aggregated_attestations.push(updated_a);
            }
            matched = true;
        } else {
            aggregated_attestations.push(a)
        }
    }
    if !matched {
        aggregated_attestations.push(local_attestation);
    }
    (aggregated_attestations, Some(local_ppoi))
}

#[derive(Debug, PartialEq, Eq, Hash, SimpleObject)]
struct CompareRatio {
    deployment: String,
    block_number: u64,
    sender_ratio: String,
    stake_ratio: String,
}

impl CompareRatio {
    fn new(
        deployment: String,
        block_number: u64,
        sender_ratio: String,
        stake_ratio: String,
    ) -> Self {
        CompareRatio {
            deployment,
            block_number,
            sender_ratio,
            stake_ratio,
        }
    }
}

#[derive(Debug, PartialEq, SimpleObject)]
struct IndexerInfo {
    address: String,
    stake: f32,
}

#[derive(Error, Debug)]
pub enum HttpServiceError {
    #[error("Service processing failed: {0}")]
    AttestationError(AttestationError),
    #[error("Missing requested data: {0}")]
    MissingData(String),
    #[error("Query failed: {0}")]
    QueryError(QueryError),
    // Below ones are not used yet
    #[error("HTTP request failed: {0}")]
    RequestFailed(String),
    #[error("HTTP response error: {0}")]
    ResponseError(String),
    #[error("Timeout error")]
    TimeoutError,
    #[error("Invalid URL: {0}")]
    InvalidUrl(String),
    #[error("HTTP client error: {0}")]
    HttpClientError(#[from] reqwest::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_aggregate_attestations() {
        let attestation1 =
            Attestation::new("ppoi-1".to_string(), 1.0, vec!["0xa1".to_string()], vec![0]);
        let attestation2 =
            Attestation::new("ppoi-2".to_string(), 2.0, vec!["0xa2".to_string()], vec![1]);
        let attestation3 =
            Attestation::new("ppoi-3".to_string(), 3.0, vec!["0xa3".to_string()], vec![2]);

        // Matched local attestation
        let local_attestation = attestation1.clone();
        let attestations = vec![attestation1.clone(), attestation2, attestation3];
        let r = ComparisonResult {
            deployment: "test_deployment".to_string(),
            block_number: 42,
            result_type: ComparisonResultType::Divergent,
            local_attestation: Some(local_attestation.clone()),
            attestations: attestations.clone(),
        };

        let local_info = IndexerInfo {
            address: String::from("0xa0"),
            stake: 1.0,
        };

        let aggregated_attestation = aggregate_attestation(r, &local_info);
        assert_eq!(aggregated_attestation.0.len(), 3);
        assert_eq!(
            aggregated_attestation.clone().1.unwrap(),
            local_attestation.ppoi
        );
        let (stake_ratio, sender_ratio) = calc_ratios(
            &aggregated_attestation.0,
            aggregated_attestation.1.as_deref(),
            local_info.stake,
        );
        assert_eq!(sender_ratio, "1:3*:1");
        assert_eq!(stake_ratio, "3:3*:2");

        // Uniquely diverged local attestation
        let r = ComparisonResult {
            deployment: "test_deployment".to_string(),
            block_number: 42,
            result_type: ComparisonResultType::Divergent,
            local_attestation: Some(Attestation::new(
                String::from("unique_ppoi"),
                1.0,
                vec![String::from("0xa0")],
                vec![0],
            )),
            attestations: attestations.clone(),
        };

        let aggregated_attestation = aggregate_attestation(r, &local_info);
        assert_eq!(aggregated_attestation.0.len(), 4);
        let (stake_ratio, _) = calc_ratios(
            &aggregated_attestation.0,
            aggregated_attestation.1.as_deref(),
            local_info.stake,
        );
        assert_eq!(stake_ratio, "3:2:1:2*");

        // Missing local attestation, not found
        let r = ComparisonResult {
            deployment: "test_deployment".to_string(),
            block_number: 42,
            result_type: ComparisonResultType::NotFound,
            local_attestation: None,
            attestations: attestations.clone(),
        };

        let aggregated_attestation = aggregate_attestation(r, &local_info);
        assert_eq!(aggregated_attestation.0.len(), 3);
        assert_eq!(aggregated_attestation.1, None);
        let (stake_ratio, sender_ratio) = calc_ratios(
            &aggregated_attestation.0,
            aggregated_attestation.1.as_deref(),
            local_info.stake,
        );
        assert_eq!(sender_ratio, "1:1:1:0*");
        assert_eq!(stake_ratio, "3:2:1:0*");
    }
}
