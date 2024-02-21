use chrono::Utc;
use clap::Parser;
use graphcast_sdk::{
    build_wallet,
    graphcast_agent::{
        message_typing::{GraphcastMessage, IdentityValidation},
        waku_handling::{connect_multiaddresses, gather_nodes},
        WAKU_DISCOVERY_ENR,
    },
    init_tracing,
    networks::NetworkName,
    wallet_address, WakuPubSubTopic,
};
use prost::Message;
use std::{net::IpAddr, str::FromStr, thread::sleep, time::Duration};
use subgraph_radio::messages::{poi::PublicPoiMessage, upgrade::UpgradeIntentMessage};
use test_utils::{
    config::TestSenderConfig, dummy_msg::DummyMsg, find_random_udp_port,
    get_random_graph_network_account, get_random_graphcast_registered_account, get_random_indexer,
    get_random_registered_indexer, get_random_subgraph_staker, get_random_valid_eth_address,
};
use tracing::{error, info};
use waku::{waku_new, GossipSubParams, ProtocolId, WakuContentTopic, WakuMessage, WakuNodeConfig};

async fn start_sender(config: TestSenderConfig) {
    std::env::set_var(
        "RUST_LOG",
        "off,hyper=off,graphcast_sdk=trace,subgraph_radio=trace,test_sender=trace",
    );
    init_tracing("pretty".to_string()).expect("Could not set up global default subscriber for logger, check environmental variable `RUST_LOG` or the CLI input `log-level");

    let gossipsub_params = GossipSubParams {
        seen_messages_ttl_seconds: Some(1800),
        history_length: Some(100_000),
        ..Default::default()
    };

    let port = find_random_udp_port();
    let discv_port = find_random_udp_port();
    info!("Starting test sender instance on port {}", port);

    let discv5_nodes = vec![WAKU_DISCOVERY_ENR.to_string()];

    let node_config = WakuNodeConfig {
        host: IpAddr::from_str("127.0.0.1").ok(),
        port: Some(port.into()),
        advertise_addr: None,
        node_key: None,
        keep_alive_interval: None,
        relay: Some(true),
        min_peers_to_publish: Some(0),
        log_level: None,
        relay_topics: [].to_vec(),
        discv5: Some(true),
        discv5_bootstrap_nodes: discv5_nodes,
        discv5_udp_port: Some(discv_port),
        store: None,
        database_url: None,
        store_retention_max_messages: None,
        store_retention_max_seconds: None,
        gossipsub_params: Some(gossipsub_params),
        dns4_domain_name: None,
        websocket_params: None,
        dns_discovery_urls: vec![],
        dns_discovery_nameserver: None,
    };

    let node_handle = waku_new(Some(node_config)).unwrap().start().unwrap();

    info!("config {:?}", config);

    let pk = match config.id_validation.unwrap() {
        IdentityValidation::NoCheck | IdentityValidation::ValidAddress => {
            get_random_valid_eth_address()
        }
        IdentityValidation::GraphcastRegistered => get_random_graphcast_registered_account(),
        IdentityValidation::GraphNetworkAccount => get_random_graph_network_account(),
        IdentityValidation::RegisteredIndexer => get_random_registered_indexer(),
        IdentityValidation::Indexer => get_random_indexer(),
        IdentityValidation::SubgraphStaker => get_random_subgraph_staker(),
    };

    let wallet = build_wallet(&pk).unwrap();
    let graph_account = wallet_address(&wallet);

    let pubsub_topic_str = "/waku/2/graphcast-v0-testnet/proto";
    let pubsub_topic = WakuPubSubTopic::from_str(pubsub_topic_str).unwrap();
    loop {
        for topic in config.topics.clone() {
            let nodes = gather_nodes(vec![], &pubsub_topic);
            // Connect to peers on the filter protocol
            connect_multiaddresses(nodes, &node_handle, ProtocolId::Filter);

            let content_topic = format!("/{}/0/{}/proto", config.radio_name, topic);
            let content_topic = WakuContentTopic::from_str(&content_topic).unwrap();

            let nonce = config.nonce.clone().and_then(|s| s.parse::<u64>().ok());

            let timestamp = Utc::now().timestamp() as u64;
            let block_number = (timestamp + 9) / 10 * 10;

            let radio_payload = PublicPoiMessage::build(
                topic.clone(),
                config.poi.clone().unwrap(),
                nonce.unwrap_or(timestamp),
                NetworkName::Mainnet,
                block_number,
                config.block_hash.clone().unwrap(),
                graph_account.clone(),
            );

            let graphcast_message = GraphcastMessage::build(
                &wallet,
                topic.clone(),
                graph_account.clone(),
                timestamp,
                radio_payload,
            )
            .await
            .unwrap();

            assert!(wallet_address(&wallet) == graphcast_message.recover_sender_address().unwrap());

            match graphcast_message.send_to_waku(
                &node_handle,
                WakuPubSubTopic::from_str("/waku/2/graphcast-v0-testnet/proto").unwrap(),
                content_topic.clone(),
            ) {
                Ok(id) => {
                    info!("Message sent successfully. Mеssage id: {:?}", id);
                }
                Err(e) => {
                    error!("Failed to send message: {:?}", e);
                }
            }

            let payload = DummyMsg::new("hello".to_string(), 42);
            let graphcast_message = GraphcastMessage::build(
                &wallet,
                topic.clone(),
                graph_account.clone(),
                timestamp,
                payload,
            )
            .await
            .unwrap();

            match graphcast_message.send_to_waku(
                &node_handle,
                WakuPubSubTopic::from_str("/waku/2/graphcast-v0-testnet/proto").unwrap(),
                content_topic.clone(),
            ) {
                Ok(id) => {
                    info!("Message sent successfully. Mеssage id: {:?}", id);
                }
                Err(e) => {
                    error!("Failed to send message: {:?}", e);
                }
            }

            let nonce = config.nonce.clone().and_then(|s| s.parse::<u64>().ok());
            let timestamp = Utc::now().timestamp() as u64;

            let payload = UpgradeIntentMessage {
                deployment: topic.clone(),
                subgraph_id: "id".to_string(),
                new_hash: "Qm123".to_string(),
                nonce: nonce.unwrap_or(timestamp),
                graph_account: graph_account.clone(),
            };

            let graphcast_message = GraphcastMessage::build(
                &wallet,
                topic.clone(),
                graph_account.clone(),
                nonce.unwrap_or(timestamp),
                payload,
            )
            .await
            .unwrap();

            match graphcast_message.send_to_waku(
                &node_handle,
                WakuPubSubTopic::from_str("/waku/2/graphcast-v0-testnet/proto").unwrap(),
                content_topic.clone(),
            ) {
                Ok(id) => {
                    info!("Message sent successfully. Mеssage id: {:?}", id);
                }
                Err(e) => {
                    error!("Failed to send message: {:?}", e);
                }
            }

            let radio_payload = PublicPoiMessage::build(
                topic.clone(),
                config.poi.clone().unwrap(),
                nonce.unwrap_or(timestamp),
                NetworkName::Mainnet,
                block_number,
                config.block_hash.clone().unwrap(),
                "graph_account_fake".to_string(),
            );

            let graphcast_message = GraphcastMessage {
                identifier: topic.clone(),
                nonce: nonce.unwrap_or(timestamp),
                graph_account: "invalid_address".to_string(),
                payload: radio_payload,
                signature: "invalid".to_string(),
            };

            let mut buff = Vec::new();
            Message::encode(&graphcast_message, &mut buff).expect("Could not encode :(");

            let waku_message = WakuMessage::new(
                buff,
                content_topic,
                2,
                Utc::now().timestamp() as usize,
                vec![],
                true,
            );

            node_handle
                .relay_publish_message(&waku_message, Some(pubsub_topic.clone()), None)
                .unwrap();

            sleep(Duration::from_secs(1));
        }

        sleep(Duration::from_secs(3));
    }
}

#[tokio::main]
pub async fn main() {
    let config = TestSenderConfig::parse();
    start_sender(config).await;
}
