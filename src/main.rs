mod api;
mod db;
mod models;
mod monitors;
mod notify;
mod state;

use state::AppState;
use std::sync::Arc;
use tokio::sync::{broadcast, Mutex};
use tower_http::cors::CorsLayer;
use tower_http::services::ServeDir;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let db_path = std::env::var("PULSE_DB").unwrap_or_else(|_| "pulse.db".to_string());
    let pool = db::init_pool(&db_path).await?;
    let (ws_tx, _) = broadcast::channel(256);

    // Each monitor hits a distinct host once per interval — keep-alive pooling
    // never gets reused here, it just holds idle sockets in RAM for nothing.
    let http_client = reqwest::Client::builder()
        .pool_max_idle_per_host(0)
        .redirect(reqwest::redirect::Policy::limited(10))
        .build()?;

    let state = AppState {
        db: pool,
        ws_tx,
        tasks: Arc::new(Mutex::new(std::collections::HashMap::new())),
        jwt_secret: Arc::new(std::env::var("PULSE_JWT_SECRET").unwrap_or_else(|_| "pulse-dev-secret".into())),
        http_client,
    };

    monitors::spawn_all(&state).await;

    let app = api::router(state)
        .nest_service("/ui", ServeDir::new("ui"))
        .layer(CorsLayer::permissive());

    let addr = std::env::var("PULSE_ADDR").unwrap_or_else(|_| "0.0.0.0:3939".to_string());
    tracing::info!("Pulse listening on {addr}");
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
