use clap::{ArgSettings, Parser};
use graphcast_sdk::graphcast_agent::message_typing::IdentityValidation;
use serde::{Deserialize, Serialize};
use subgraph_radio::config::{Config, CoverageLevel};

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
        indexer_address: String::from("0x06f1e799A36f1Cb6aceAb09745dbf1920C7446D3"),
        graph_node_endpoint: String::new(),
        private_key: Some(
            "8c3a4ba32d5b30ca4f016772001b6b118a95f89b8a29e61276c0f48606952b5d".to_string(),
        ),
        mnemonic: None,
        registry_subgraph: String::new(),
        network_subgraph: String::new(),
        graphcast_network: "testnet".to_string(),
        topics: vec![],
        coverage: CoverageLevel::Comprehensive,
        collect_message_duration: 60,
        waku_host: None,
        waku_port: None,
        waku_node_key: None,
        waku_addr: None,
        boot_node_addresses: vec![],
        waku_log_level: None,
        log_level: "off,hyper=off,graphcast_sdk=trace,subgraph_radio=trace,test_runner=trace"
            .to_string(),
        slack_token: None,
        slack_channel: None,
        discord_webhook: None,
        metrics_host: String::new(),
        metrics_port: None,
        server_host: String::new(),
        server_port: None,
        persistence_file_path: None,
        log_format: "pretty".to_string(),
        radio_name: String::new(),
        telegram_chat_id: None,
        telegram_token: None,
        discv5_enrs: None,
        discv5_port: None,
        filter_protocol: None,
        id_validation: IdentityValidation::ValidAddress,
        topic_update_interval: 600,
    }
}
