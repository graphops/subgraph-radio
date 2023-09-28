use std::{
    fs,
    net::{TcpListener, UdpSocket},
    path::Path,
    process::{Child, Command},
    sync::{Arc, Mutex},
    thread,
    time::Duration,
};

use config::TestSenderConfig;
use graphcast_sdk::graphcast_agent::message_typing::GraphcastMessage;
use graphcast_sdk::graphcast_agent::message_typing::IdentityValidation;
use mock_server::{start_mock_server, ServerState};
use prost::Message;
use rand::Rng;
use subgraph_radio::{
    config::{Config, CoverageLevel},
    messages::poi::PublicPoiMessage,
};
use tracing::info;

pub mod config;
pub mod dummy_msg;
pub mod mock_server;

pub struct ProcessManager {
    pub senders: Vec<Arc<Mutex<Child>>>,
    pub radio: Arc<Mutex<Child>>,
    pub server_state: ServerState,
}

impl Drop for ProcessManager {
    fn drop(&mut self) {
        let _ = self.senders.get(0).unwrap().lock().unwrap().kill();
        let _ = self.radio.lock().unwrap().kill();
    }
}

fn find_random_tcp_port() -> u16 {
    let mut rng = rand::thread_rng();
    let mut port = 0;

    for _ in 0..10 {
        // Generate a random port number within the range 49152 to 65535
        let test_port = rng.gen_range(49152..=65535);
        match TcpListener::bind(("0.0.0.0", test_port)) {
            Ok(_) => {
                port = test_port;
                break;
            }
            Err(_) => {
                println!("Port {} is not available, retrying...", test_port);
                thread::sleep(Duration::from_secs(1));
                continue;
            }
        }
    }

    if port == 0 {
        panic!("Could not find a free port");
    }

    port
}

pub async fn setup(
    config: &Config,
    test_file_name: &str,
    test_sender_config: &mut TestSenderConfig,
) -> ProcessManager {
    if let Some(file_path) = &config.radio_infrastructure().persistence_file_path {
        let path = Path::new(file_path);
        if path.exists() {
            fs::remove_file(path).expect("Failed to remove file");
        }
    }

    let id = uuid::Uuid::new_v4().to_string();
    let radio_name = format!("{}-{}", test_file_name, id);
    test_sender_config.radio_name = radio_name.clone();

    let basic_sender = Arc::new(Mutex::new(
        Command::new("cargo")
            .arg("run")
            .arg("-p")
            .arg("test-sender")
            .arg("--")
            .arg("--topics")
            .arg(&test_sender_config.topics.join(","))
            .arg("--radio-name")
            .arg(&test_sender_config.radio_name)
            .arg("--block-hash")
            .arg(&test_sender_config.block_hash.clone().unwrap_or(
                "4dbba1ba9fb18b0034965712598be1368edcf91ae2c551d59462aab578dab9c5".to_string(),
            ))
            .arg("--nonce")
            .arg(test_sender_config.nonce.as_deref().unwrap_or("None"))
            .arg("--radio-payload")
            .arg(
                test_sender_config
                    .radio_payload
                    .clone()
                    .unwrap_or("radio_payload_message".to_string()),
            )
            .arg("--poi")
            .arg(&test_sender_config.poi.clone().unwrap_or(
                "0x25331f98b82ca7f3966256bf508a7ede52e715b631dfa3d73b846bb7617f6b9e".to_string(),
            ))
            .spawn()
            .expect("Failed to start command"),
    ));

    let waku_port = find_random_udp_port();
    let discv5_port = find_random_udp_port();

    // TODO: Consider adding helper functions here
    let mut config = config.clone();

    let port = find_random_tcp_port();
    let host = format!("127.0.0.1:{}", port);
    let server_state = start_mock_server(
        host.clone(),
        config.radio_infrastructure().topics.clone(),
        test_sender_config.staked_tokens.clone(),
    )
    .await;

    config.graph_stack.graph_node_status_endpoint = format!("http://{}/graphql", host);
    config.graph_stack.registry_subgraph = format!("http://{}/registry-subgraph", host);
    config.graph_stack.network_subgraph = format!("http://{}/network-subgraph", host);
    config.radio_infrastructure.radio_name = radio_name;
    config.waku.waku_port = Some(waku_port.to_string());
    config.waku.discv5_port = Some(discv5_port);

    info!(
        "Starting Subgraph Radio instance on port {}",
        waku_port.to_string()
    );

    let radio = Arc::new(Mutex::new(start_radio(&config)));

    ProcessManager {
        senders: vec![Arc::clone(&basic_sender)],
        radio: Arc::clone(&radio),
        server_state,
    }
}

pub fn teardown(process_manager: ProcessManager, store_path: &str) {
    // Kill the processes
    for sender in &process_manager.senders {
        let _ = sender.lock().unwrap().kill();
    }
    let _ = process_manager.radio.lock().unwrap().kill();

    if Path::new(&store_path).exists() {
        fs::remove_file(store_path).unwrap();
    }
}

pub fn start_radio(config: &Config) -> Child {
    Command::new("cargo")
        .arg("run")
        .arg("-p")
        .arg("subgraph-radio")
        .arg("--")
        .arg("--graph-node-endpoint")
        .arg(&config.graph_stack().graph_node_status_endpoint)
        .arg("--private-key")
        .arg(
            config
                .graph_stack()
                .private_key
                .as_deref()
                .unwrap_or("None"),
        )
        .arg("--registry-subgraph")
        .arg(&config.graph_stack().registry_subgraph)
        .arg("--network-subgraph")
        .arg(&config.graph_stack().network_subgraph)
        .arg("--graphcast-network")
        .arg(config.radio_infrastructure().graphcast_network.to_string())
        .arg("--topics")
        .arg(config.radio_infrastructure().topics.join(","))
        .arg("--coverage")
        .arg(match config.radio_infrastructure().coverage {
            CoverageLevel::None => "none",
            CoverageLevel::Minimal => "minimal",
            CoverageLevel::OnChain => "on-chain",
            CoverageLevel::Comprehensive => "comprehensive",
        })
        .arg("--collect-message-duration")
        .arg(
            config
                .radio_infrastructure()
                .collect_message_duration
                .to_string(),
        )
        .arg("--waku-log-level")
        .arg(config.waku().waku_log_level.clone())
        .arg("--waku-port")
        .arg(config.waku().waku_port.as_deref().unwrap_or("None"))
        .arg("--log-level")
        .arg(&config.radio_infrastructure().log_level)
        .arg("--slack-token")
        .arg(
            config
                .radio_infrastructure()
                .slack_token
                .as_deref()
                .unwrap_or("None"),
        )
        .arg("--slack-channel")
        .arg(
            config
                .radio_infrastructure()
                .slack_channel
                .as_deref()
                .unwrap_or("None"),
        )
        .arg("--discord-webhook")
        .arg(
            config
                .radio_infrastructure()
                .discord_webhook
                .as_deref()
                .unwrap_or("None"),
        )
        .arg("--persistence-file-path")
        .arg(
            config
                .radio_infrastructure()
                .persistence_file_path
                .as_deref()
                .unwrap_or("None"),
        )
        .arg("--log-format")
        .arg(&config.radio_infrastructure().log_format.to_string())
        .arg("--radio-name")
        .arg(&config.radio_infrastructure().radio_name)
        .arg("--topic-update-interval")
        .arg(
            config
                .radio_infrastructure()
                .topic_update_interval
                .to_string(),
        )
        .arg("--discv5-port")
        .arg(
            config
                .waku()
                .discv5_port
                .map(|p| p.to_string())
                .unwrap_or_else(|| "None".to_string()),
        )
        .arg("--indexer-address")
        .arg(&config.graph_stack().indexer_address)
        .arg("--id-validation")
        .arg(match config.radio_infrastructure().id_validation {
            IdentityValidation::NoCheck => "no-check",
            IdentityValidation::ValidAddress => "valid-address",
            IdentityValidation::GraphcastRegistered => "graphcast-registered",
            IdentityValidation::GraphNetworkAccount => "graph-network-account",
            IdentityValidation::RegisteredIndexer => "registered-indexer",
            IdentityValidation::Indexer => "indexer",
            IdentityValidation::SubgraphStaker => "subgraph-staker",
        })
        .spawn()
        .expect("Failed to start command")
}

pub fn find_random_udp_port() -> u16 {
    let mut rng = rand::thread_rng();
    let mut port = 0;

    for _ in 0..10 {
        // Generate a random port number within the range 49152 to 65535
        let test_port = rng.gen_range(49152..=65535);
        match UdpSocket::bind(("0.0.0.0", test_port)) {
            Ok(_) => {
                port = test_port;
                break;
            }
            Err(_) => continue,
        }
    }

    if port == 0 {
        panic!("Could not find a free port");
    }

    port
}

pub fn messages_are_equal<T>(msg1: &GraphcastMessage<T>, msg2: &GraphcastMessage<T>) -> bool
where
    T: Message
        + ethers::types::transaction::eip712::Eip712
        + Default
        + Clone
        + 'static
        + async_graphql::OutputType,
{
    msg1.identifier == msg2.identifier
        && msg1.nonce == msg2.nonce
        && msg1.signature == msg2.signature
}

pub fn payloads_are_equal(payload1: &PublicPoiMessage, payload2: &PublicPoiMessage) -> bool {
    payload1.identifier == payload2.identifier
        && payload1.content == payload2.content
        && payload1.network == payload2.network
        && payload1.block_number == payload2.block_number
        && payload1.block_hash == payload2.block_hash
}
