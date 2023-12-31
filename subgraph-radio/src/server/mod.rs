use axum::{extract::Extension, routing::get, Router};
use axum_server::Handle;
use graphcast_sdk::graphcast_agent::GraphcastAgent;
use sqlx::SqlitePool;
use std::net::SocketAddr;
use std::str::FromStr;

use std::sync::Arc;
use tracing::{debug, info};

use crate::{
    config::Config,
    server::{
        model::{build_schema, SubgraphRadioContext},
        routes::{graphql_handler, graphql_playground, health},
    },
};

pub mod model;
pub mod routes;

/// Run HTTP server to provide API services
/// Set up the routes for a radio health endpoint at `/health`
/// and a versioned GraphQL endpoint at `api/v1/graphql`
/// This function starts a API server at the configured server_host and server_port
pub async fn run_server(
    config: Config,
    db: &SqlitePool,
    graphcast_agent: &'static GraphcastAgent,
    handle: Handle,
) {
    if config.radio_setup().server_port.is_none() {
        return;
    }
    let port = config.radio_setup().server_port.unwrap();
    let context = Arc::new(SubgraphRadioContext::init(
        config.clone(),
        db,
        graphcast_agent,
    ));

    let schema = build_schema(Arc::clone(&context)).await;

    debug!("Setting up HTTP service");

    let app = Router::new()
        .route("/health", get(health))
        .route(
            "/api/v1/graphql",
            get(graphql_playground).post(graphql_handler),
        )
        .layer(Extension(schema))
        .layer(Extension(context));
    let addr = SocketAddr::from_str(&format!("{}:{}", config.radio_setup().server_host, port))
        .expect("Create address");

    info!(
        host = tracing::field::debug(&config.radio_setup().server_host),
        port, "Bind port to service"
    );

    axum_server::bind(addr)
        .handle(handle)
        .serve(app.into_make_service())
        .await
        .expect("Error starting API service");
}
