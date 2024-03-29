use autometrics::{encode_global_metrics, global_metrics_exporter};
use axum::http::StatusCode;
use axum::routing::get;
use axum::Router;
use axum_server::Handle;
use once_cell::sync::Lazy;
use prometheus::{core::Collector, Registry};
use prometheus::{Gauge, GaugeVec, IntCounter, IntCounterVec, IntGauge, IntGaugeVec, Opts};

use std::{net::SocketAddr, str::FromStr};
use tracing::{debug, info};

// Received (and validated) messages counter
#[allow(dead_code)]
pub static VALIDATED_MESSAGES: Lazy<IntCounterVec> = Lazy::new(|| {
    let m = IntCounterVec::new(
        Opts::new("validated_messages", "Number of validated messages")
            .namespace("graphcast")
            .subsystem("subgraph_radio"),
        &["deployment", "message_type"],
    )
    .expect("Failed to create validated_messages counters");
    prometheus::register(Box::new(m.clone()))
        .expect("Failed to register validated_messages counter");
    m
});

// Received (and validated) messages counter
#[allow(dead_code)]
pub static CACHED_PPOI_MESSAGES: Lazy<IntGaugeVec> = Lazy::new(|| {
    let m = IntGaugeVec::new(
        Opts::new("cached_ppoi_messages", "Number of messages in cache")
            .namespace("graphcast")
            .subsystem("subgraph_radio"),
        &["deployment"],
    )
    .expect("Failed to create cached_ppoi_messages gauges");
    prometheus::register(Box::new(m.clone()))
        .expect("Failed to register cached_ppoi_messages guage");
    m
});

// These are the subgraphs that are being actively cross-checked (the ones we are receiving remote attestations for)
#[allow(dead_code)]
pub static ACTIVE_INDEXERS: Lazy<IntGaugeVec> = Lazy::new(|| {
    let m = IntGaugeVec::new(
        Opts::new(
            "active_indexers",
            "Number of indexers actively crosschecking on the deployment (self excluded)",
        )
        .namespace("graphcast")
        .subsystem("subgraph_radio"),
        &["deployment"],
    )
    .expect("Failed to create active_indexers gauge");
    prometheus::register(Box::new(m.clone())).expect("Failed to register ACTIVE_INDEXERS counter");
    m
});

#[allow(dead_code)]
pub static DIVERGING_SUBGRAPHS: Lazy<IntGauge> = Lazy::new(|| {
    let m = IntGauge::with_opts(
        Opts::new(
            "diverging_subgraphs",
            "Number of diverging subgraphs with non-consensus POIs from cross-checking",
        )
        .namespace("graphcast")
        .subsystem("subgraph_radio"),
    )
    .expect("Failed to create diverging_subgraphs gauge");
    prometheus::register(Box::new(m.clone()))
        .expect("Failed to register diverging_subgraphs gauge");
    m
});

#[allow(dead_code)]
pub static CONNECTED_PEERS: Lazy<IntGauge> = Lazy::new(|| {
    let m = IntGauge::with_opts(
        Opts::new(
            "connected_peers",
            "Number of Gossip peers connected with Graphcast agent",
        )
        .namespace("graphcast")
        .subsystem("subgraph_radio"),
    )
    .expect("Failed to create connected_peers gauge");
    prometheus::register(Box::new(m.clone())).expect("Failed to register connected_peers gauge");
    m
});

#[allow(dead_code)]
pub static GOSSIP_PEERS: Lazy<IntGauge> = Lazy::new(|| {
    let m = IntGauge::with_opts(
        Opts::new("gossip_peers", "Total number of gossip peers discovered")
            .namespace("graphcast")
            .subsystem("subgraph_radio"),
    )
    .expect("Failed to create gossip_peers gauge");
    prometheus::register(Box::new(m.clone())).expect("Failed to register gossip_peers gauge");
    m
});

#[allow(dead_code)]
pub static RECEIVED_MESSAGES: Lazy<IntCounter> = Lazy::new(|| {
    let m = IntCounter::with_opts(
        Opts::new("received_messages", "Number of messages received in total")
            .namespace("graphcast")
            .subsystem("subgraph_radio"),
    )
    .expect("Failed to create received_messages counter");
    prometheus::register(Box::new(m.clone()))
        .expect("Failed to register received_messages counter");
    m
});

#[allow(dead_code)]
pub static AVERAGE_PROCESSING_TIME: Lazy<Gauge> = Lazy::new(|| {
    let m = Gauge::with_opts(
        Opts::new(
            "average_processing_time",
            "Average time taken to process each POI message",
        )
        .namespace("graphcast")
        .subsystem("subgraph_radio"),
    )
    .expect("Failed to create average_processing_time gauge");
    prometheus::register(Box::new(m.clone()))
        .expect("Failed to register average_processing_time gauge");
    m
});

#[allow(dead_code)]
pub static FREQUENT_SENDERS_COUNTER: Lazy<IntCounterVec> = Lazy::new(|| {
    let m = IntCounterVec::new(
        Opts::new(
            "frequent_senders",
            "Count of messages received from each indexer",
        )
        .namespace("graphcast")
        .subsystem("subgraph_radio"),
        &["indexer_address"],
    )
    .expect("Failed to create frequent_senders counter");
    prometheus::register(Box::new(m.clone())).expect("Failed to register frequent_senders counter");
    m
});

#[allow(dead_code)]
pub static LATEST_MESSAGE_TIMESTAMP: Lazy<GaugeVec> = Lazy::new(|| {
    let m = GaugeVec::new(
        Opts::new(
            "latest_message_timestamp",
            "Timestamp of the last received public POI message",
        )
        .namespace("graphcast")
        .subsystem("subgraph_radio"),
        &["deployment_hash"],
    )
    .expect("Failed to create latest_message_timestamp gauge");
    prometheus::register(Box::new(m.clone()))
        .expect("Failed to register latest_message_timestamp gauge");
    m
});

#[allow(dead_code)]
pub static ATTESTED_MAX_STAKE_WEIGHT: Lazy<GaugeVec> = Lazy::new(|| {
    let m = GaugeVec::new(
        Opts::new("attested_max_stake_weight", "Highest stake-backed POI")
            .namespace("graphcast")
            .subsystem("subgraph_radio"),
        &["deployment_hash"],
    )
    .expect("Failed to create attested_max_stake_weight gauge");
    prometheus::register(Box::new(m.clone())).expect("Failed to register max_poi_stake gauge");
    m
});

#[allow(dead_code)]
pub static NUM_TOPICS: Lazy<IntGauge> = Lazy::new(|| {
    let m = IntGauge::with_opts(
        Opts::new(
            "num_topics",
            "Number of topics (subgraphs) being actively cross-checked",
        )
        .namespace("graphcast")
        .subsystem("subgraph_radio"),
    )
    .expect("Failed to create num_topics gauge");
    prometheus::register(Box::new(m.clone())).expect("Failed to register num_topics gauge");
    m
});

#[allow(dead_code)]
pub static COMPARISON_RESULTS: Lazy<IntGaugeVec> = Lazy::new(|| {
    let m = IntGaugeVec::new(
        Opts::new(
            "comparison_results",
            "Rate of comparison results for each deployment",
        )
        .namespace("graphcast")
        .subsystem("subgraph_radio"),
        &["deployment"],
    )
    .expect("Failed to create comparison_results gauge");
    prometheus::register(Box::new(m.clone())).expect("Failed to register COMPARISON_RESULTS gauge");
    m
});

#[allow(dead_code)]
pub static REGISTRY: Lazy<prometheus::Registry> = Lazy::new(prometheus::Registry::new);

#[allow(dead_code)]
pub fn register_metrics(registry: &Registry, metrics: Vec<Box<dyn Collector>>) {
    for metric in metrics {
        registry.register(metric).expect("Cannot register metrics");
        debug!("registered metric");
    }
}

#[allow(dead_code)]
pub fn start_metrics() {
    register_metrics(
        &REGISTRY,
        vec![
            Box::new(VALIDATED_MESSAGES.clone()),
            Box::new(CACHED_PPOI_MESSAGES.clone()),
            Box::new(ACTIVE_INDEXERS.clone()),
            Box::new(DIVERGING_SUBGRAPHS.clone()),
            Box::new(CONNECTED_PEERS.clone()),
            Box::new(GOSSIP_PEERS.clone()),
            Box::new(RECEIVED_MESSAGES.clone()),
            Box::new(AVERAGE_PROCESSING_TIME.clone()),
            Box::new(FREQUENT_SENDERS_COUNTER.clone()),
            Box::new(LATEST_MESSAGE_TIMESTAMP.clone()),
            Box::new(ATTESTED_MAX_STAKE_WEIGHT.clone()),
            Box::new(NUM_TOPICS.clone()),
            Box::new(COMPARISON_RESULTS.clone()),
        ],
    );
}

/// This handler serializes the metrics into a string for Prometheus to scrape
#[allow(dead_code)]
pub async fn get_metrics() -> (StatusCode, String) {
    match encode_global_metrics() {
        Ok(metrics) => (StatusCode::OK, metrics),
        Err(err) => (StatusCode::INTERNAL_SERVER_ERROR, format!("{err:?}")),
    }
}

/// Run the API server as well as Prometheus and a traffic generator
#[allow(dead_code)]
pub async fn handle_serve_metrics(host: String, port: u16, handle: Handle) {
    // Set up the exporter to collect metrics
    let _exporter = global_metrics_exporter();

    let app = Router::new().route("/metrics", get(get_metrics));
    let addr =
        SocketAddr::from_str(&format!("{}:{}", host, port)).expect("Start Prometheus metrics");
    // let server = axum::Server::bind(&addr);
    info!(
        address = addr.to_string(),
        "Prometheus Metrics port exposed"
    );

    axum_server::bind(addr)
        .handle(handle)
        .serve(app.into_make_service())
        .await
        .expect("Error starting Prometheus metrics service")
}
