use derive_getters::Getters;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc::Receiver,
    Arc,
};
use std::time::Duration;
use tokio::time::{interval, sleep, timeout};
use tracing::{debug, error, info, trace, warn};

use crate::{
    chainhead_block_str, messages::poi::process_valid_message,
    operator::indexer_management::health_query,
};
use crate::{messages::poi::PublicPoiMessage, metrics::VALIDATED_MESSAGES};
use graphcast_sdk::{
    graphcast_agent::{
        message_typing::check_message_validity, waku_handling::WakuHandlingError, GraphcastAgent,
    },
    graphql::client_graph_node::{subgraph_network_blocks, update_network_chainheads},
    WakuMessage,
};

use crate::config::Config;
use crate::messages::upgrade::UpgradeIntentMessage;
use crate::metrics::handle_serve_metrics;
use crate::operator::attestation::log_gossip_summary;
use crate::operator::attestation::process_comparison_results;
use crate::server::run_server;
use crate::state::PersistedState;
use crate::GRAPHCAST_AGENT;

use self::notifier::Notifier;

pub mod attestation;
pub mod callbook;
pub mod indexer_management;
pub mod notifier;
pub mod operation;

/// Aggregated control flow configurations
/// Not used currently
#[derive(Getters)]
#[allow(unused)]
struct ControlFlow {
    running: Arc<AtomicBool>,
    skip_iteration: Arc<AtomicBool>,
    iteration_timeout: Duration,
    update_timeout: Duration,
    gossip_timeout: Duration,
    topic_update_duration: Duration,
    state_update_duration: Duration,
    gossip_poi_duration: Duration,
    comparison_duration: Duration,
}

impl ControlFlow {
    fn new() -> Self {
        let running = Arc::new(AtomicBool::new(true));
        let skip_iteration = Arc::new(AtomicBool::new(false));

        let topic_update_duration = Duration::from_secs(600);
        let state_update_duration = Duration::from_secs(15);
        let gossip_poi_duration = Duration::from_secs(30);
        let comparison_duration = Duration::from_secs(60);

        let iteration_timeout = Duration::from_secs(180);
        let update_timeout = Duration::from_secs(10);
        let gossip_timeout = Duration::from_secs(150);

        ControlFlow {
            running,
            skip_iteration,
            iteration_timeout,
            update_timeout,
            gossip_timeout,
            topic_update_duration,
            state_update_duration,
            gossip_poi_duration,
            comparison_duration,
        }
    }
}

/// Radio operator contains all states needed for radio operations
#[allow(unused)]
pub struct RadioOperator {
    config: Config,
    persisted_state: PersistedState,
    graphcast_agent: Arc<GraphcastAgent>,
    notifier: Notifier,
    control_flow: ControlFlow,
}

impl RadioOperator {
    /// Create a radio operator with radio configurations, persisted data,
    /// graphcast agent, and control flow
    pub async fn new(config: &Config, agent: GraphcastAgent) -> RadioOperator {
        debug!("Initializing program state");
        // Initialize program state
        let persisted_state: PersistedState = config.init_radio_state().await;

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

        GRAPHCAST_AGENT
            .get()
            .unwrap()
            .register_handler()
            .expect("Could not register handler");

        RadioOperator {
            config: config.clone(),
            persisted_state,
            graphcast_agent,
            notifier,
            control_flow: ControlFlow::new(),
        }
    }

    /// Preparation for running the radio applications
    /// Expose metrics and subscribe to graphcast topics
    pub async fn prepare(&self, receiver: Receiver<WakuMessage>) {
        // Set up Prometheus metrics url if configured
        if let Some(port) = self.config.radio_infrastructure().metrics_port {
            debug!("Initializing metrics port");
            tokio::spawn(handle_serve_metrics(
                self.config.radio_infrastructure().metrics_host.clone(),
                port,
                self.control_flow.running.clone(),
            ));
        }

        // Provide generated topics to Graphcast agent
        let topics = self
            .config
            .generate_topics(
                &self.config.radio_infrastructure().coverage,
                &self.config.graph_stack.indexer_address,
            )
            .await;
        debug!(
            topics = tracing::field::debug(&topics),
            "Found content topics for subscription",
        );
        self.graphcast_agent
            .update_content_topics(topics.clone())
            .await;

        self.message_processor(receiver).await;
    }

    pub fn graphcast_agent(&self) -> &GraphcastAgent {
        &self.graphcast_agent
    }

    /// Read persisted state at the time of access
    pub fn state(&self) -> PersistedState {
        self.persisted_state.clone()
    }

    /// Radio operations
    pub async fn run(&'static self) {
        // Control flow
        // TODO: expose to radio config for the users
        let running = Arc::new(AtomicBool::new(true));
        let skip_iteration = Arc::new(AtomicBool::new(false));
        let skip_iteration_clone = skip_iteration.clone();

        let mut topic_update_interval = interval(Duration::from_secs(
            self.config.radio_infrastructure.topic_update_interval,
        ));

        let mut state_update_interval = interval(Duration::from_secs(60));
        let mut gossip_poi_interval = interval(Duration::from_secs(30));
        let mut comparison_interval = interval(Duration::from_secs(30));

        let iteration_timeout = Duration::from_secs(180);
        let update_timeout = Duration::from_secs(5);
        let gossip_timeout = Duration::from_secs(120);

        // Separate thread to skip a main loop iteration when hit timeout
        tokio::spawn(async move {
            tokio::time::sleep(iteration_timeout).await;
            skip_iteration_clone.store(true, Ordering::SeqCst);
        });

        // Initialize Http server with graceful shutdown if configured
        if self.config.radio_infrastructure().server_port.is_some() {
            let state_ref = &self.persisted_state;
            let config_cloned = self.config.clone();
            tokio::spawn(run_server(config_cloned, state_ref, running.clone()));
        }

        // Main loop for sending messages, can factor out
        // and take radio specific query and parsing for radioPayload
        while running.load(Ordering::SeqCst) {
            // Run event intervals sequentially by satisfication of other intervals and corresponding tick
            tokio::select! {
                _ = topic_update_interval.tick() => {
                    if skip_iteration.load(Ordering::SeqCst) {
                        skip_iteration.store(false, Ordering::SeqCst);
                        continue;
                    }
                    // Update topic subscription
                    let result = timeout(update_timeout,
                        self.graphcast_agent()
                        .update_content_topics(self.config.generate_topics(
                            &self.config.radio_infrastructure().coverage,
                            &self.config.graph_stack().indexer_address).await)
                    ).await;

                    if result.is_err() {
                        warn!("update_content_topics timed out");
                    } else {
                        debug!("update_content_topics completed");
                    }
                },
                _ = state_update_interval.tick() => {
                    if skip_iteration.load(Ordering::SeqCst) {
                        skip_iteration.store(false, Ordering::SeqCst);
                        continue;
                    }

                    // Save cache if path provided
                    let _ = &self.config.radio_infrastructure().persistence_file_path.as_ref().map(|path| {
                        self.persisted_state.update_cache(path);
                    });
                },
                _ = gossip_poi_interval.tick() => {
                    if skip_iteration.load(Ordering::SeqCst) {
                        skip_iteration.store(false, Ordering::SeqCst);
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
                        let identifiers = self.graphcast_agent.content_identifiers().await;
                        let num_topics = identifiers.len();
                        let blocks_str = chainhead_block_str(&network_chainhead_blocks);
                        info!(
                            chainhead = blocks_str.clone(),
                            num_gossip_peers = self.graphcast_agent.number_of_peers(),
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
                    if skip_iteration.load(Ordering::SeqCst) {
                        skip_iteration.store(false, Ordering::SeqCst);
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
                        let identifiers = self.graphcast_agent().content_identifiers().await;
                        let blocks_str = chainhead_block_str(&network_chainhead_blocks);

                        trace!(
                            state = tracing::field::debug(&self.state()),
                            "current state",
                        );

                        let comparison_res = self.compare_poi(
                            identifiers.clone(),
                        )
                        .await;

                        process_comparison_results(
                            blocks_str,
                            identifiers.len(),
                            comparison_res,
                            self.notifier.clone(),
                            self.persisted_state.clone()
                        )
                    }).await;

                    if result.is_err() {
                        warn!("compare_poi timed out");
                    } else {
                        debug!("compare_poi completed");
                    }
                },
                else => break,
            }

            sleep(Duration::from_secs(5)).await;
            continue;
        }
    }

    /// Process messages
    pub async fn message_processor(&self, receiver: Receiver<WakuMessage>) {
        let state = self.persisted_state.clone();
        let notifier = self.notifier.clone();
        let config = self.config.clone();

        tokio::spawn(async move {
            for msg in receiver {
                let timeout_duration = Duration::from_secs(1);
                let process_res = timeout(
                    timeout_duration,
                    process_message(state.clone(), notifier.clone(), config.clone(), msg),
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
    state: PersistedState,
    notifier: Notifier,
    config: Config,
    msg: WakuMessage,
) {
    trace!("Decoding waku message into Graphcast Message with Radio specified payload");
    let agent = GRAPHCAST_AGENT
        .get()
        .expect("Could not retrieve Graphcast agent");
    let id_validation = agent.id_validation.clone();
    let callbook = agent.callbook.clone();
    let nonces = agent.nonces.clone();
    let local_sender = agent.graphcast_identity.graphcast_id.clone();
    let parsed = if let Ok(msg) = agent.decode::<PublicPoiMessage>(msg.payload()).await {
        trace!(
            message = tracing::field::debug(&msg),
            "Parseable as Public PoI message, now validate",
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
                let is_valid = msg
                    .payload
                    .validity_check(&msg, &config.graph_stack.graph_node_status_endpoint)
                    .await
                    .is_ok();

                if is_valid {
                    VALIDATED_MESSAGES
                        .with_label_values(&[&msg.identifier, "public_poi_message"])
                        .inc();
                    process_valid_message(msg.clone(), &state).await;
                };
                is_valid
            }
            Err(e) => {
                debug!(
                    err = tracing::field::debug(e),
                    "Failed to validate by Graphcast"
                );
                false
            }
        }
    } else {
        false
    };

    if !parsed {
        if let Ok(msg) = agent.decode::<UpgradeIntentMessage>(msg.payload()).await {
            trace!(
                message = tracing::field::debug(&msg),
                "Parseable as Upgrade Intent message, now validate",
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
                        "Failed to validate Graphcast sender"
                    );
                    return;
                }
            };
            let is_valid = msg
                .payload
                .validity_check(msg, &config.graph_stack.network_subgraph.clone())
                .await;

            if let Ok(radio_msg) = is_valid {
                VALIDATED_MESSAGES
                    .with_label_values(&[&msg.identifier, "upgrade_intent_message"])
                    .inc();
                radio_msg.process_valid_message(&config, &notifier).await;
            };
        } else {
            trace!("Waku message not decoded or validated, skipped message",);
        };
    }
}
