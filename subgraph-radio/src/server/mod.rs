use axum::{extract::Extension, routing::get, Router, Server};
use std::net::SocketAddr;
use std::str::FromStr;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use tracing::{debug, info};

use crate::{
    config::Config,
    server::{
        model::{build_schema, SubgraphRadioContext},
        routes::{graphql_handler, graphql_playground, health},
    },
    state::PersistedState,
};

pub mod model;
pub mod routes;

/// Run HTTP server to provide API services
/// Set up the routes for a radio health endpoint at `/health`
/// and a versioned GraphQL endpoint at `api/v1/graphql`
/// This function starts a API server at the configured server_host and server_port
pub async fn run_server(
    config: Config,
    persisted_state: &'static PersistedState,
    _running_program: Arc<AtomicBool>,
) {
    if config.server_port().is_none() {
        return;
    }
    let port = config.server_port().unwrap();
    let context = Arc::new(SubgraphRadioContext::init(config.clone(), persisted_state));

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
    let addr = SocketAddr::from_str(&format!("{}:{}", config.server_host(), port))
        .expect("Create address");

    info!(
        host = tracing::field::debug(config.server_host()),
        port, "Bind port to service"
    );
    Server::bind(&addr)
        .serve(app.into_make_service())
        // .with_graceful_shutdown(shutdown_signal(running_program))
        .await
        .expect("Error starting API service");
}
