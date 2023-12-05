use async_graphql::{Context, EmptyMutation, EmptySubscription, Object, Schema, SimpleObject};
use chrono::Utc;
use sqlx::{Error as SqlxError, SqlitePool};

use crate::{
    database::{
        get_comparison_results, get_comparison_results_by_deployment, get_upgrade_intent_messages,
    },
    OperationError,
};
use std::{collections::HashMap, sync::Arc};
use thiserror::Error;

use crate::{
    config::Config,
    database::{
        get_local_attestation, get_local_attestations, get_local_attestations_by_identifier,
        get_remote_ppoi_messages,
    },
    messages::{poi::PublicPoiMessage, upgrade::UpgradeIntentMessage},
    operator::attestation::{
        self, compare_attestation, process_ppoi_message, Attestation, AttestationError,
        ComparisonResult, ComparisonResultType,
    },
};
use graphcast_sdk::{
    graphcast_agent::{message_typing::GraphcastMessage, GraphcastAgent, PeerData},
    graphql::QueryError,
};

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
        let pool = &ctx.data_unchecked::<Arc<SubgraphRadioContext>>().db;

        let msgs = ctx
            .data_unchecked::<Arc<SubgraphRadioContext>>()
            .remote_ppoi_messages_filtered(&identifier, &block, pool)
            .await;
        Ok(msgs)
    }

    async fn upgrade_intent_messages(
        &self,
        ctx: &Context<'_>,
        subgraph_id: Option<String>,
    ) -> Result<Vec<UpgradeIntentMessage>, HttpServiceError> {
        let pool = &ctx.data_unchecked::<Arc<SubgraphRadioContext>>().db;

        let msgs = ctx
            .data_unchecked::<Arc<SubgraphRadioContext>>()
            .upgrade_intent_messages_filtered(&subgraph_id, pool)
            .await
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
    ) -> Result<Vec<Attestation>, HttpServiceError> {
        let pool = &ctx.data_unchecked::<Arc<SubgraphRadioContext>>().db;
        let attestations = SubgraphRadioContext::local_attestations(pool, identifier, block).await;

        Ok(attestations)
    }

    /// Function that optionally takes in identifier and block filters.
    async fn comparison_results(
        &self,
        ctx: &Context<'_>,
        identifier: Option<String>,
        block: Option<u64>,
        result_type: Option<ComparisonResultType>,
    ) -> Result<Vec<ComparisonResult>, HttpServiceError> {
        let pool = &ctx.data_unchecked::<Arc<SubgraphRadioContext>>().db;

        let res = &ctx
            .data_unchecked::<Arc<SubgraphRadioContext>>()
            .comparison_results(pool, identifier, block, result_type)
            .await;

        Ok(res.to_vec())
    }

    /// Function to grab the latest relevant comparison result of a deployment
    async fn comparison_result(
        &self,
        ctx: &Context<'_>,
        identifier: String,
    ) -> Result<Option<ComparisonResult>, HttpServiceError> {
        let pool = &ctx.data_unchecked::<Arc<SubgraphRadioContext>>().db;

        let res = &SubgraphRadioContext::comparison_result(pool, &identifier).await;
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
            match aggregate_attestation(r.clone(), &local_info) {
                Ok((aggregated_attestations, local_ppoi)) => {
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
                Err(e) => {
                    return Err(HttpServiceError::OperationError(e));
                }
            }
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

    /// Return Waku Peer data excluding local waku node
    async fn gossip_peers(&self, ctx: &Context<'_>) -> Result<Vec<PeerData>, HttpServiceError> {
        let peers = ctx
            .data_unchecked::<Arc<SubgraphRadioContext>>()
            .gossip_peers();
        Ok(peers)
    }

    /// Return Waku Peer data for the local waku node
    async fn local_gossip_peer(
        &self,
        ctx: &Context<'_>,
    ) -> Result<Option<PeerData>, HttpServiceError> {
        let peer = ctx
            .data_unchecked::<Arc<SubgraphRadioContext>>()
            .local_gossip_node();
        Ok(peer)
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
            stakes.push(format!("{}*", att.stake_weight + local_stake as u64));
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
        .data(ctx.db.clone())
        .finish()
}

pub struct SubgraphRadioContext {
    pub radio_config: Config,
    pub db: SqlitePool,
    pub graphcast_agent: &'static GraphcastAgent,
}

impl SubgraphRadioContext {
    pub fn init(
        radio_config: Config,
        db: &SqlitePool,
        graphcast_agent: &'static GraphcastAgent,
    ) -> Self {
        Self {
            radio_config,
            db: db.clone(),
            graphcast_agent,
        }
    }

    pub async fn local_attestations(
        pool: &SqlitePool,
        identifier: Option<String>,
        block: Option<u64>,
    ) -> Vec<Attestation> {
        let db_records: Vec<Attestation> = match (identifier, block) {
            (Some(deployment_id), Some(block_num)) => {
                get_local_attestation(pool, &deployment_id, block_num)
                    .await
                    .unwrap_or_default()
                    .into_iter()
                    .collect()
            }
            (Some(deployment_id), None) => {
                get_local_attestations_by_identifier(pool, &deployment_id)
                    .await
                    .unwrap_or_default()
            }
            (None, None) => get_local_attestations(pool).await.unwrap_or_default(),
            (None, Some(_)) => Vec::new(),
        };

        db_records.into_iter().collect()
    }

    pub async fn remote_ppoi_messages(
        &self,
        pool: &SqlitePool,
    ) -> Vec<GraphcastMessage<PublicPoiMessage>> {
        get_remote_ppoi_messages(pool).await.unwrap_or_default()
    }

    pub async fn remote_ppoi_messages_filtered(
        &self,
        identifier: &Option<String>,
        block: &Option<u64>,
        pool: &SqlitePool,
    ) -> Vec<GraphcastMessage<PublicPoiMessage>> {
        let msgs = self.remote_ppoi_messages(pool).await;
        let filtered = msgs
            .iter()
            .filter(|&message| filter_remote_ppoi_messages(message, identifier, block))
            .cloned()
            .collect::<Vec<_>>();
        filtered
    }

    pub async fn upgrade_intent_messages(
        pool: &SqlitePool,
    ) -> Result<HashMap<String, GraphcastMessage<UpgradeIntentMessage>>, SqlxError> {
        let records = get_upgrade_intent_messages(pool).await?;

        let mut messages = HashMap::new();
        for record in records {
            let msg = GraphcastMessage {
                identifier: record.deployment.clone(),
                nonce: record.nonce,
                graph_account: record.graph_account.clone(),
                signature: record.signature.clone(),
                payload: UpgradeIntentMessage {
                    deployment: record.deployment,
                    subgraph_id: record.subgraph_id,
                    new_hash: record.new_hash,
                    nonce: record.nonce,
                    graph_account: record.graph_account,
                    signature: record.signature,
                },
            };
            messages.insert(msg.payload.subgraph_id.clone(), msg);
        }

        Ok(messages)
    }

    pub async fn upgrade_intent_messages_filtered(
        &self,
        subgraph_id: &Option<String>,
        pool: &SqlitePool,
    ) -> Vec<GraphcastMessage<UpgradeIntentMessage>> {
        let msgs = SubgraphRadioContext::upgrade_intent_messages(pool)
            .await
            .unwrap_or_default();

        subgraph_id
            .as_ref()
            .and_then(|id| msgs.get(id).cloned())
            .map_or(vec![], |m| vec![m])
    }

    pub async fn comparison_result(
        pool: &SqlitePool,
        deployment_hash: &str,
    ) -> Option<ComparisonResult> {
        let records = get_comparison_results_by_deployment(pool, deployment_hash)
            .await
            .ok()?;

        let record = records.into_iter().next()?;
        Some(record)
    }

    pub async fn comparison_results(
        &self,
        pool: &SqlitePool,
        identifier: Option<String>,
        block: Option<u64>,
        result_type: Option<ComparisonResultType>,
    ) -> Vec<ComparisonResult> {
        if let Some(block) = block {
            let locals =
                SubgraphRadioContext::local_attestations(pool, identifier.clone(), Some(block))
                    .await;

            let config = self.radio_config();

            let mut res = Vec::new();
            for entry in locals {
                let msgs = self
                    .remote_ppoi_messages_filtered(&identifier, &Some(block), pool)
                    .await;

                let remote_attestations = process_ppoi_message(msgs, &config.callbook())
                    .await
                    .ok()
                    .and_then(|r| {
                        r.get(&entry.identifier)
                            .and_then(|deployment_attestations| {
                                deployment_attestations.get(&entry.block_number).cloned()
                            })
                    })
                    .unwrap_or_default();

                let r = compare_attestation(entry, remote_attestations);
                if result_type.is_none() || (Some(r.result_type) == result_type) {
                    res.push(r);
                }
            }

            res
        } else {
            let comparison_results = match get_comparison_results(pool).await {
                Ok(results) => results,
                Err(_) => return Vec::new(),
            };

            // Filter the ComparisonResults based on identifier and result_type
            let filtered_results = comparison_results
                .into_iter()
                .filter(|cmp_res| {
                    (identifier.is_none() || Some(&cmp_res.deployment) == identifier.as_ref())
                        && (result_type.is_none()
                            || Some(cmp_res.result_type.to_string().as_str())
                                == result_type.map(|rt| rt.to_string()).as_deref())
                })
                .collect::<Vec<ComparisonResult>>();

            filtered_results
        }
    }

    pub fn radio_config(&self) -> Config {
        self.radio_config.clone()
    }

    pub fn gossip_peers(&self) -> Vec<PeerData> {
        self.graphcast_agent
            .peers_data()
            .unwrap_or_default()
            .into_iter()
            .map(|p| PeerData {
                peer_id: p.peer_id().to_string(),
                protocols: p.protocols().iter().map(|p| p.to_string()).collect(),
                addresses: p.addresses().iter().map(|a| a.to_string()).collect(),
                connected: p.connected(),
            })
            .collect()
    }

    pub fn local_gossip_node(&self) -> Option<PeerData> {
        self.graphcast_agent.local_peer().map(|p| PeerData {
            peer_id: p.peer_id().to_string(),
            protocols: p.protocols().iter().map(|p| p.to_string()).collect(),
            addresses: p.addresses().iter().map(|a| a.to_string()).collect(),
            connected: p.connected(),
        })
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
) -> Result<(Vec<Attestation>, Option<String>), OperationError> {
    let local_attestation = if let Some(local_attestation) = r.local_attestation {
        local_attestation
    } else {
        return Ok((r.attestations, None));
    };

    let local_ppoi = local_attestation.ppoi.clone();
    let mut aggregated_attestations: Vec<Attestation> = vec![];
    let mut matched = false;

    let current_timestamp: Result<_, _> = Utc::now().timestamp().try_into();
    let current_timestamp = match current_timestamp {
        Ok(t) => t,
        Err(e) => {
            return Err(OperationError::Others(format!(
                "Timestamp conversion failed: {}",
                e
            )))
        }
    };

    for a in r.attestations {
        if a.ppoi == local_ppoi {
            let updated_attestation = attestation::Attestation::update(
                &a,
                local_info.address.clone(),
                local_info.stake,
                current_timestamp, // Use the converted timestamp
            );
            if let Ok(updated_a) = updated_attestation {
                aggregated_attestations.push(updated_a);
            }
            matched = true;
        } else {
            aggregated_attestations.push(a);
        }
    }

    if !matched {
        aggregated_attestations.push(local_attestation);
    }
    Ok((aggregated_attestations, Some(local_ppoi)))
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
    #[error("Operation error: {0}")]
    OperationError(OperationError),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_aggregate_attestations() {
        let attestation1 = Attestation::new(
            "test_deployment".to_string(),
            40,
            "ppoi-1".to_string(),
            1.0,
            vec!["0xa1".to_string()],
            vec![0],
        );
        let attestation2 = Attestation::new(
            "test_deployment".to_string(),
            40,
            "ppoi-2".to_string(),
            2.0,
            vec!["0xa2".to_string()],
            vec![1],
        );
        let attestation3 = Attestation::new(
            "test_deployment".to_string(),
            40,
            "ppoi-3".to_string(),
            3.0,
            vec!["0xa3".to_string()],
            vec![2],
        );

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

        let aggregated_attestation = aggregate_attestation(r, &local_info).unwrap();
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
                "test_deployment".to_string(),
                40,
                String::from("unique_ppoi"),
                1.0,
                vec![String::from("0xa0")],
                vec![0],
            )),
            attestations: attestations.clone(),
        };

        let aggregated_attestation = aggregate_attestation(r, &local_info).unwrap();
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

        let aggregated_attestation = aggregate_attestation(r, &local_info).unwrap();
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
