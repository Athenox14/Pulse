use crate::models::Monitor;
use crate::state::AppState;

pub async fn dispatch(state: &AppState, m: &Monitor, status: i64, msg: &str) {
    let ids: Vec<String> = m
        .notification_ids
        .as_deref()
        .and_then(|s| serde_json::from_str(s).ok())
        .unwrap_or_default();
    if ids.is_empty() {
        return;
    }
    let status_text = if status == 1 { "UP" } else { "DOWN" };
    let text = format!("[Pulse] {} is {} - {}", m.name, status_text, msg);

    for id in ids {
        let row: Option<(String, String)> =
            sqlx::query_as("SELECT type, config FROM notifications WHERE id = ? AND active = 1")
                .bind(&id)
                .fetch_optional(&state.db)
                .await
                .ok()
                .flatten();
        let Some((ntype, config)) = row else { continue };
        let cfg: serde_json::Value = serde_json::from_str(&config).unwrap_or_default();
        let client = reqwest::Client::new();
        let result = match ntype.as_str() {
            "webhook" => {
                if let Some(url) = cfg.get("url").and_then(|v| v.as_str()) {
                    client
                        .post(url)
                        .json(&serde_json::json!({"monitor": m.name, "status": status_text, "msg": msg}))
                        .send()
                        .await
                        .map(|_| ())
                        .map_err(|e| e.to_string())
                } else {
                    Err("missing url".into())
                }
            }
            "discord" => {
                if let Some(url) = cfg.get("webhook_url").and_then(|v| v.as_str()) {
                    client
                        .post(url)
                        .json(&serde_json::json!({"content": text}))
                        .send()
                        .await
                        .map(|_| ())
                        .map_err(|e| e.to_string())
                } else {
                    Err("missing webhook_url".into())
                }
            }
            "slack" => {
                if let Some(url) = cfg.get("webhook_url").and_then(|v| v.as_str()) {
                    client
                        .post(url)
                        .json(&serde_json::json!({"text": text}))
                        .send()
                        .await
                        .map(|_| ())
                        .map_err(|e| e.to_string())
                } else {
                    Err("missing webhook_url".into())
                }
            }
            "telegram" => {
                let (token, chat_id) = (
                    cfg.get("bot_token").and_then(|v| v.as_str()),
                    cfg.get("chat_id").and_then(|v| v.as_str()),
                );
                if let (Some(token), Some(chat_id)) = (token, chat_id) {
                    let url = format!("https://api.telegram.org/bot{}/sendMessage", token);
                    client
                        .post(&url)
                        .json(&serde_json::json!({"chat_id": chat_id, "text": text}))
                        .send()
                        .await
                        .map(|_| ())
                        .map_err(|e| e.to_string())
                } else {
                    Err("missing bot_token/chat_id".into())
                }
            }
            _ => Err(format!("unsupported notif type {}", ntype)),
        };
        if let Err(e) = result {
            tracing::warn!("notification {} failed: {}", id, e);
        }
    }
}
