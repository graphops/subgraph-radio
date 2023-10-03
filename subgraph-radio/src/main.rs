use std::sync::mpsc;

use dotenv::dotenv;

use graphcast_sdk::{graphcast_agent::GraphcastAgent, WakuMessage};
use subgraph_radio::{config::Config, operator::RadioOperator, RADIO_OPERATOR};
use tracing::info;
use warp::serve;
use warp::Filter;

extern crate partial_application;

#[tokio::main]
async fn main() {
    dotenv().ok();

    // TODO: start frontend
    let wasm_bytes = include_bytes!("qos-dashboard.wasm");
    let html_bytes = include_bytes!("qos-dashboard.html");

    let wasm = warp::path("qos-dashboard.wasm").and_then(move || async move {
        let reply = warp::reply::with_header(&wasm_bytes[..], "content-type", "application/wasm");
        Ok::<_, warp::Rejection>(reply)
    });

    let html = warp::path("index.html").and_then(move || async move {
        let reply = warp::reply::with_header(&html_bytes[..], "content-type", "text/html");
        Ok::<_, warp::Rejection>(reply)
    });

    let routes = wasm.or(html);

    serve(routes)
        .run(([127, 0, 0, 1], 3030)) // Change 3030 to your desired port
        .await;

    info!("got here");

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

    // Start separate processes
    radio_operator.prepare(receiver).await;

    _ = RADIO_OPERATOR.set(radio_operator);

    // Start radio operations
    RADIO_OPERATOR.get().unwrap().run().await;
}
