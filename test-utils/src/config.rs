use clap::{ArgSettings, Parser};
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
    #[clap(long, value_name = "TOPICS", help = "Topics for test messages", setting = ArgSettings::UseValueDelimiter)]
    pub topics: Vec<String>,
    #[clap(long, value_name = "RADIO_NAME", help = "Instance-specific radio name")]
    pub radio_name: String,
    #[clap(long, value_name = "BLOCK_HASH")]
    pub block_hash: Option<String>,
    #[clap(long, value_name = "SENDER_TOKENS")]
    pub staked_tokens: Option<String>,
    #[clap(long, value_name = "NONCE")]
    pub nonce: Option<String>,
    #[clap(long, value_name = "RADIO_PAYLOAD")]
    pub radio_payload: Option<String>,
    #[clap(long, value_name = "POI")]
    pub poi: Option<String>,
}

pub fn test_config() -> Config {
    Config {
        graph_stack: {
            GraphStack {
                indexer_address: String::from("0x7e6528e4ce3055e829a32b5dc4450072bac28bc6"),
                graph_node_status_endpoint: String::new(),
                private_key: Some(
                    "ccaea3e3aca412cb3920dbecd77bc725dfe9a5e16f940f19912d9c9dbee01e8f".to_string(),
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
                boot_node_addresses: vec![],
                waku_log_level: "fatal".to_string(),
                discv5_enrs: Some(vec!["enr:-P-4QJI8tS1WTdIQxq_yIrD05oIIW1Xg-tm_qfP0CHfJGnp9dfr6ttQJmHwTNxGEl4Le8Q7YHcmi-kXTtphxFysS11oBgmlkgnY0gmlwhLymh5GKbXVsdGlhZGRyc7hgAC02KG5vZGUtMDEuZG8tYW1zMy53YWt1djIucHJvZC5zdGF0dXNpbS5uZXQGdl8ALzYobm9kZS0wMS5kby1hbXMzLndha3V2Mi5wcm9kLnN0YXR1c2ltLm5ldAYfQN4DiXNlY3AyNTZrMaEDbl1X_zJIw3EAJGtmHMVn4Z2xhpSoUaP5ElsHKCv7hlWDdGNwgnZfg3VkcIIjKIV3YWt1Mg8".to_string()]),
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
