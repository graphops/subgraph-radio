use std::sync::mpsc;

use dotenv::dotenv;

use graphcast_sdk::{graphcast_agent::GraphcastAgent, WakuMessage};
use subgraph_radio::{config::Config, operator::RadioOperator, RADIO_OPERATOR};

extern crate partial_application;

#[tokio::main]
async fn main() {
    dotenv().ok();
    // Parse basic configurations
    let radio_config = Config::args();

    let (sender, receiver) = mpsc::channel::<WakuMessage>();
    let agent = GraphcastAgent::new(
        radio_config.to_graphcast_agent_config().await.unwrap(),
        sender,
    )
    .await
    .expect("Initialize Graphcast agent");
    // Initialization and pass in for static lifetime throughout the program
    let radio_operator = RadioOperator::new(&radio_config, agent).await;

    // Start message processes
    radio_operator.message_processor(receiver).await;

    _ = RADIO_OPERATOR.set(radio_operator);

    // Start radio operations
    RADIO_OPERATOR.get().unwrap().run().await;
}
