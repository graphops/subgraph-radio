use std::env;
use std::path::Path;
use std::sync::{atomic::Ordering, mpsc::Receiver, Arc};
use std::time::Duration;

use ethers_core::types::transaction::eip712::Eip712;
use graphcast_sdk::{
    graphcast_agent::{
        message_typing::{check_message_validity, GraphcastMessage},
        waku_handling::WakuHandlingError,
        GraphcastAgent,
    },
    graphql::client_graph_node::{subgraph_network_blocks, update_network_chainheads},
    WakuMessage,
};

use sqlx::SqlitePool;
use tokio::time::{interval, sleep, timeout};
use tracing::{debug, error, info, trace, warn};

use crate::entities::{
    clear_all_notifications, fetch_and_cache_attestations, get_comparison_results,
    get_comparison_results_by_type, get_notifications, save_upgrade_intent_message,
};
use crate::messages::upgrade::UpgradeIntentMessage;
use crate::metrics::handle_serve_metrics;
use crate::operator::attestation::log_gossip_summary;
use crate::operator::attestation::process_comparison_results;
use crate::operator::notifier::NotificationMode;
use crate::server::run_server;
use crate::GRAPHCAST_AGENT;
use crate::{
    chainhead_block_str,
    messages::poi::{process_valid_message, PublicPoiMessage},
    metrics::{
        CONNECTED_PEERS, DIVERGING_SUBGRAPHS, GOSSIP_PEERS, RECEIVED_MESSAGES, VALIDATED_MESSAGES,
    },
    operator::{attestation::ComparisonResultType, indexer_management::health_query},
};
use crate::{config::Config, shutdown, ControlFlow};

use self::notifier::Notifier;

pub mod attestation;
pub mod callbook;
pub mod indexer_management;
pub mod notifier;
pub mod operation;

/// Radio operator contains all states needed for radio operations
#[allow(unused)]
pub struct RadioOperator {
    config: Config,
    graphcast_agent: Arc<GraphcastAgent>,
    notifier: Notifier,
    control_flow: ControlFlow,
    pub db: SqlitePool,
}

impl RadioOperator {
    /// Create a radio operator with radio configurations, persisted data,
    /// graphcast agent, and control flow
    pub async fn new(config: &Config, agent: GraphcastAgent) -> RadioOperator {
        debug!("Connecting to database");

        let db = match &config.radio_setup().sqlite_file_path {
            Some(path) => {
                let cwd = env::current_dir().unwrap();
                let absolute_path = cwd.join(path);

                if !Path::new(&absolute_path).exists() {
                    std::fs::File::create(&absolute_path)
                        .expect("Failed to create the database file");
                    debug!("Database file created at {}", absolute_path.display());
                }

                let db_url = format!("sqlite://{}", absolute_path.display());

                SqlitePool::connect(&db_url)
                    .await
                    .expect("Could not connect to the SQLite database")
            }
            None => SqlitePool::connect("sqlite::memory:")
                .await
                .expect("Failed to connect to the in-memory database"),
        };

        debug!("Check for database migration");
        sqlx::migrate!()
            .run(&db)
            .await
            .expect("Could not run migration");

        debug!("Initializing Graphcast Agent");
        let graphcast_agent = Arc::new(agent);

        debug!("Set global static instance of graphcast_agent");
        _ = GRAPHCAST_AGENT.set(graphcast_agent.clone());

        //TODO: Refactor indexer management server validation to SDK, similar to graph node status endpoint
        if let Some(url) = &config.graph_stack.indexer_management_server_endpoint {
            _ = health_query(url)
                .await
                .expect("Failed to validate the provided indexer management server endpoint");
        };
        let notifier = Notifier::from_config(config);
        let control_flow = ControlFlow::new();

        // Spawn a task to gracefully shutdown
        tokio::spawn(shutdown(control_flow.clone()));

        // Set up Prometheus metrics url if configured
        if let Some(port) = config.radio_setup().metrics_port {
            debug!("Initializing metrics port");
            tokio::spawn(handle_serve_metrics(
                config.radio_setup().metrics_host.clone(),
                port,
                control_flow.metrics_handle.clone(),
            ));
        }

        // Provide generated topics to Graphcast agent
        let topics = config
            .generate_topics(&config.radio_setup().gossip_topic_coverage)
            .await;
        debug!(
            topics = tracing::field::debug(&topics),
            "Found content topics for subscription",
        );
        graphcast_agent.update_content_topics(topics.clone());

        RadioOperator {
            config: config.clone(),
            graphcast_agent,
            notifier,
            control_flow,
            db,
        }
    }

    pub fn graphcast_agent(&self) -> &GraphcastAgent {
        &self.graphcast_agent
    }

    /// Radio operations
    pub async fn run(&'static self) {
        // Control flow
        let mut topic_update_interval = interval(Duration::from_secs(
            self.config.radio_setup.topic_update_interval,
        ));

        let mut state_update_interval = interval(self.control_flow.update_event);
        let mut gossip_poi_interval = interval(self.control_flow.gossip_event);
        let mut comparison_interval = interval(self.control_flow.compare_event);
        let mut cache_refresh_interval = interval(Duration::from_secs(240));

        let mut notification_interval = tokio::time::interval(Duration::from_secs(
            self.config.radio_setup.notification_interval * 3600,
        ));

        let iteration_timeout = self.control_flow.iteration_timeout;
        let update_timeout = self.control_flow.update_timeout;
        let gossip_timeout = self.control_flow.gossip_timeout;

        // Separate thread to skip a main loop iteration when hit timeout
        tokio::spawn(async move {
            tokio::time::sleep(iteration_timeout).await;
            self.control_flow
                .skip_iteration
                .store(true, Ordering::SeqCst);
        });

        // Initialize Http server with graceful shutdown if configured
        if self.config.radio_setup().server_port.is_some() {
            let config_cloned = self.config.clone();
            tokio::spawn(run_server(
                config_cloned,
                &self.db,
                self.graphcast_agent(),
                self.control_flow.server_handle.clone(),
            ));
        }

        tokio::spawn(async move {
            let _ = fetch_and_cache_attestations(&self.db).await;
        });

        // Main loop for sending messages, can factor out
        // and take radio specific query and parsing for radioPayload
        while self.control_flow.running.load(Ordering::SeqCst) {
            // Run event intervals sequentially by satisfication of other intervals and corresponding tick
            tokio::select! {
                _ = topic_update_interval.tick() => {
                    if self.control_flow.skip_iteration.load(Ordering::SeqCst) {
                        self.control_flow.skip_iteration.store(false, Ordering::SeqCst);
                        continue;
                    }
                    // Update topic subscription
                    let result = timeout(update_timeout,
                        self.config.generate_topics(
                            &self.config.radio_setup().gossip_topic_coverage)
                    ).await;

                    if result.is_err() {
                        warn!("update_content_topics timed out");
                    } else {
                        self.graphcast_agent()
                        .update_content_topics(result.unwrap());
                        debug!("update_content_topics completed");
                    }
                },
                _ = state_update_interval.tick() => {
                    if self.control_flow.skip_iteration.load(Ordering::SeqCst) {
                        self.control_flow.skip_iteration.store(false, Ordering::SeqCst);
                        continue;
                    }
                    // Update the number of peers connected
                    let connected_peers = self.graphcast_agent.connected_peer_count().unwrap_or_default() as i64;
                    let gossip_peers = self.graphcast_agent.number_of_peers() as i64;
                    CONNECTED_PEERS.set(connected_peers);
                    GOSSIP_PEERS.set(gossip_peers);

                    match get_comparison_results_by_type(&self.db, "Divergent").await {
                        Ok(results) => {
                            let diverged_num = results.len();
                            DIVERGING_SUBGRAPHS.set(diverged_num.try_into().unwrap());
                        },
                        Err(e) => {
                            error!("Error fetching divergent comparison results: {:?}", e);
                        }
                    }

                    info!(connected_peers, gossip_peers, "State update summary");
                },
                _ = gossip_poi_interval.tick() => {
                    if self.control_flow.skip_iteration.load(Ordering::SeqCst) {
                        self.control_flow.skip_iteration.store(false, Ordering::SeqCst);
                        continue;
                    }

                    let result = timeout(gossip_timeout, {
                        // Update all the chainheads of the network
                        // Also get a hash map returned on the subgraph mapped to network name and latest block
                        let network_chainhead_blocks = match self.config.callbook().indexing_statuses().await {
                            Ok(res) => update_network_chainheads(
                                res,
                            ),
                            Err(e) => {
                                error!(err = tracing::field::debug(&e), "Could not query indexing statuses, failed to get network chainhead, pull again later");
                                continue;
                            }
                        };
                        // Separate calls to indexing_statuses as it is not cloneable
                        let subgraph_network_latest_blocks = match self.config.callbook().indexing_statuses().await {
                            Ok(res) => subgraph_network_blocks(res),
                            Err(e) => {
                                error!(err = tracing::field::debug(&e), "Could not query indexing statuses, failed to get subgraph latest block, pull again later");
                                continue;
                            }
                        };

                        trace!(
                            network_pointers = tracing::field::debug(&subgraph_network_latest_blocks),
                            "Subgraph network and latest blocks",
                        );

                        // Radio specific message content query function
                        // Function takes in an identifier string and make specific queries regarding the identifier
                        // The example here combines a single function provided query endpoint, current block info based on the subgraph's indexing network
                        // Then the function gets sent to agent for making identifier independent queries
                        let identifiers = self.graphcast_agent.content_identifiers();
                        let num_topics = identifiers.len();
                        let blocks_str = chainhead_block_str(&network_chainhead_blocks);
                        info!(
                            chainhead = blocks_str.clone(),
                            num_gossip_peers = self.graphcast_agent.number_of_peers(),
                            num_connected_peers = self.graphcast_agent.connected_peer_count().unwrap_or_default(),
                            num_topics,
                            "Network statuses",
                        );

                        let send_ops = self.gossip_poi(
                            identifiers.clone(),
                            &network_chainhead_blocks.clone(),
                            &subgraph_network_latest_blocks,
                        ).await;

                        log_gossip_summary(
                            blocks_str,
                            identifiers.len(),
                            send_ops,
                        )
                    }).await;

                    if result.is_err() {
                        warn!("gossip_poi timed out");
                    } else {
                        debug!("gossip_poi completed");
                    }
                },
                _ = comparison_interval.tick() => {
                    if self.control_flow.skip_iteration.load(Ordering::SeqCst) {
                        self.control_flow.skip_iteration.store(false, Ordering::SeqCst);
                        continue;
                    }

                    let result = timeout(update_timeout, {
                        // Update all the chainheads of the network
                        // Also get a hash map returned on the subgraph mapped to network name and latest block
                        let indexing_status = match self.config.callbook().indexing_statuses().await {
                            Ok(res) => res,
                            Err(e) => {
                                error!(err = tracing::field::debug(&e), "Could not query indexing statuses for comparison, pull again later");
                                continue;
                            }
                        };
                        let network_chainhead_blocks = update_network_chainheads(
                                indexing_status,
                            );
                        let identifiers = self.graphcast_agent().content_identifiers();
                        let blocks_str = chainhead_block_str(&network_chainhead_blocks);

                        let comparison_res = self.compare_poi(
                            identifiers.clone(),
                        )
                        .await;

                        process_comparison_results(
                            blocks_str,
                            identifiers.len(),
                            comparison_res,
                            self.notifier.clone(),
                            self.db.clone()
                        )
                    }).await;

                    if result.is_err() {
                        warn!("compare_poi timed out");
                    } else {
                        debug!("compare_poi completed");
                    }
                },
                _ = notification_interval.tick() => {
                    match self.config.radio_setup.notification_mode {
                        NotificationMode::PeriodicReport => {
                            match get_comparison_results(&self.db.clone()).await {
                                Ok(comparison_results) => {
                                    if !comparison_results.is_empty() {
                                        let lines = {
                                            let (mut matching, mut divergent) = (0, 0);
                                            let mut lines = Vec::new();
                                            let total = comparison_results.len();

                                            let divergent_lines: Vec<String> = comparison_results.iter().filter_map(|res| {
                                                match res.result_type {
                                                    ComparisonResultType::Match => {
                                                        matching += 1;
                                                        None
                                                    },
                                                    ComparisonResultType::Divergent => {
                                                        divergent += 1;
                                                        Some(format!("{} - {}", res.deployment, res.block_number))
                                                    },
                                                    _ => None,
                                                }
                                            }).collect();

                                            lines.push(format!(
                                                "Total subgraphs being cross-checked: {}\nMatching: {}\nDivergent: {}, identifiers and blocks:",
                                                total, matching, divergent
                                            ));
                                            lines.extend(divergent_lines);
                                            lines
                                        };

                                        self.notifier.notify(lines.join("\n")).await;
                                    }
                                },
                                Err(e) => {
                                    error!("Error fetching comparison results: {:?}", e);
                                }
                            }
                        },
                        NotificationMode::PeriodicUpdate => {
                            match get_notifications(&self.db).await {
                                Ok(notifications) => {
                                    if !notifications.is_empty() {
                                        let notification_messages: Vec<String> = notifications
                                            .iter()
                                            .map(|n| format!("{}: {}", n.deployment, n.message))
                                            .collect();

                                        self.notifier.notify(notification_messages.join("\n")).await;

                                        if let Err(e) = clear_all_notifications(&self.db).await {
                                            error!("Error clearing notifications: {:?}", e);
                                        }
                                    }
                                },
                                Err(e) => {
                                    error!("Error fetching notifications: {:?}", e);
                                }
                            }
                        },
                        _ => {}
                    }
                },

                _ = cache_refresh_interval.tick() => {
                    tokio::spawn(async move {
                        let _ = fetch_and_cache_attestations(&self.db).await;
                    });
                }

                else => break,
            }

            sleep(Duration::from_secs(5)).await;
            continue;
        }
    }

    pub async fn message_processor(&self, receiver: Receiver<WakuMessage>) {
        let notifier = self.notifier.clone();
        let config = self.config.clone();
        let db = self.db.clone();

        tokio::spawn(async move {
            for msg in receiver {
                let timeout_duration = Duration::from_secs(10);

                let process_res = timeout(
                    timeout_duration,
                    process_message(notifier.clone(), config.clone(), msg, &db),
                )
                .await;

                match process_res {
                    Ok(_) => trace!("New message processed"),
                    Err(e) => debug!(error = e.to_string(), "Message processor timed out"),
                }
            }
        });
    }
}

/// Decode message into persistence, notifications, and other handlers
pub async fn process_message(
    notifier: Notifier,
    config: Config,
    msg: WakuMessage,
    db: &SqlitePool,
) {
    trace!("Decoding waku message into Graphcast Message with Radio specified payload");
    RECEIVED_MESSAGES.inc();
    let agent = GRAPHCAST_AGENT
        .get()
        .expect("Could not retrieve Graphcast agent");
    let id_validation = agent.id_validation.clone();
    let callbook = agent.callbook.clone();
    let nonces = agent.nonces.clone();
    let local_sender = agent.graphcast_identity.graphcast_id.clone();

    // handle each message based on their type
    match determine_message_type(msg) {
        TypedMessage::PublicPoi(msg) => {
            trace!(
                message = tracing::field::debug(&msg),
                "Handling a Public PoI message",
            );
            match check_message_validity(
                msg,
                &nonces,
                callbook.clone(),
                local_sender.clone(),
                &id_validation,
            )
            .await
            .map_err(|e| WakuHandlingError::InvalidMessage(e.to_string()))
            {
                Ok(msg) => {
                    if msg
                        .payload
                        .validity_check(&msg, &config.graph_stack.graph_node_status_endpoint)
                        .await
                        .is_ok()
                    {
                        VALIDATED_MESSAGES
                            .with_label_values(&[&msg.identifier, "public_poi_message"])
                            .inc();
                        process_valid_message(msg.clone(), db).await;
                    }
                }
                Err(e) => {
                    debug!(
                        err = tracing::field::debug(e),
                        "Failed to validate incoming message"
                    );
                }
            }
        }
        TypedMessage::UpgradeIntent(msg) => {
            trace!(
                message = tracing::field::debug(&msg),
                "Handling a Upgrade Intent message",
            );
            // Skip general first time sender nonce check and timestamp check
            let msg = match msg
                .valid_sender(
                    callbook.graphcast_registry(),
                    callbook.graph_network(),
                    local_sender.clone(),
                    &id_validation,
                )
                .await
                .map_err(|e| WakuHandlingError::InvalidMessage(e.to_string()))
            {
                Ok(msg) => msg,
                Err(e) => {
                    debug!(
                        err = tracing::field::debug(e),
                        "Failed to validate incoming message, sender address is invalid"
                    );
                    return;
                }
            };

            if let Ok(radio_msg) = msg
                .payload
                .validity_check(msg, &config.graph_stack.network_subgraph.clone())
                .await
            {
                VALIDATED_MESSAGES
                    .with_label_values(&[&msg.identifier, "upgrade_intent_message"])
                    .inc();
                if radio_msg
                    .process_valid_message(&config, &notifier, db)
                    .await
                    .is_ok()
                {
                    let _ = save_upgrade_intent_message(db, msg.clone()).await;
                };
            };
        }
        TypedMessage::Unsupported => {
            trace!("Waku message not decoded or validated, skipped message with unsupported type",)
        }
    }
}

/// Message types supported by the radio operator
pub enum TypedMessage {
    PublicPoi(GraphcastMessage<PublicPoiMessage>),
    UpgradeIntent(GraphcastMessage<UpgradeIntentMessage>),
    Unsupported,
}

/// Determine message type
pub fn determine_message_type(msg: WakuMessage) -> TypedMessage {
    if let Ok(ppoi_msg) = GraphcastMessage::<PublicPoiMessage>::decode(msg.payload()) {
        if ppoi_msg.payload.domain().is_ok()
            && ppoi_msg.payload.domain().unwrap().name == Some(String::from("PublicPoiMessage"))
        {
            TypedMessage::PublicPoi(ppoi_msg)
        } else if let Ok(msg) = GraphcastMessage::<UpgradeIntentMessage>::decode(msg.payload()) {
            if msg.payload.domain().is_ok()
                && msg.payload.domain().unwrap().name == Some(String::from("UpgradeIntentMessage"))
            {
                TypedMessage::UpgradeIntent(msg)
            } else {
                TypedMessage::Unsupported
            }
        } else {
            TypedMessage::Unsupported
        }
    } else if let Ok(msg) = GraphcastMessage::<UpgradeIntentMessage>::decode(msg.payload()) {
        if msg.payload.domain().is_ok()
            && msg.payload.domain().unwrap().name == Some(String::from("UpgradeIntentMessage"))
        {
            TypedMessage::UpgradeIntent(msg)
        } else {
            TypedMessage::Unsupported
        }
    } else {
        TypedMessage::Unsupported
    }
}
