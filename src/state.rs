use sqlx::SqlitePool;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{broadcast, Mutex};

#[derive(Clone)]
pub struct AppState {
    pub db: SqlitePool,
    pub ws_tx: broadcast::Sender<String>,
    pub due_at: Arc<Mutex<HashMap<String, tokio::time::Instant>>>,
    pub jwt_secret: Arc<String>,
}
