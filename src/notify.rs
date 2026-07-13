use crate::models::{Monitor, STATUS_DOWN, STATUS_MAINTENANCE, STATUS_PENDING, STATUS_UP};
use crate::state::AppState;
use chrono::Utc;
use serde_json::{json, Value};

const COLOR_UP: i64 = 0x2ECC71;
const COLOR_DOWN: i64 = 0xE74C3C;
const COLOR_PENDING: i64 = 0xF1C40F;
const COLOR_MAINTENANCE: i64 = 0x3498DB;

pub async fn dispatch(state: &AppState, m: &Monitor, status: i64, msg: &str, ping_ms: Option<i64>) {
    let ids: Vec<String> = m
        .notification_ids
        .as_deref()
        .and_then(|s| serde_json::from_str(s).ok())
        .unwrap_or_default();
    if ids.is_empty() {
        return;
    }
    let status_text = if status == STATUS_UP { "UP" } else { "DOWN" };
    let text = format!("[Pulse] {} is {} - {}", m.name, status_text, msg);
    let client = &state.http_client;

    for id in ids {
        let row: Option<(String, String)> =
            sqlx::query_as("SELECT type, config FROM notifications WHERE id = ? AND active = 1")
                .bind(&id)
                .fetch_optional(&state.db)
                .await
                .ok()
                .flatten();
        let Some((ntype, config)) = row else { continue };
        let cfg: Value = serde_json::from_str(&config).unwrap_or_default();
        let result = match ntype.as_str() {
            "webhook" => {
                if let Some(url) = cfg.get("url").and_then(|v| v.as_str()) {
                    post_json(client, url, &json!({"monitor": m.name, "status": status_text, "msg": msg})).await
                } else {
                    Err("missing url".into())
                }
            }
            "discord" => send_discord(client, &cfg, m, status, msg, ping_ms, &text).await,
            "slack" => {
                if let Some(url) = cfg.get("webhook_url").and_then(|v| v.as_str()) {
                    post_json(client, url, &json!({"text": text})).await
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
                    post_json(client, &url, &json!({"chat_id": chat_id, "text": text})).await
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

/// Builds and sends a Discord webhook payload. By default renders a rich
/// embed (title/color/fields) like Uptime Kuma; `use_embed: false` in the
/// notification config falls back to the old plain-text message. `content`,
/// `username`, and `avatar_url` are user-configurable so a channel can e.g.
/// mention a role (`"content": "<@&123456789012345678>"`) above the embed.
async fn send_discord(
    client: &reqwest::Client,
    cfg: &Value,
    m: &Monitor,
    status: i64,
    msg: &str,
    ping_ms: Option<i64>,
    fallback_text: &str,
) -> Result<(), String> {
    let url = cfg
        .get("webhook_url")
        .and_then(|v| v.as_str())
        .ok_or("missing webhook_url")?;
    let use_embed = cfg.get("use_embed").and_then(|v| v.as_bool()).unwrap_or(true);

    let mut payload = json!({});
    if let Some(content) = cfg.get("content").and_then(|v| v.as_str()) {
        payload["content"] = json!(content);
    } else if !use_embed {
        payload["content"] = json!(fallback_text);
    }
    if let Some(username) = cfg.get("username").and_then(|v| v.as_str()) {
        payload["username"] = json!(username);
    }
    if let Some(avatar) = cfg.get("avatar_url").and_then(|v| v.as_str()) {
        payload["avatar_url"] = json!(avatar);
    }
    if use_embed {
        payload["embeds"] = json!([build_embed(m, status, msg, ping_ms)]);
    }

    post_json(client, url, &payload).await
}

fn build_embed(m: &Monitor, status: i64, msg: &str, ping_ms: Option<i64>) -> Value {
    let (emoji, verb, color) = match status {
        STATUS_UP => ("✅", "is up!", COLOR_UP),
        STATUS_DOWN => ("❌", "went down.", COLOR_DOWN),
        STATUS_PENDING => ("⏳", "is pending.", COLOR_PENDING),
        STATUS_MAINTENANCE => ("🔧", "is under maintenance.", COLOR_MAINTENANCE),
        _ => ("ℹ️", "status changed.", COLOR_PENDING),
    };
    let title = format!("{} Your service {} {} {}", emoji, m.name, verb, emoji);

    let mut fields = vec![json!({"name": "Service Name", "value": m.name, "inline": true})];

    let location = m.url.clone().or_else(|| {
        m.hostname.clone().map(|h| match m.port {
            Some(p) => format!("{}:{}", h, p),
            None => h,
        })
    });
    if let Some(loc) = location {
        fields.push(json!({"name": "Service URL", "value": loc, "inline": true}));
    }

    fields.push(json!({
        "name": "Time (UTC)",
        "value": Utc::now().format("%Y-%m-%d %H:%M:%S").to_string(),
        "inline": false
    }));

    if status == STATUS_UP {
        if let Some(p) = ping_ms {
            fields.push(json!({"name": "Ping", "value": format!("{} ms", p), "inline": true}));
        }
    } else if status == STATUS_DOWN && !msg.is_empty() {
        fields.push(json!({"name": "Error", "value": msg, "inline": false}));
    }

    json!({
        "title": title,
        "color": color,
        "fields": fields,
        "timestamp": Utc::now().to_rfc3339(),
    })
}

async fn post_json(client: &reqwest::Client, url: &str, payload: &Value) -> Result<(), String> {
    let resp = client
        .post(url)
        .json(payload)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let code = resp.status();
    if code.is_success() {
        Ok(())
    } else {
        let body = resp.text().await.unwrap_or_default();
        Err(format!("http {}: {}", code, body))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn monitor(url: Option<&str>) -> Monitor {
        Monitor {
            id: "m1".into(),
            name: "OxaDash".into(),
            monitor_type: "http".into(),
            url: url.map(String::from),
            hostname: None,
            port: None,
            interval_sec: 60,
            retries: 0,
            retry_interval_sec: 60,
            timeout_sec: 30,
            keyword: None,
            expected_status: None,
            method: None,
            headers: None,
            body: None,
            active: true,
            upside_down: false,
            max_redirects: 10,
            notification_ids: None,
            created_at: Utc::now(),
        }
    }

    #[test]
    fn embed_down_has_error_field_and_red_color() {
        let m = monitor(Some("https://oxadash.fr/"));
        let embed = build_embed(&m, STATUS_DOWN, "Request failed with status code 502", Some(120));
        assert_eq!(embed["color"], COLOR_DOWN);
        assert!(embed["title"].as_str().unwrap().contains("went down"));
        let fields = embed["fields"].as_array().unwrap();
        assert!(fields.iter().any(|f| f["name"] == "Service URL" && f["value"] == "https://oxadash.fr/"));
        assert!(fields.iter().any(|f| f["name"] == "Error" && f["value"] == "Request failed with status code 502"));
        assert!(!fields.iter().any(|f| f["name"] == "Ping"));
    }

    #[test]
    fn embed_up_has_ping_field_and_green_color() {
        let m = monitor(Some("https://oxadash.fr/"));
        let embed = build_embed(&m, STATUS_UP, "200 OK", Some(1344));
        assert_eq!(embed["color"], COLOR_UP);
        assert!(embed["title"].as_str().unwrap().contains("is up!"));
        let fields = embed["fields"].as_array().unwrap();
        assert!(fields.iter().any(|f| f["name"] == "Ping" && f["value"] == "1344 ms"));
        assert!(!fields.iter().any(|f| f["name"] == "Error"));
    }

    #[test]
    fn embed_falls_back_to_hostname_when_no_url() {
        let mut m = monitor(None);
        m.hostname = Some("db.internal".into());
        m.port = Some(5432);
        let embed = build_embed(&m, STATUS_DOWN, "connection refused", None);
        let fields = embed["fields"].as_array().unwrap();
        assert!(fields.iter().any(|f| f["name"] == "Service URL" && f["value"] == "db.internal:5432"));
    }
}
