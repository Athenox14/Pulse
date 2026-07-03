use sqlx::sqlite::{SqlitePool, SqlitePoolOptions};
use anyhow::Result;

pub async fn init_pool(path: &str) -> Result<SqlitePool> {
    let url = format!("sqlite://{}?mode=rwc", path);
    let pool = SqlitePoolOptions::new().max_connections(8).connect(&url).await?;
    sqlx::query(SCHEMA).execute(&pool).await?;
    Ok(pool)
}

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS monitors (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    type TEXT NOT NULL,
    url TEXT,
    hostname TEXT,
    port INTEGER,
    interval_sec INTEGER NOT NULL DEFAULT 60,
    retries INTEGER NOT NULL DEFAULT 0,
    retry_interval_sec INTEGER NOT NULL DEFAULT 60,
    timeout_sec INTEGER NOT NULL DEFAULT 30,
    keyword TEXT,
    expected_status INTEGER,
    method TEXT,
    headers TEXT,
    body TEXT,
    active INTEGER NOT NULL DEFAULT 1,
    upside_down INTEGER NOT NULL DEFAULT 0,
    max_redirects INTEGER NOT NULL DEFAULT 10,
    notification_ids TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS heartbeats (
    id TEXT PRIMARY KEY,
    monitor_id TEXT NOT NULL,
    status INTEGER NOT NULL,
    msg TEXT,
    ping_ms INTEGER,
    important INTEGER NOT NULL DEFAULT 0,
    time TEXT NOT NULL DEFAULT (datetime('now')),
    FOREIGN KEY(monitor_id) REFERENCES monitors(id) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS idx_hb_monitor_time ON heartbeats(monitor_id, time DESC);

CREATE TABLE IF NOT EXISTS notifications (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    type TEXT NOT NULL,
    config TEXT NOT NULL,
    active INTEGER NOT NULL DEFAULT 1
);

CREATE TABLE IF NOT EXISTS status_pages (
    id TEXT PRIMARY KEY,
    slug TEXT NOT NULL UNIQUE,
    title TEXT NOT NULL,
    description TEXT,
    monitor_ids TEXT NOT NULL DEFAULT '[]',
    published INTEGER NOT NULL DEFAULT 1
);

CREATE TABLE IF NOT EXISTS maintenance_windows (
    id TEXT PRIMARY KEY,
    title TEXT NOT NULL,
    monitor_ids TEXT NOT NULL DEFAULT '[]',
    start_time TEXT NOT NULL,
    end_time TEXT NOT NULL,
    active INTEGER NOT NULL DEFAULT 1
);

CREATE TABLE IF NOT EXISTS users (
    id TEXT PRIMARY KEY,
    username TEXT NOT NULL UNIQUE,
    password_hash TEXT NOT NULL
);
"#;
