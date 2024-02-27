use graphcast_sdk::build_wallet;

use std::{
    net::{TcpListener, UdpSocket},
    process::{Child, Command},
    sync::{Arc, Mutex},
    thread,
    time::Duration,
};

use config::TestSenderConfig;
use graphcast_sdk::{
    graphcast_agent::message_typing::{GraphcastMessage, IdentityValidation, RadioPayload},
    wallet_address,
};
use mock_server::{start_mock_server, ServerState};

use rand::Rng;
use subgraph_radio::{
    config::{Config, CoverageLevel},
    messages::poi::PublicPoiMessage,
};
use tracing::info;

pub mod config;
pub mod dummy_msg;
pub mod mock_server;

static GRAPHCAST_REGISTERED_ACCOUNTS: [&str; 10] = [
    "207c7ddacdde787e60e2b6eace5e5ec1eacffe91bbb27b9dea2c82ef0f8bfbef",
    "3e90105928bd7d1bf61f2cb50cd35deacedeaf8e0db3f2fd916cd06f4ae6fce0",
    "a9c6dd1badc6995dabbb5c5ff3e1bc2eaaadb0d45cab6e815ccbbbe964f29ac5",
    "07dfddcbcff0a6c1de00fea5ee6ce3aa4ca44b6efaaee439b037fb8a0dc4d0ce",
    "c07ad44bec2bcd92db3e46ffce242dee88ffeef4f6efdced3a6db958d6ddb9d4",
    "a5ccf4cc829dcdacfa04da5b34a43ef7fd7b95ad8f84ac8ab5ba2f2d7d7ba1d1",
    "5cce32210cfae096f06cc61a4dbb9485dcbf9b392ddfe016886ddeacaab2fa1d",
    "e9f1c7df9968aba583dc8e2b0ce7b3f6df34c73b3fce5bddd8dc92d4aec8293f",
    "76cc5c74ac6711f0bffa3ad0edbb0b41cad8ede5789f308edb4282cffbcab71d",
    "9ed8eed6459b90cc7739c9b926f5bb993cfcfe7f6bafb1c79b06160f3f701e3b",
];

static GRAPH_NETWORK_ACCOUNTS: [&str; 5] = [
    "cecbc2dc44bd2a756fbe03cea34d0ad46d0d0d0e17efb81a84a8d1bcfcbb5bf6",
    "443d671bfc255b582a11b8a923b032bb1d80fad19e4afb2af57eae2a5faa36e9",
    "b8c81ea0f8353b26df8984be161ed3ff38ef63b9ee13a87afae31190f62980c6",
    "79ca3b7a2b8d72b8be494a33b0efa4ea0843ccde652eddbe5d6cbb3edbf0bf11",
    "2685b72d4b1554e0f06f2afa4bfb9cb45be3cfcadbebfc343c5fda583dcfc29a",
];

static VALID_ETH_ADDRESSES: [&str; 5] = [
    "d809d520ddec2c0aea8add0b9c0932e084b3bb2ff22a3048ccf8b7baccc7cfe9",
    "afa418b0fcba7c9ef2fbddd9daedbf754630ecb23ebbcce24670cecb92302367",
    "6cee6aaa1e605f623b14f1c36afde96436fb515eb9df1f8bf04fed13e3b56cf3",
    "fdfabb5ebdd7aeecec15a50c1865a794bfaaee246732bd0ff6ca8386c4fec2ad",
    "952166e31ebae0b509b8fbc4d4aefd274ec1adb3cde2f7dcf6ddb85b3cb3f1ed",
];

static INDEXERS: [&str; 6] = [
    "aca0487f12fba5b38cafbc8dbcdee684f1baaa1ffe81001ad9ff2bbf99d45920",
    "31a4b96a3dd2cecea1a4eaa4ada1bbc43df462640ae012a3ff6a5c1adcceff81",
    "cea5386fc0fa3fe27a59bfad1fc88bbbcecca3accee517ed1b644bae9a52a12b",
    "05d5d18202eeddb4258c42f9e4bae9daa32a27dec0ce95be74ba01d6b7bddb60",
    "a5484b9c6bb10ba2dd05fcec371eb27fd0c2bf3b5daebfcda3078232af28bf5a",
    "233ef85e93a563454f5277c11afbf3593c0a1457c2ff8210b53e4579cfb273d6",
];

static REGISTERED_INDEXERS: [&str; 5] = [
    "a5ccf4cc829dcdacfa04da5b34a43ef7fd7b95ad8f84ac8ab5ba2f2d7d7ba1d1",
    "5cce32210cfae096f06cc61a4dbb9485dcbf9b392ddfe016886ddeacaab2fa1d",
    "e9f1c7df9968aba583dc8e2b0ce7b3f6df34c73b3fce5bddd8dc92d4aec8293f",
    "76cc5c74ac6711f0bffa3ad0edbb0b41cad8ede5789f308edb4282cffbcab71d",
    "9ed8eed6459b90cc7739c9b926f5bb993cfcfe7f6bafb1c79b06160f3f701e3b",
];

static SUBGRAPH_STAKERS: [&str; 5] = [
    "b445c65e2c90a2ea13f2252cb2adfd25eb2ee1ffceca590b623ec7d0ff3b8208",
    "f1642d082f5b59dfebe5ff07dcaf3fee2f0a95aa81ae12fd183bf3a9237fbb6a",
    "ba7ebda59cab88c2aaba2ab1d9af751b816fbdfc0b0b28ea3e5efccea6af0eda",
    "cc060128ced6f9dce30debe18f91d8c086d4aed57edfe6f1614f898afd2d73e5",
    "3c4fd7f41cdae6c5ad39e5fb192e5eae1a80161bcf6c5ae1e4b77eedefb97f01",
];

pub struct ProcessManager {
    pub senders: Vec<Arc<Mutex<Child>>>,
    pub radio: Arc<Mutex<Child>>,
    pub server_state: ServerState,
}

impl Drop for ProcessManager {
    fn drop(&mut self) {
        let _ = self.senders.first().unwrap().lock().unwrap().kill();
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

pub fn is_address_derived_from_keys(address: &str, keys: &[&str]) -> bool {
    keys.iter().any(|&private_key| {
        if let Ok(wallet) = build_wallet(private_key) {
            let derived_address = wallet_address(&wallet);
            derived_address == address
        } else {
            false
        }
    })
}

pub async fn setup(
    config: &Config,
    test_file_name: &str,
    test_sender_config: &mut TestSenderConfig,
) -> ProcessManager {
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
            .arg("--poi")
            .arg(&test_sender_config.poi.clone().unwrap_or(
                "0x25331f98b82ca7f3966256bf508a7ede52e715b631dfa3d73b846bb7617f6b9e".to_string(),
            ))
            .arg("--id-validation")
            .arg(
                test_sender_config
                    .id_validation
                    .as_ref()
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| "indexer".to_string()),
            )
            .spawn()
            .expect("Failed to start command"),
    ));

    let waku_port = find_random_udp_port();
    let discv5_port = find_random_udp_port();

    // TODO: Consider adding helper functions here
    let mut config = config.clone();

    let port = find_random_tcp_port();
    let host = format!("127.0.0.1:{}", port);
    let server_state = start_mock_server(host.clone(), config.radio_setup().topics.clone()).await;

    config.graph_stack.graph_node_status_endpoint = format!("http://{}/graphql", host);
    config.graph_stack.registry_subgraph = format!("http://{}/registry-subgraph", host);
    config.graph_stack.network_subgraph = format!("http://{}/network-subgraph", host);
    config.radio_setup.radio_name = radio_name;
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

pub fn teardown(process_manager: ProcessManager) {
    // Kill the processes
    for sender in &process_manager.senders {
        let _ = sender.lock().unwrap().kill();
    }
    let _ = process_manager.radio.lock().unwrap().kill();
}

pub fn start_radio(config: &Config) -> Child {
    Command::new("cargo")
        .arg("run")
        .arg("-p")
        .arg("subgraph-radio")
        .arg("--")
        .arg("--graph-node-status-endpoint")
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
        .arg(config.radio_setup().graphcast_network.to_string())
        .arg("--topics")
        .arg(config.radio_setup().topics.join(","))
        .arg("--gossip-topic-coverage")
        .arg(match config.radio_setup().gossip_topic_coverage {
            CoverageLevel::None => "none",
            CoverageLevel::Minimal => "minimal",
            CoverageLevel::OnChain => "on-chain",
            CoverageLevel::Comprehensive => "comprehensive",
        })
        .arg("--collect-message-duration")
        .arg(config.radio_setup().collect_message_duration.to_string())
        .arg("--waku-log-level")
        .arg(config.waku().waku_log_level.clone())
        .arg("--waku-port")
        .arg(config.waku().waku_port.as_deref().unwrap_or("None"))
        .arg("--log-level")
        .arg(&config.radio_setup().log_level)
        .arg("--slack-webhook")
        .arg(
            config
                .radio_setup()
                .slack_webhook
                .as_deref()
                .unwrap_or("None"),
        )
        .arg("--discord-webhook")
        .arg(
            config
                .radio_setup()
                .discord_webhook
                .as_deref()
                .unwrap_or("None"),
        )
        .arg("--sqlite-file-path")
        .arg(
            config
                .radio_setup()
                .sqlite_file_path
                .as_deref()
                .unwrap_or("None"),
        )
        .arg("--log-format")
        .arg(&config.radio_setup().log_format.to_string())
        .arg("--radio-name")
        .arg(&config.radio_setup().radio_name)
        .arg("--topic-update-interval")
        .arg(config.radio_setup().topic_update_interval.to_string())
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
        .arg(match config.radio_setup().id_validation {
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
    T: RadioPayload,
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

fn get_random_private_key(keys: &[&'static str]) -> String {
    let mut rng = rand::thread_rng();
    let random_index = rng.gen_range(0..keys.len());
    keys[random_index].to_string()
}

pub fn get_random_indexer() -> String {
    get_random_private_key(&INDEXERS)
}

pub fn get_random_graph_network_account() -> String {
    get_random_private_key(&GRAPH_NETWORK_ACCOUNTS)
}

pub fn get_random_valid_eth_address() -> String {
    get_random_private_key(&VALID_ETH_ADDRESSES)
}

pub fn get_random_registered_indexer() -> String {
    get_random_private_key(&REGISTERED_INDEXERS)
}

pub fn get_random_graphcast_registered_account() -> String {
    get_random_private_key(&GRAPHCAST_REGISTERED_ACCOUNTS)
}

pub fn get_random_subgraph_staker() -> String {
    get_random_private_key(&SUBGRAPH_STAKERS)
}
