use clap::Parser;
use graphcast_sdk::graphcast_agent::WAKU_DISCOVERY_ENR;
use graphcast_sdk::{
    graphcast_agent::message_typing::IdentityValidation, GraphcastNetworkName, LogFormat,
};
use serde::{Deserialize, Serialize};
use subgraph_radio::config::{Config, CoverageLevel, GraphStack, RadioSetup, Waku};
use subgraph_radio::operator::notifier::NotificationMode;

use crate::find_random_udp_port;

#[derive(Clone, Debug, Parser, Serialize, Deserialize)]
#[clap(name = "test-sender", about = "Mock message sender")]
pub struct TestSenderConfig {
    #[clap(
        long,
        value_name = "TOPICS",
        help = "Topics for test messages",
        use_value_delimiter = true
    )]
    pub topics: Vec<String>,
    #[clap(long, value_name = "RADIO_NAME", help = "Instance-specific radio name")]
    pub radio_name: String,
    #[clap(long, value_name = "BLOCK_HASH")]
    pub block_hash: Option<String>,
    #[clap(long, value_name = "NONCE")]
    pub nonce: Option<String>,
    #[clap(long, value_name = "POI")]
    pub poi: Option<String>,
    #[clap(long, value_name = "ID_VALIDATION", value_enum)]
    pub id_validation: Option<IdentityValidation>,
}

pub fn test_config() -> Config {
    Config {
        graph_stack: {
            GraphStack {
                indexer_address: String::from("0x8c63ab7d3086b4a7ce1841a716564bf9318d9db7"),
                graph_node_status_endpoint: String::new(),
                private_key: Some(
                    "233ef85e93a563454f5277c11afbf3593c0a1457c2ff8210b53e4579cfb273d6".to_string(),
                ),
                mnemonic: None,
                registry_subgraph: String::new(),
                network_subgraph: String::new(),
                indexer_management_server_endpoint: None,
                protocol_network: None,
            }
        },
        waku: {
            Waku {
                waku_host: None,
                waku_port: None,
                waku_node_key: None,
                waku_addr: None,
                boot_node_addresses: vec!["/dns4/graph.myrandomdemos.online/tcp/31900/p2p/16Uiu2HAm5oH7pmpoTQoS5PgdehCinnrtpPAUhBuajG6AG3XHrR2P".to_string(), "/dns4/graph.myrandomdemos.online/tcp/8000/wss/p2p/16Uiu2HAm5oH7pmpoTQoS5PgdehCinnrtpPAUhBuajG6AG3XHrR2P".to_string()],
                waku_log_level: "fatal".to_string(),
                discv5_enrs: Some(vec![WAKU_DISCOVERY_ENR.to_string()]),
                discv5_port: Some(find_random_udp_port()),
                filter_protocol: None,
            }
        },
        radio_setup: {
            RadioSetup {
                graphcast_network: GraphcastNetworkName::Testnet,
                topics: vec![],
                gossip_topic_coverage: CoverageLevel::OnChain,
                auto_upgrade_ratelimit: 60000,
                collect_message_duration: 60,
                log_level:
                    "off,hyper=off,graphcast_sdk=trace,subgraph_radio=trace,test_runner=trace"
                        .to_string(),
                slack_webhook: None,
                discord_webhook: None,
                metrics_host: String::new(),
                metrics_port: None,
                server_host: String::new(),
                server_port: None,
                log_format: LogFormat::Pretty,
                radio_name: String::new(),
                telegram_chat_id: None,
                telegram_token: None,
                id_validation: IdentityValidation::ValidAddress,
                topic_update_interval: 600,
                auto_upgrade_coverage: CoverageLevel::OnChain,
                notification_mode: NotificationMode::Live,
                notification_interval: 24,
                sqlite_file_path: None,
            }
        },
        config_file: None,
    }
}
