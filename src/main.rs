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

    let state = AppState {
        db: pool,
        ws_tx,
        due_at: Arc::new(Mutex::new(std::collections::HashMap::new())),
        jwt_secret: Arc::new(std::env::var("PULSE_JWT_SECRET").unwrap_or_else(|_| "pulse-dev-secret".into())),
    };

    let scheduler_state = state.clone();
    tokio::spawn(async move {
        monitors::run_scheduler(scheduler_state).await;
    });

    let app = api::router(state)
        .nest_service("/ui", ServeDir::new("ui"))
        .layer(CorsLayer::permissive());

    let addr = std::env::var("PULSE_ADDR").unwrap_or_else(|_| "0.0.0.0:3939".to_string());
    tracing::info!("Pulse listening on {addr}");
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
