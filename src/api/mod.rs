use axum::{
    extract::{ws::WebSocketUpgrade, ws::Message, Path, State},
    response::{IntoResponse, Json},
    routing::{get, post, delete},
    Router,
};
use serde_json::json;
use crate::models::*;
use crate::state::AppState;
use chrono::Utc;

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/api/health", get(health))
        .route("/api/monitors", get(list_monitors).post(create_monitor))
        .route("/api/monitors/:id", get(get_monitor).put(update_monitor).delete(delete_monitor))
        .route("/api/monitors/:id/pause", post(pause_monitor))
        .route("/api/monitors/:id/resume", post(resume_monitor))
        .route("/api/monitors/:id/heartbeats", get(get_heartbeats))
        .route("/api/monitors/:id/uptime", get(get_uptime))
        .route("/api/monitors/:id/check", post(force_check))
        .route("/api/notifications", get(list_notifications).post(create_notification))
        .route("/api/notifications/:id", delete(delete_notification))
        .route("/api/status-pages", get(list_status_pages).post(create_status_page))
        .route("/api/status-pages/:slug", get(get_status_page))
        .route("/api/maintenance", get(list_maintenance).post(create_maintenance))
        .route("/api/maintenance/:id", delete(delete_maintenance))
        .route("/ws", get(ws_handler))
        .with_state(state)
}

async fn health() -> impl IntoResponse {
    Json(json!({"status": "ok", "service": "pulse"}))
}

async fn list_monitors(State(state): State<AppState>) -> impl IntoResponse {
    let rows = sqlx::query_as::<_, MonitorDb>("SELECT * FROM monitors ORDER BY created_at DESC")
        .fetch_all(&state.db)
        .await
        .unwrap_or_default();
    Json(rows.into_iter().map(|r| r.into_monitor()).collect::<Vec<Monitor>>())
}

async fn create_monitor(State(state): State<AppState>, Json(input): Json<CreateMonitor>) -> impl IntoResponse {
    let id = new_id();
    let notif_ids = input.notification_ids.clone();
    let res = sqlx::query(
        "INSERT INTO monitors (id,name,type,url,hostname,port,interval_sec,retries,retry_interval_sec,timeout_sec,keyword,expected_status,method,headers,body,active,upside_down,max_redirects,notification_ids)
         VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)"
    )
    .bind(&id).bind(&input.name).bind(&input.monitor_type).bind(&input.url).bind(&input.hostname)
    .bind(input.port).bind(input.interval_sec).bind(input.retries).bind(input.retry_interval_sec)
    .bind(input.timeout_sec).bind(&input.keyword).bind(input.expected_status).bind(&input.method)
    .bind(&input.headers).bind(&input.body).bind(input.active).bind(input.upside_down)
    .bind(input.max_redirects).bind(&notif_ids)
    .execute(&state.db).await;

    match res {
        Ok(_) => {
            if input.active {
                crate::monitors::spawn_monitor_task(&state, id.clone()).await;
            }
            (axum::http::StatusCode::CREATED, Json(json!({"id": id}))).into_response()
        }
        Err(e) => (axum::http::StatusCode::BAD_REQUEST, Json(json!({"error": e.to_string()}))).into_response(),
    }
}

async fn get_monitor(State(state): State<AppState>, Path(id): Path<String>) -> impl IntoResponse {
    match crate::monitors::fetch_monitor(&state, &id).await {
        Some(m) => Json(m).into_response(),
        None => axum::http::StatusCode::NOT_FOUND.into_response(),
    }
}

async fn update_monitor(State(state): State<AppState>, Path(id): Path<String>, Json(input): Json<CreateMonitor>) -> impl IntoResponse {
    let res = sqlx::query(
        "UPDATE monitors SET name=?,type=?,url=?,hostname=?,port=?,interval_sec=?,retries=?,retry_interval_sec=?,timeout_sec=?,keyword=?,expected_status=?,method=?,headers=?,body=?,active=?,upside_down=?,max_redirects=?,notification_ids=? WHERE id=?"
    )
    .bind(&input.name).bind(&input.monitor_type).bind(&input.url).bind(&input.hostname)
    .bind(input.port).bind(input.interval_sec).bind(input.retries).bind(input.retry_interval_sec)
    .bind(input.timeout_sec).bind(&input.keyword).bind(input.expected_status).bind(&input.method)
    .bind(&input.headers).bind(&input.body).bind(input.active).bind(input.upside_down)
    .bind(input.max_redirects).bind(&input.notification_ids).bind(&id)
    .execute(&state.db).await;
    match res {
        Ok(_) => {
            if input.active {
                crate::monitors::spawn_monitor_task(&state, id.clone()).await;
            } else {
                crate::monitors::stop_monitor_task(&state, &id).await;
            }
            Json(json!({"ok": true})).into_response()
        }
        Err(e) => (axum::http::StatusCode::BAD_REQUEST, Json(json!({"error": e.to_string()}))).into_response(),
    }
}

async fn delete_monitor(State(state): State<AppState>, Path(id): Path<String>) -> impl IntoResponse {
    crate::monitors::stop_monitor_task(&state, &id).await;
    let _ = sqlx::query("DELETE FROM monitors WHERE id = ?").bind(&id).execute(&state.db).await;
    let _ = sqlx::query("DELETE FROM heartbeats WHERE monitor_id = ?").bind(&id).execute(&state.db).await;
    Json(json!({"ok": true}))
}

async fn pause_monitor(State(state): State<AppState>, Path(id): Path<String>) -> impl IntoResponse {
    let _ = sqlx::query("UPDATE monitors SET active = 0 WHERE id = ?").bind(&id).execute(&state.db).await;
    crate::monitors::stop_monitor_task(&state, &id).await;
    Json(json!({"ok": true}))
}

async fn resume_monitor(State(state): State<AppState>, Path(id): Path<String>) -> impl IntoResponse {
    let _ = sqlx::query("UPDATE monitors SET active = 1 WHERE id = ?").bind(&id).execute(&state.db).await;
    crate::monitors::spawn_monitor_task(&state, id).await;
    Json(json!({"ok": true}))
}

async fn force_check(State(state): State<AppState>, Path(id): Path<String>) -> impl IntoResponse {
    if let Some(m) = crate::monitors::fetch_monitor(&state, &id).await {
        let st = state.clone();
        tokio::spawn(async move { crate::monitors::run_check(&st, &m).await; });
        Json(json!({"ok": true})).into_response()
    } else {
        axum::http::StatusCode::NOT_FOUND.into_response()
    }
}

async fn get_heartbeats(State(state): State<AppState>, Path(id): Path<String>) -> impl IntoResponse {
    let rows = sqlx::query_as::<_, HeartbeatDb>(
        "SELECT * FROM heartbeats WHERE monitor_id = ? ORDER BY time DESC LIMIT 200",
    )
    .bind(&id)
    .fetch_all(&state.db)
    .await
    .unwrap_or_default();
    Json(rows.into_iter().map(|r| r.into_hb()).collect::<Vec<Heartbeat>>())
}

async fn get_uptime(State(state): State<AppState>, Path(id): Path<String>) -> impl IntoResponse {
    async fn ratio(state: &AppState, id: &str, hours: i64) -> f64 {
        let row: Option<(i64, i64)> = sqlx::query_as(
            "SELECT COUNT(*), SUM(CASE WHEN status = 1 THEN 1 ELSE 0 END) FROM heartbeats WHERE monitor_id = ? AND time >= datetime('now', ?)"
        )
        .bind(id)
        .bind(format!("-{} hours", hours))
        .fetch_optional(&state.db)
        .await
        .ok()
        .flatten();
        match row {
            Some((total, up)) if total > 0 => up as f64 / total as f64,
            _ => 0.0,
        }
    }
    let u24 = ratio(&state, &id, 24).await;
    let u168 = ratio(&state, &id, 168).await;
    let u720 = ratio(&state, &id, 720).await;
    Json(json!({"uptime_24h": u24, "uptime_7d": u168, "uptime_30d": u720}))
}

async fn list_notifications(State(state): State<AppState>) -> impl IntoResponse {
    let rows = sqlx::query_as::<_, Notification>("SELECT * FROM notifications")
        .fetch_all(&state.db)
        .await
        .unwrap_or_default();
    Json(rows)
}

async fn create_notification(State(state): State<AppState>, Json(input): Json<CreateNotification>) -> impl IntoResponse {
    let id = new_id();
    let cfg = input.config.to_string();
    let res = sqlx::query("INSERT INTO notifications (id,name,type,config,active) VALUES (?,?,?,?,?)")
        .bind(&id).bind(&input.name).bind(&input.notif_type).bind(&cfg).bind(input.active)
        .execute(&state.db).await;
    match res {
        Ok(_) => (axum::http::StatusCode::CREATED, Json(json!({"id": id}))).into_response(),
        Err(e) => (axum::http::StatusCode::BAD_REQUEST, Json(json!({"error": e.to_string()}))).into_response(),
    }
}

async fn delete_notification(State(state): State<AppState>, Path(id): Path<String>) -> impl IntoResponse {
    let _ = sqlx::query("DELETE FROM notifications WHERE id = ?").bind(&id).execute(&state.db).await;
    Json(json!({"ok": true}))
}

async fn list_status_pages(State(state): State<AppState>) -> impl IntoResponse {
    let rows = sqlx::query_as::<_, StatusPage>("SELECT * FROM status_pages")
        .fetch_all(&state.db)
        .await
        .unwrap_or_default();
    Json(rows)
}

async fn create_status_page(State(state): State<AppState>, Json(input): Json<CreateStatusPage>) -> impl IntoResponse {
    let id = new_id();
    let mids = serde_json::to_string(&input.monitor_ids).unwrap_or("[]".into());
    let res = sqlx::query("INSERT INTO status_pages (id,slug,title,description,monitor_ids,published) VALUES (?,?,?,?,?,?)")
        .bind(&id).bind(&input.slug).bind(&input.title).bind(&input.description).bind(&mids).bind(input.published)
        .execute(&state.db).await;
    match res {
        Ok(_) => (axum::http::StatusCode::CREATED, Json(json!({"id": id}))).into_response(),
        Err(e) => (axum::http::StatusCode::BAD_REQUEST, Json(json!({"error": e.to_string()}))).into_response(),
    }
}

async fn get_status_page(State(state): State<AppState>, Path(slug): Path<String>) -> impl IntoResponse {
    let page = sqlx::query_as::<_, StatusPage>("SELECT * FROM status_pages WHERE slug = ? AND published = 1")
        .bind(&slug)
        .fetch_optional(&state.db)
        .await
        .ok()
        .flatten();
    let Some(page) = page else { return axum::http::StatusCode::NOT_FOUND.into_response() };
    let mids: Vec<String> = serde_json::from_str(&page.monitor_ids).unwrap_or_default();
    let mut monitors = Vec::new();
    for id in mids {
        if let Some(m) = crate::monitors::fetch_monitor(&state, &id).await {
            let latest: Option<HeartbeatDb> = sqlx::query_as(
                "SELECT * FROM heartbeats WHERE monitor_id = ? ORDER BY time DESC LIMIT 1",
            )
            .bind(&id)
            .fetch_optional(&state.db)
            .await
            .ok()
            .flatten();
            monitors.push(json!({
                "id": m.id, "name": m.name,
                "status": latest.map(|h| h.status).unwrap_or(2)
            }));
        }
    }
    Json(json!({"title": page.title, "description": page.description, "monitors": monitors})).into_response()
}

async fn list_maintenance(State(state): State<AppState>) -> impl IntoResponse {
    let rows = sqlx::query_as::<_, MaintenanceDb>("SELECT * FROM maintenance_windows")
        .fetch_all(&state.db)
        .await
        .unwrap_or_default();
    Json(rows.into_iter().map(|r| r.into_mw()).collect::<Vec<MaintenanceWindow>>())
}

#[derive(serde::Deserialize)]
struct CreateMaintenance {
    title: String,
    monitor_ids: Vec<String>,
    start_time: chrono::DateTime<Utc>,
    end_time: chrono::DateTime<Utc>,
}

async fn create_maintenance(State(state): State<AppState>, Json(input): Json<CreateMaintenance>) -> impl IntoResponse {
    let id = new_id();
    let mids = serde_json::to_string(&input.monitor_ids).unwrap_or("[]".into());
    let res = sqlx::query("INSERT INTO maintenance_windows (id,title,monitor_ids,start_time,end_time,active) VALUES (?,?,?,?,?,1)")
        .bind(&id).bind(&input.title).bind(&mids).bind(input.start_time).bind(input.end_time)
        .execute(&state.db).await;
    match res {
        Ok(_) => (axum::http::StatusCode::CREATED, Json(json!({"id": id}))).into_response(),
        Err(e) => (axum::http::StatusCode::BAD_REQUEST, Json(json!({"error": e.to_string()}))).into_response(),
    }
}

async fn delete_maintenance(State(state): State<AppState>, Path(id): Path<String>) -> impl IntoResponse {
    let _ = sqlx::query("DELETE FROM maintenance_windows WHERE id = ?").bind(&id).execute(&state.db).await;
    Json(json!({"ok": true}))
}

async fn ws_handler(ws: WebSocketUpgrade, State(state): State<AppState>) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, state))
}

async fn handle_socket(mut socket: axum::extract::ws::WebSocket, state: AppState) {
    let mut rx = state.ws_tx.subscribe();
    loop {
        tokio::select! {
            msg = rx.recv() => {
                match msg {
                    Ok(text) => { if socket.send(Message::Text(text)).await.is_err() { break; } }
                    Err(_) => continue,
                }
            }
            incoming = socket.recv() => {
                match incoming {
                    Some(Ok(_)) => continue,
                    _ => break,
                }
            }
        }
    }
}

#[derive(sqlx::FromRow)]
struct MonitorDb {
    id: String, name: String, r#type: String, url: Option<String>, hostname: Option<String>,
    port: Option<i64>, interval_sec: i64, retries: i64, retry_interval_sec: i64, timeout_sec: i64,
    keyword: Option<String>, expected_status: Option<i64>, method: Option<String>, headers: Option<String>,
    body: Option<String>, active: bool, upside_down: bool, max_redirects: i64,
    notification_ids: Option<String>, created_at: chrono::DateTime<Utc>,
}
impl MonitorDb {
    fn into_monitor(self) -> Monitor {
        Monitor {
            id: self.id, name: self.name, monitor_type: self.r#type, url: self.url, hostname: self.hostname,
            port: self.port, interval_sec: self.interval_sec, retries: self.retries,
            retry_interval_sec: self.retry_interval_sec, timeout_sec: self.timeout_sec, keyword: self.keyword,
            expected_status: self.expected_status, method: self.method, headers: self.headers, body: self.body,
            active: self.active, upside_down: self.upside_down, max_redirects: self.max_redirects,
            notification_ids: self.notification_ids, created_at: self.created_at,
        }
    }
}

#[derive(sqlx::FromRow)]
struct HeartbeatDb {
    id: String, monitor_id: String, status: i64, msg: Option<String>, ping_ms: Option<i64>,
    important: bool, time: chrono::DateTime<Utc>,
}
impl HeartbeatDb {
    fn into_hb(self) -> Heartbeat {
        Heartbeat { id: self.id, monitor_id: self.monitor_id, status: self.status, msg: self.msg,
            ping_ms: self.ping_ms, important: self.important, time: self.time }
    }
}

#[derive(sqlx::FromRow)]
struct MaintenanceDb {
    id: String, title: String, monitor_ids: String, start_time: chrono::DateTime<Utc>,
    end_time: chrono::DateTime<Utc>, active: bool,
}
impl MaintenanceDb {
    fn into_mw(self) -> MaintenanceWindow {
        MaintenanceWindow { id: self.id, title: self.title, monitor_ids: self.monitor_ids,
            start_time: self.start_time, end_time: self.end_time, active: self.active }
    }
}
