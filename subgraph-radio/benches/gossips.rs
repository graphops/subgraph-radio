use criterion::async_executor::FuturesExecutor;
use criterion::{black_box, criterion_group, criterion_main, Criterion};

use graphcast_sdk::graphcast_agent::GraphcastAgent;
use subgraph_radio::operator::RadioOperator;

use rand::{thread_rng, Rng};
use secp256k1::SecretKey;
use std::collections::HashMap;
use std::sync::mpsc;

use graphcast_sdk::networks::NetworkName;
use graphcast_sdk::{BlockPointer, NetworkPointer, WakuMessage};
use subgraph_radio::config::{Config, GraphStack};

fn gossip_poi_bench(c: &mut Criterion) {
    let identifiers = black_box(vec!["identifier1".to_string(), "identifier2".to_string()]);
    let network_chainhead_blocks: HashMap<NetworkName, BlockPointer> =
        black_box(Default::default());
    let subgraph_network_latest_blocks: HashMap<String, NetworkPointer> =
        black_box(Default::default());
    let pk = black_box(generate_random_private_key());

    let config = black_box(Config {
        graph_stack: GraphStack {
            private_key: Some(pk.display_secret().to_string()),
            ..Default::default()
        },
        ..Default::default()
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
