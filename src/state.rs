use sqlx::SqlitePool;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{broadcast, Mutex};
use tokio::task::JoinHandle;

#[derive(Clone)]
pub struct AppState {
    pub db: SqlitePool,
    pub ws_tx: broadcast::Sender<String>,
    pub tasks: Arc<Mutex<HashMap<String, JoinHandle<()>>>>,
    pub http_client: reqwest::Client,
}
