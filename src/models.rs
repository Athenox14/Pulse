use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "text")]
pub enum MonitorType {
    Http,
    Tcp,
    Ping,
    Dns,
    Keyword,
    Json,
}

impl MonitorType {
    pub fn as_str(&self) -> &'static str {
        match self {
            MonitorType::Http => "http",
            MonitorType::Tcp => "tcp",
            MonitorType::Ping => "ping",
            MonitorType::Dns => "dns",
            MonitorType::Keyword => "keyword",
            MonitorType::Json => "json",
        }
    }
    pub fn from_str(s: &str) -> Self {
        match s {
            "tcp" => MonitorType::Tcp,
            "ping" => MonitorType::Ping,
            "dns" => MonitorType::Dns,
            "keyword" => MonitorType::Keyword,
            "json" => MonitorType::Json,
            _ => MonitorType::Http,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Monitor {
    pub id: String,
    pub name: String,
    #[serde(rename = "type")]
    pub monitor_type: String,
    pub url: Option<String>,
    pub hostname: Option<String>,
    pub port: Option<i64>,
    pub interval_sec: i64,
    pub retries: i64,
    pub retry_interval_sec: i64,
    pub timeout_sec: i64,
    pub keyword: Option<String>,
    pub expected_status: Option<i64>,
    pub method: Option<String>,
    pub headers: Option<String>,
    pub body: Option<String>,
    pub active: bool,
    pub upside_down: bool,
    pub max_redirects: i64,
    pub notification_ids: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CreateMonitor {
    pub name: String,
    #[serde(rename = "type")]
    pub monitor_type: String,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub hostname: Option<String>,
    #[serde(default)]
    pub port: Option<i64>,
    #[serde(default = "default_interval")]
    pub interval_sec: i64,
    #[serde(default)]
    pub retries: i64,
    #[serde(default = "default_retry_interval")]
    pub retry_interval_sec: i64,
    #[serde(default = "default_timeout")]
    pub timeout_sec: i64,
    #[serde(default)]
    pub keyword: Option<String>,
    #[serde(default)]
    pub expected_status: Option<i64>,
    #[serde(default)]
    pub method: Option<String>,
    #[serde(default)]
    pub headers: Option<String>,
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default = "default_true")]
    pub active: bool,
    #[serde(default)]
    pub upside_down: bool,
    #[serde(default = "default_redirects")]
    pub max_redirects: i64,
    #[serde(default)]
    pub notification_ids: Option<String>,
}

fn default_interval() -> i64 { 60 }
fn default_retry_interval() -> i64 { 60 }
fn default_timeout() -> i64 { 30 }
fn default_true() -> bool { true }
fn default_redirects() -> i64 { 10 }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Heartbeat {
    pub id: String,
    pub monitor_id: String,
    pub status: i64, // 0=down 1=up 2=pending 3=maintenance
    pub msg: Option<String>,
    pub ping_ms: Option<i64>,
    pub important: bool,
    pub time: DateTime<Utc>,
}

pub const STATUS_DOWN: i64 = 0;
pub const STATUS_UP: i64 = 1;
pub const STATUS_PENDING: i64 = 2;
pub const STATUS_MAINTENANCE: i64 = 3;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[derive(sqlx::FromRow)]
pub struct Notification {
    pub id: String,
    pub name: String,
    #[serde(rename = "type")]
    pub notif_type: String, // webhook, discord, slack, telegram
    pub config: String,     // json blob
    pub active: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CreateNotification {
    pub name: String,
    #[serde(rename = "type")]
    pub notif_type: String,
    pub config: serde_json::Value,
    #[serde(default = "default_true")]
    pub active: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[derive(sqlx::FromRow)]
pub struct StatusPage {
    pub id: String,
    pub slug: String,
    pub title: String,
    pub description: Option<String>,
    pub monitor_ids: String, // json array
    pub published: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CreateStatusPage {
    pub slug: String,
    pub title: String,
    pub description: Option<String>,
    #[serde(default)]
    pub monitor_ids: Vec<String>,
    #[serde(default = "default_true")]
    pub published: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MaintenanceWindow {
    pub id: String,
    pub title: String,
    pub monitor_ids: String,
    pub start_time: DateTime<Utc>,
    pub end_time: DateTime<Utc>,
    pub active: bool,
}

pub fn new_id() -> String {
    Uuid::new_v4().to_string()
}
