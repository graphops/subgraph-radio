use criterion::async_executor::FuturesExecutor;
use criterion::{black_box, criterion_group, criterion_main, Criterion};
use graphcast_sdk::graphcast_agent::message_typing::IdentityValidation;
use graphcast_sdk::graphcast_agent::GraphcastAgent;
use subgraph_radio::operator::RadioOperator;

use rand::{thread_rng, Rng};
use secp256k1::SecretKey;
use std::collections::HashMap;
use std::sync::mpsc;

use graphcast_sdk::networks::NetworkName;
use graphcast_sdk::{BlockPointer, GraphcastNetworkName, LogFormat, NetworkPointer, WakuMessage};
use subgraph_radio::config::{Config, CoverageLevel, GraphStack, RadioInfrastructure, Waku};

fn gossip_poi_bench(c: &mut Criterion) {
    let identifiers = black_box(vec!["identifier1".to_string(), "identifier2".to_string()]);
    let network_chainhead_blocks: HashMap<NetworkName, BlockPointer> =
        black_box(Default::default());
    let subgraph_network_latest_blocks: HashMap<String, NetworkPointer> =
        black_box(Default::default());
    let pk = black_box(generate_random_private_key());

    let config = black_box(Config {
        graph_stack: GraphStack {
            indexer_address: String::from("indexer_address"),
            graph_node_status_endpoint: String::from("http://localhost:8030/graphql"),
            private_key: Some(pk.display_secret().to_string()),
            mnemonic: None,
            registry_subgraph: String::from(
                "https://api.thegraph.com/subgraphs/name/hopeyen/graphcast-registry-goerli",
            ),
            network_subgraph: String::from(
                "https://api.thegraph.com/subgraphs/name/graphprotocol/graph-network-goerli",
            ),
            indexer_management_server_endpoint: None,
        },
        waku: Waku {
            waku_host: None,
            waku_port: None,
            waku_node_key: None,
            boot_node_addresses: vec![],
            waku_log_level: None,
            waku_addr: None,
            discv5_enrs: None,
            discv5_port: None,
            filter_protocol: None,
        },
        radio_infrastructure: RadioInfrastructure {
            radio_name: String::from("test"),
            topics: vec![String::from(
                "QmbaLc7fEfLGUioKWehRhq838rRzeR8cBoapNJWNSAZE8u",
            )],
            coverage: CoverageLevel::Comprehensive,
            collect_message_duration: 10,
            ratelimit_threshold: 60000,
            log_level: String::from("info"),
            slack_token: None,
            slack_channel: None,
            discord_webhook: None,
            telegram_token: None,
            telegram_chat_id: None,
            metrics_host: String::from("0.0.0.0"),
            metrics_port: None,
            server_host: String::from("0.0.0.0"),
            server_port: None,
            persistence_file_path: None,
            id_validation: IdentityValidation::NoCheck,
            topic_update_interval: 600,
            log_format: LogFormat::Pretty,
            graphcast_network: GraphcastNetworkName::Testnet,
            auto_upgrade: CoverageLevel::Comprehensive,
        },
        config_file: None,
    });

    c.bench_function("gossip_poi", move |b| {
        b.to_async(FuturesExecutor).iter(|| async {
            let (sender, _) = mpsc::channel::<WakuMessage>();
            let agent =
                GraphcastAgent::new(config.to_graphcast_agent_config().await.unwrap(), sender)
                    .await
                    .expect("Initialize Graphcast agent");
            RadioOperator::new(&config, agent)
                .await
                .gossip_poi(
                    identifiers.clone(),
                    &network_chainhead_blocks,
                    &subgraph_network_latest_blocks,
                )
                .await
        })
    });
}

criterion_group!(benches, gossip_poi_bench);
criterion_main!(benches);

pub fn generate_random_private_key() -> SecretKey {
    let mut rng = thread_rng();
    let mut private_key = [0u8; 32];
    rng.fill(&mut private_key[..]);

    SecretKey::from_slice(&private_key).expect("Error parsing secret key")
}
