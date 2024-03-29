use autometrics::autometrics;
use clap::value_parser;
use clap::{command, Args, Parser};
use derive_getters::Getters;
use ethers::signers::WalletError;
use graphcast_sdk::{
    build_wallet,
    callbook::CallBook,
    graphcast_agent::{
        message_typing::IdentityValidation, GraphcastAgentConfig, GraphcastAgentError,
    },
    graphql::{client_network::query_network_subgraph, QueryError},
    init_tracing, wallet_address, GraphcastNetworkName, LogFormat,
};
use graphcast_sdk::{cf_nameserver, Account};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use tracing::info;

use crate::metrics::NUM_TOPICS;
use crate::operator::notifier::NotificationMode;
use crate::{active_allocation_hashes, syncing_deployment_hashes};

#[derive(clap::ValueEnum, Clone, Debug, Serialize, Deserialize, Default)]
pub enum CoverageLevel {
    None,
    Minimal,
    #[default]
    OnChain,
    Comprehensive,
}

fn is_valid_ethereum_address(address: &str) -> bool {
    address.len() == 42
        && address.starts_with("0x")
        && address[2..].chars().all(|c| c.is_ascii_hexdigit())
}

fn parse_indexer_address(value: &str) -> Result<String, ConfigError> {
    if !is_valid_ethereum_address(value) {
        return Err(ConfigError::ValidateInput(
            "Invalid Ethereum address".to_string(),
        ));
    }

    Ok(value.to_string())
}

#[derive(Clone, Debug, Parser, Serialize, Deserialize, Getters, Default)]
#[clap(
    name = "subgraph-radio",
    about = "Cross-check POIs with other Indexer and received subgraph owner notifications in real time",
    author = "GraphOps"
)]
pub struct Config {
    #[command(flatten)]
    pub graph_stack: GraphStack,
    #[command(flatten)]
    pub waku: Waku,
    #[command(flatten)]
    pub radio_setup: RadioSetup,
    #[arg(
        short,
        long,
        value_name = "config_file",
        env = "CONFIG_FILE",
        help = "Configuration file (toml or yaml format)"
    )]
    pub config_file: Option<String>,
}

impl Config {
    /// Parse config arguments
    pub fn args() -> Self {
        let config = if let Ok(file_path) = std::env::var("CONFIG_FILE") {
            confy::load_path::<Config>(file_path.clone()).unwrap_or_else(|e| {
                panic!(
                    "{} file cannot be parsed into Config: {}",
                    file_path.clone(),
                    e
                )
            })
        } else {
            Config::parse()
        };

        std::env::set_var("RUST_LOG", config.radio_setup().log_level.clone());
        // Enables tracing under RUST_LOG variable
        init_tracing(config.radio_setup().log_format.to_string()).expect("Could not set up global default subscriber for logger, check environmental variable `RUST_LOG` or the CLI input `log-level`");
        config
    }

    pub async fn validate_indexer_address(&self) {
        let input = self.wallet_input().unwrap();
        let wallet = build_wallet(input).unwrap();
        let agent = wallet_address(&wallet);

        let account = Account::new(
            agent.to_ascii_lowercase(),
            self.graph_stack()
                .indexer_address()
                .to_string()
                .to_ascii_lowercase(),
        );

        let verified = account
            .verify(
                self.graph_stack().network_subgraph(),
                self.graph_stack().registry_subgraph(),
                &IdentityValidation::Indexer,
            )
            .await;

        if verified.is_err() {
            panic!(
                "Indexer address validation failed: {:?}",
                verified.unwrap_err()
            );
        }
    }

    /// Validate that private key as an Eth wallet
    fn parse_key(value: &str) -> Result<String, WalletError> {
        // The wallet can be stored instead of the original private key
        let wallet = build_wallet(value)?;
        let address = wallet_address(&wallet);
        info!(address, "Resolved Graphcast id");
        Ok(String::from(value))
    }

    /// Private key takes precedence over mnemonic
    pub fn wallet_input(&self) -> Result<&String, ConfigError> {
        match (
            &self.graph_stack().private_key,
            &self.graph_stack().mnemonic,
        ) {
            (Some(p), _) => Ok(p),
            (_, Some(m)) => Ok(m),
            _ => Err(ConfigError::ValidateInput(
                "Must provide either private key or mnemonic".to_string(),
            )),
        }
    }

    pub async fn to_graphcast_agent_config(
        &self,
    ) -> Result<GraphcastAgentConfig, GraphcastAgentError> {
        let wallet_key = self.wallet_input().unwrap().to_string();
        let topics = self.radio_setup().topics.clone();

        GraphcastAgentConfig::new(
            wallet_key,
            self.graph_stack().indexer_address.clone(),
            self.radio_setup().radio_name.clone(),
            self.graph_stack().registry_subgraph.clone(),
            self.graph_stack().network_subgraph.clone(),
            self.radio_setup().id_validation.clone(),
            Some(self.graph_stack().graph_node_status_endpoint.clone()),
            Some(self.waku().boot_node_addresses.clone()),
            Some(self.radio_setup().graphcast_network.to_string()),
            Some(topics),
            self.waku().waku_node_key.clone(),
            self.waku().waku_host.clone(),
            self.waku().waku_port.clone(),
            self.waku().waku_addr.clone(),
            self.waku().filter_protocol,
            self.waku().discv5_enrs.clone(),
            self.waku().discv5_port,
            self.waku().discv5_enrs.clone().unwrap_or_default(),
            Some(cf_nameserver().to_string()),
        )
        .await
    }

    pub async fn basic_info(&self) -> Result<(&str, u64), QueryError> {
        let my_address = self.graph_stack().indexer_address();
        let my_stake = query_network_subgraph(self.graph_stack().network_subgraph(), my_address)
            .await
            .unwrap()
            .indexer_stake() as u64;
        info!(
            my_address,
            my_stake, "Initializing radio operator for indexer identity",
        );
        Ok((my_address, my_stake))
    }

    pub fn callbook(&self) -> CallBook {
        CallBook::new(
            self.graph_stack().registry_subgraph.clone(),
            self.graph_stack().network_subgraph.clone(),
            Some(self.graph_stack().graph_node_status_endpoint.clone()),
        )
    }

    /// Generate a set of unique topics along with given static topics
    #[autometrics]
    pub async fn generate_topics(&self, coverage: &CoverageLevel) -> Vec<String> {
        let static_topics = HashSet::from_iter(self.radio_setup().topics.to_vec());
        let topics = match coverage {
            CoverageLevel::None => HashSet::new(),
            CoverageLevel::Minimal => static_topics,
            CoverageLevel::OnChain => {
                let mut topics: HashSet<String> = active_allocation_hashes(
                    self.callbook().graph_network(),
                    &self.graph_stack().indexer_address,
                )
                .await
                .into_iter()
                .collect();
                topics.extend(static_topics);
                topics
            }
            CoverageLevel::Comprehensive => {
                let active_topics: HashSet<String> = active_allocation_hashes(
                    self.callbook().graph_network(),
                    &self.graph_stack().indexer_address,
                )
                .await
                .into_iter()
                .collect();
                let mut additional_topics: HashSet<String> =
                    syncing_deployment_hashes(self.graph_stack().graph_node_status_endpoint())
                        .await
                        .into_iter()
                        .collect();

                additional_topics.extend(active_topics);
                additional_topics.extend(static_topics);
                additional_topics
            }
        };

        NUM_TOPICS.set(topics.len() as i64);
        topics.into_iter().collect::<Vec<String>>()
    }

    /// Supprot multiple protocol networks, first from the user configured protocol_network or
    /// resolve to utilize the network subgraph endpoint
    pub fn protocol_network(&self) -> Result<String, ConfigError> {
        let network = if let Some(network) = self.graph_stack().protocol_network().clone() {
            network
        } else {
            self.graph_stack()
                .network_subgraph()
                .split_terminator('/')
                .last()
                .and_then(|suf| suf.strip_prefix("graph-network-"))
                .ok_or(ConfigError::ValidateInput(
                    format!("Not a conventionally parseable graph-network subgraph API: {:#?}, please provide a protocol network specification",
                    self.graph_stack().network_subgraph())
                ))?
                .to_string()
        };
        Ok(network)
    }
}

#[derive(Clone, Debug, Args, Serialize, Deserialize, Getters, Default)]
#[group(required = true, multiple = true)]
pub struct GraphStack {
    #[clap(
        long,
        value_name = "ENDPOINT",
        env = "GRAPH_NODE_STATUS_ENDPOINT",
        help = "API endpoint to the Graph Node status endpoint"
    )]
    pub graph_node_status_endpoint: String,
    #[clap(
        long,
        value_parser = parse_indexer_address,
        value_name = "INDEXER_ADDRESS",
        env = "INDEXER_ADDRESS",
        help = "Graph account corresponding to Graphcast operator"
    )]
    pub indexer_address: String,
    #[clap(
        long,
        value_name = "SUBGRAPH",
        env = "REGISTRY_SUBGRAPH",
        help = "Subgraph endpoint to the Graphcast Registry",
        default_value = "https://api.thegraph.com/subgraphs/name/hopeyen/graphcast-registry-mainnet"
    )]
    pub registry_subgraph: String,
    #[clap(
        long,
        value_name = "SUBGRAPH",
        env = "NETWORK_SUBGRAPH",
        help = "Subgraph endpoint to The Graph network subgraph",
        default_value = "https://api.thegraph.com/subgraphs/name/graphprotocol/graph-network-mainnet"
    )]
    pub network_subgraph: String,
    #[clap(
        long,
        value_name = "KEY",
        value_parser = Config::parse_key,
        env = "PRIVATE_KEY",
        hide_env_values = true,
        help = "Private key to the Graphcast ID wallet (Precendence over mnemonics)",
    )]
    // should keep this value private, this is current public due to the constructing a Config in test-utils
    // We can get around this by making an explicit function to make config instead of direct build in {}
    pub private_key: Option<String>,
    #[clap(
        long,
        value_name = "KEY",
        value_parser = Config::parse_key,
        env = "MNEMONIC",
        hide_env_values = true,
        help = "Mnemonic to the Graphcast ID wallet (first address of the wallet is used; Only one of private key or mnemonic is needed)",
    )]
    pub mnemonic: Option<String>,
    #[clap(
        long,
        value_name = "ENDPOINT",
        env = "INDEXER_MANAGEMENT_SERVER_ENDPOINT",
        help = "API endpoint to the Indexer management server endpoint"
    )]
    pub indexer_management_server_endpoint: Option<String>,
    #[clap(
        long,
        value_name = "NETWORK",
        env = "PROTOCOL_NETWORK",
        help = "The protocol network for The Graph (currently match with suffix of the network subgraph API)"
    )]
    pub protocol_network: Option<String>,
}

#[derive(Clone, Debug, Args, Serialize, Deserialize, Default)]
#[group(required = true, multiple = true)]
pub struct RadioSetup {
    #[clap(
        long,
        value_name = "GRAPHCAST_NETWORK",
        default_value = "mainnet",
        env = "GRAPHCAST_NETWORK",
        help = "Supported Graphcast networks: mainnet, testnet"
    )]
    pub graphcast_network: GraphcastNetworkName,
    #[clap(
        long,
        value_name = "[TOPIC]",
        value_delimiter = ',',
        env = "TOPICS",
        help = "Comma separated static list of content topics to subscribe to (Static list to include)"
    )]
    pub topics: Vec<String>,
    #[clap(
        long,
        value_name = "GOSSIP_TOPIC_COVERAGE",
        value_enum,
        default_value = "comprehensive",
        env = "GOSSIP_TOPIC_COVERAGE",
        help = "Toggle for gossip topic coverage level",
        long_help = "Topic gossip coverage level\ncomprehensive: Subscribe to on-chain topics, user defined static topics, and additional topics\n
            on-chain: Subscribe to on-chain topics and user defined static topics\nminimal: Only subscribe to user defined static topics.\n
            Default: comprehensive"
    )]
    pub gossip_topic_coverage: CoverageLevel,
    #[clap(
        long,
        value_name = "AUTO_UPGRADE_COVERAGE",
        value_enum,
        default_value = "comprehensive",
        env = "AUTO_UPGRADE_COVERAGE",
        help = "Toggle for the types of subgraph the radio send offchain syncing commands to indexer management server. Default to upgrade all syncing deployments",
        long_help = "Topic upgrade coverage level\ncomprehensive: on-chain allocations, user defined static topics, and additional topics\n
            on-chain: Subscribe to on-chain topics and user defined static topics\nminimal: Only subscribe to user defined static topics.\n
            none: no automatic upgrade, only notifications.\nDefault: comprehensive"
    )]
    pub auto_upgrade_coverage: CoverageLevel,
    #[clap(
        long,
        value_parser = value_parser!(u64),
        default_value = "86400",
        value_name = "RATELIMIT_THRESHOLD",
        env = "RATELIMIT_THRESHOLD",
        help = "Set upgrade intent ratelimit in seconds: only one upgrade per subgraph within the threshold (default: 86400 seconds = 1 day)"
    )]
    pub auto_upgrade_ratelimit: u64,
    #[clap(
        long,
        value_parser = value_parser!(u64),
        default_value = "120",
        value_name = "COLLECT_MESSAGE_DURATION",
        env = "COLLECT_MESSAGE_DURATION",
        help = "Set the minimum duration to wait for a topic message collection"
    )]
    pub collect_message_duration: u64,
    #[clap(
        long,
        value_name = "SLACK_WEBHOOK",
        help = "Slack webhook URL to send messags to",
        env = "SLACK_WEBHOOK"
    )]
    pub slack_webhook: Option<String>,
    #[clap(
        long,
        value_name = "DISCORD_WEBHOOK",
        help = "Discord webhook URL to send messages to",
        env = "DISCORD_WEBHOOK"
    )]
    pub discord_webhook: Option<String>,
    #[clap(
        long,
        value_name = "TELEGRAM_TOKEN",
        help = "Telegram Bot API Token",
        env = "TELEGRAM_TOKEN"
    )]
    pub telegram_token: Option<String>,
    #[clap(
        long,
        value_name = "TELEGRAM_CHAT_ID",
        help = "Id of Telegram chat (DM or group) to send messages to",
        env = "TELEGRAM_CHAT_ID"
    )]
    pub telegram_chat_id: Option<i64>,
    #[clap(
        long,
        value_name = "METRICS_HOST",
        default_value = "0.0.0.0",
        help = "If port is set, the Radio will expose Prometheus metrics on the given host. This requires having a local Prometheus server running and scraping metrics on the given port.",
        env = "METRICS_HOST"
    )]
    pub metrics_host: String,
    #[clap(
        long,
        value_name = "METRICS_PORT",
        help = "If set, the Radio will expose Prometheus metrics on the given port (off by default). This requires having a local Prometheus server running and scraping metrics on the given port.",
        env = "METRICS_PORT"
    )]
    pub metrics_port: Option<u16>,
    #[clap(
        long,
        value_name = "SERVER_HOST",
        default_value = "0.0.0.0",
        help = "If port is set, the Radio will expose API service on the given host.",
        env = "SERVER_HOST"
    )]
    pub server_host: String,
    #[clap(
        long,
        value_name = "SERVER_PORT",
        help = "If set, the Radio will expose API service on the given port (off by default).",
        env = "SERVER_PORT"
    )]
    pub server_port: Option<u16>,
    #[clap(
        long,
        value_name = "RADIO_NAME",
        env = "RADIO_NAME",
        default_value = "subgraph-radio"
    )]
    pub radio_name: String,
    #[clap(
        long,
        value_name = "ID_VALIDATION",
        value_enum,
        default_value = "indexer",
        env = "ID_VALIDATION",
        help = "Identity validaiton mechanism for message signers",
        long_help = "Identity validaiton mechanism for message signers. Default: indexer\n
        no-check: all messages signer is valid, \n
        valid-address: signer needs to be an valid Eth address, \n
        graphcast-registered: must be registered at Graphcast Registry, \n
        graph-network-account: must be a Graph account, \n
        registered-indexer: must be registered at Graphcast Registry, correspond to and Indexer statisfying indexer minimum stake requirement, \n
        indexer: must be registered at Graphcast Registry or is a Graph Account, correspond to and Indexer statisfying indexer minimum stake requirement"
    )]
    pub id_validation: IdentityValidation,
    #[clap(
        long,
        value_name = "TOPIC_UPDATE_INTERVAL",
        env = "TOPIC_UPDATE_INTERVAL",
        default_value = "600"
    )]
    pub topic_update_interval: u64,
    #[clap(
        long,
        value_name = "LOG_LEVEL",
        default_value = "info",
        help = "logging configurationt to set as RUST_LOG",
        env = "RUST_LOG"
    )]
    pub log_level: String,
    #[clap(
        long,
        value_name = "LOG_FORMAT",
        env = "LOG_FORMAT",
        help = "Supported logging formats: pretty, json, full, compact",
        long_help = "pretty: verbose and human readable; json: not verbose and parsable; compact:  not verbose and not parsable; full: verbose and not parseable",
        default_value = "pretty"
    )]
    pub log_format: LogFormat,
    #[clap(
        long,
        value_name = "NOTIFICATION_MODE",
        env = "NOTIFICATION_MODE",
        help = "Supported: live, periodic-report, periodic-update",
        long_help = "live: send a notification as soon as it finds a divergence; periodic-report: send a notification on a specified interval (default is 24 hours but can be configured with the NOTIFICATION_INTERVAL variable) with a summary and a list of divergent subgraphs; periodic-update: send a notification on a specified interval (default is 24 hours but can be configured with the NOTIFICATION_INTERVAL variable) containing updates since the previous notification",
        default_value = "live"
    )]
    pub notification_mode: NotificationMode,
    #[clap(
        long,
        value_name = "NOTIFICATION_INTERVAL",
        env = "NOTIFICATION_INTERVAL",
        help = "Interval (in hours) between sending a divergence summary notification",
        default_value = "24"
    )]
    pub notification_interval: u64,
    #[clap(
        long,
        value_name = "SQLITE_FILE_PATH",
        help = "Path to sqlite database file. If none is set, will spin up an in-memory db.",
        env = "SQLITE_FILE_PATH"
    )]
    pub sqlite_file_path: Option<String>,
}

#[derive(Clone, Debug, Args, Serialize, Deserialize, Default)]
#[group(required = false, multiple = true)]
pub struct Waku {
    #[clap(
        long,
        value_name = "WAKU_HOST",
        help = "Host for the Waku gossip client",
        env = "WAKU_HOST"
    )]
    pub waku_host: Option<String>,
    #[clap(
        long,
        value_name = "WAKU_PORT",
        help = "Port for the Waku gossip client",
        env = "WAKU_PORT"
    )]
    pub waku_port: Option<String>,
    #[clap(
        long,
        value_name = "KEY",
        env = "WAKU_NODE_KEY",
        hide_env_values = true,
        help = "Private key to the Waku node id"
    )]
    pub waku_node_key: Option<String>,
    #[clap(
        long,
        value_name = "KEY",
        env = "WAKU_ADDRESS",
        hide_env_values = true,
        help = "Advertised address to be connected among the Waku peers"
    )]
    pub waku_addr: Option<String>,
    #[clap(
        long,
        value_name = "NODE_ADDRESSES",
        help = "Comma separated static list of waku boot nodes to connect to",
        env = "BOOT_NODE_ADDRESSES"
    )]
    pub boot_node_addresses: Vec<String>,
    #[clap(
        long,
        value_name = "WAKU_LOG_LEVEL",
        help = "Waku node logging configuration",
        default_value = "fatal",
        env = "WAKU_LOG_LEVEL"
    )]
    pub waku_log_level: String,
    #[clap(
        long,
        value_name = "DISCV5_ENRS",
        help = "Comma separated ENRs for Waku discv5 bootstrapping",
        env = "DISCV5_ENRS"
    )]
    pub discv5_enrs: Option<Vec<String>>,
    #[clap(
        long,
        value_name = "DISCV5_PORT",
        help = "Waku node to expose discoverable udp port",
        env = "DISCV5_PORT"
    )]
    pub discv5_port: Option<u16>,
    #[clap(long, value_name = "FILTER_PROTOCOL", env = "ENABLE_FILTER_PROTOCOL")]
    pub filter_protocol: Option<bool>,
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("Validate the input: {0}")]
    ValidateInput(String),
    #[error("Generate JSON representation of the config file: {0}")]
    GenerateJson(serde_json::Error),
    #[error("QueryError: {0}")]
    QueryError(QueryError),
    #[error("Toml file error: {0}")]
    ReadStr(std::io::Error),
    #[error("Unknown error: {0}")]
    Other(anyhow::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> Config {
        Config {
            graph_stack: GraphStack {
                indexer_address: String::from("indexer_address"),
                graph_node_status_endpoint: String::from("http://localhost:8030/graphql"),
                private_key: Some(
                    "ccaea3e3aca412cb3920dbecd77bc725dfe9a5e16f940f19912d9c9dbee01e8f".to_string(),
                ),
                mnemonic: None,
                registry_subgraph: String::from(
                    "https://api.thegraph.com/subgraphs/name/hopeyen/graphcast-registry-goerli",
                ),
                network_subgraph: String::from(
                    "https://api.thegraph.com/subgraphs/name/graphprotocol/graph-network-goerli",
                ),
                indexer_management_server_endpoint: None,
                protocol_network: None,
            },
            waku: Waku {
                waku_host: None,
                waku_port: None,
                waku_node_key: None,
                boot_node_addresses: vec!["/dns4/graph.myrandomdemos.online/tcp/31900/p2p/16Uiu2HAm5oH7pmpoTQoS5PgdehCinnrtpPAUhBuajG6AG3XHrR2P".to_string(), "/dns4/graph.myrandomdemos.online/tcp/8000/wss/p2p/16Uiu2HAm5oH7pmpoTQoS5PgdehCinnrtpPAUhBuajG6AG3XHrR2P".to_string()],
                waku_log_level: "fatal".to_string(),
                waku_addr: None,
                discv5_enrs: None,
                discv5_port: None,
                filter_protocol: None,
            },
            radio_setup: RadioSetup {
                ..Default::default()
            },
            config_file: None,
        }
    }

    #[test]
    fn protocol_network() {
        let mut config = test_config();
        assert_eq!(&config.protocol_network().unwrap(), "goerli");

        config.graph_stack.protocol_network = Some("arbitrum-one".to_string());
        assert_eq!(&config.protocol_network().unwrap(), "arbitrum-one");
    }
}
