use crate::models::*;
use crate::state::AppState;
use chrono::Utc;
use rand::Rng;
use std::time::Duration;
use tokio::net::TcpStream;
use tokio::time::timeout;

/// Spawns one long-lived task per active monitor at startup. Each task sleeps
/// between checks instead of the whole scheduler polling the DB every second
/// (a full-table scan per tick that scaled badly past a few hundred monitors).
/// Startup checks are jittered across each monitor's own interval so 1000
/// monitors don't all fire (and later re-fire, in lockstep) at once.
pub async fn spawn_all(state: &AppState) {
    let rows: Vec<(String, i64)> =
        sqlx::query_as("SELECT id, interval_sec FROM monitors WHERE active = 1")
            .fetch_all(&state.db)
            .await
            .unwrap_or_default();
    for (id, interval_sec) in rows {
        let jitter = rand::thread_rng().gen_range(0..interval_sec.max(1) as u64);
        spawn_monitor_task_with_delay(state, id, Duration::from_secs(jitter)).await;
    }
}

pub async fn spawn_monitor_task(state: &AppState, monitor_id: String) {
    spawn_monitor_task_with_delay(state, monitor_id, Duration::ZERO).await;
}

async fn spawn_monitor_task_with_delay(state: &AppState, monitor_id: String, delay: Duration) {
    let mut tasks = state.tasks.lock().await;
    if let Some(old) = tasks.remove(&monitor_id) {
        old.abort();
    }
    let st = state.clone();
    let mid = monitor_id.clone();
    let handle = tokio::spawn(async move {
        if !delay.is_zero() {
            tokio::time::sleep(delay).await;
        }
        monitor_loop(st, mid).await
    });
    tasks.insert(monitor_id, handle);
}

pub async fn stop_monitor_task(state: &AppState, monitor_id: &str) {
    let mut tasks = state.tasks.lock().await;
    if let Some(handle) = tasks.remove(monitor_id) {
        handle.abort();
    }
}

async fn monitor_loop(state: AppState, monitor_id: String) {
    loop {
        let monitor = match fetch_monitor(&state, &monitor_id).await {
            Some(m) if m.active => m,
            _ => return, // deleted, paused, or gone: task ends, no more DB polling for it
        };
        run_check(&state, &monitor).await;
        tokio::time::sleep(Duration::from_secs(monitor.interval_sec.max(1) as u64)).await;
    }
}

pub async fn fetch_monitor(state: &AppState, id: &str) -> Option<Monitor> {
    sqlx::query_as::<_, MonitorRow>("SELECT * FROM monitors WHERE id = ?")
        .bind(id)
        .fetch_optional(&state.db)
        .await
        .ok()
        .flatten()
        .map(|r| r.into())
}

#[derive(sqlx::FromRow)]
struct MonitorRow {
    id: String,
    name: String,
    r#type: String,
    url: Option<String>,
    hostname: Option<String>,
    port: Option<i64>,
    interval_sec: i64,
    retries: i64,
    retry_interval_sec: i64,
    timeout_sec: i64,
    keyword: Option<String>,
    expected_status: Option<i64>,
    method: Option<String>,
    headers: Option<String>,
    body: Option<String>,
    active: bool,
    upside_down: bool,
    max_redirects: i64,
    notification_ids: Option<String>,
    created_at: chrono::DateTime<Utc>,
}

impl From<MonitorRow> for Monitor {
    fn from(r: MonitorRow) -> Self {
        Monitor {
            id: r.id,
            name: r.name,
            monitor_type: r.r#type,
            url: r.url,
            hostname: r.hostname,
            port: r.port,
            interval_sec: r.interval_sec,
            retries: r.retries,
            retry_interval_sec: r.retry_interval_sec,
            timeout_sec: r.timeout_sec,
            keyword: r.keyword,
            expected_status: r.expected_status,
            method: r.method,
            headers: r.headers,
            body: r.body,
            active: r.active,
            upside_down: r.upside_down,
            max_redirects: r.max_redirects,
            notification_ids: r.notification_ids,
            created_at: r.created_at,
        }
    }
}

pub async fn run_check(state: &AppState, m: &Monitor) {
    let mut attempt = 0i64;
    let max_attempts = m.retries.max(0) + 1;
    let (mut status, mut msg, mut ping_ms) = (STATUS_DOWN, String::new(), None);

    while attempt < max_attempts {
        let start = std::time::Instant::now();
        let result = do_check(state, m).await;
        let elapsed = start.elapsed().as_millis() as i64;
        match result {
            Ok(m_ok) => {
                status = STATUS_UP;
                msg = m_ok;
                ping_ms = Some(elapsed);
                break;
            }
            Err(e) => {
                status = STATUS_DOWN;
                msg = e;
                ping_ms = Some(elapsed);
            }
        }
        attempt += 1;
        if attempt < max_attempts {
            tokio::time::sleep(Duration::from_secs(m.retry_interval_sec.max(1) as u64)).await;
        }
    }

    if m.upside_down {
        status = if status == STATUS_UP { STATUS_DOWN } else { STATUS_UP };
    }

    let prev_status: Option<i64> = sqlx::query_scalar(
        "SELECT status FROM heartbeats WHERE monitor_id = ? ORDER BY time DESC LIMIT 1",
    )
    .bind(&m.id)
    .fetch_optional(&state.db)
    .await
    .ok()
    .flatten();

    let important = prev_status.map(|p| p != status).unwrap_or(true);

    let hb_id = new_id();
    let _ = sqlx::query(
        "INSERT INTO heartbeats (id, monitor_id, status, msg, ping_ms, important, time) VALUES (?,?,?,?,?,?,?)",
    )
    .bind(&hb_id)
    .bind(&m.id)
    .bind(status)
    .bind(&msg)
    .bind(ping_ms)
    .bind(important)
    .bind(Utc::now())
    .execute(&state.db)
    .await;

    let hb = Heartbeat {
        id: hb_id,
        monitor_id: m.id.clone(),
        status,
        msg: Some(msg.clone()),
        ping_ms,
        important,
        time: Utc::now(),
    };
    let _ = state.ws_tx.send(serde_json::json!({"type":"heartbeat","data": hb}).to_string());

    if important {
        crate::notify::dispatch(state, m, status, &msg, ping_ms).await;
    }
}

async fn do_check(state: &AppState, m: &Monitor) -> Result<String, String> {
    match m.monitor_type.as_str() {
        "http" | "keyword" | "json" => check_http(state, m).await,
        "tcp" => check_tcp(m).await,
        "ping" => check_ping(m).await,
        "dns" => check_dns(m).await,
        _ => Err("unknown monitor type".into()),
    }
}

async fn check_http(state: &AppState, m: &Monitor) -> Result<String, String> {
    let url = m.url.clone().ok_or("missing url")?;
    let method = m.method.clone().unwrap_or_else(|| "GET".into());
    let req = state
        .http_client
        .request(method.parse().unwrap_or(reqwest::Method::GET), &url)
        .timeout(Duration::from_secs(m.timeout_sec.max(1) as u64));
    let req = if let Some(body) = &m.body {
        req.body(body.clone())
    } else {
        req
    };
    let resp = req.send().await.map_err(|e| e.to_string())?;
    let status_code = resp.status().as_u16();
    let expected = m.expected_status.unwrap_or(200) as u16;
    if m.monitor_type == "keyword" {
        let text = resp.text().await.map_err(|e| e.to_string())?;
        let kw = m.keyword.clone().unwrap_or_default();
        if !text.contains(&kw) {
            return Err(format!("keyword '{}' not found", kw));
        }
        return Ok(format!("{} OK, keyword found", status_code));
    }
    if status_code == expected || (m.expected_status.is_none() && resp.status().is_success()) {
        Ok(format!("{} OK", status_code))
    } else {
        Err(format!("expected {}, got {}", expected, status_code))
    }
}

async fn check_tcp(m: &Monitor) -> Result<String, String> {
    let host = m.hostname.clone().ok_or("missing hostname")?;
    let port = m.port.ok_or("missing port")?;
    let addr = format!("{}:{}", host, port);
    match timeout(Duration::from_secs(m.timeout_sec.max(1) as u64), TcpStream::connect(&addr)).await {
        Ok(Ok(_)) => Ok("TCP connect OK".into()),
        Ok(Err(e)) => Err(e.to_string()),
        Err(_) => Err("timeout".into()),
    }
}

async fn check_ping(m: &Monitor) -> Result<String, String> {
    // fallback: TCP-style reachability via connect on port 80 if no port given, else ICMP not available without raw sockets.
    let host = m.hostname.clone().ok_or("missing hostname")?;
    let addr = format!("{}:{}", host, m.port.unwrap_or(80));
    match timeout(Duration::from_secs(m.timeout_sec.max(1) as u64), TcpStream::connect(&addr)).await {
        Ok(Ok(_)) => Ok("host reachable".into()),
        Ok(Err(e)) => Err(e.to_string()),
        Err(_) => Err("timeout".into()),
    }
}

async fn check_dns(m: &Monitor) -> Result<String, String> {
    use trust_dns_resolver::TokioAsyncResolver;
    let host = m.hostname.clone().ok_or("missing hostname")?;
    let resolver = TokioAsyncResolver::tokio_from_system_conf().map_err(|e| e.to_string())?;
    let resp = resolver.lookup_ip(host.as_str()).await.map_err(|e| e.to_string())?;
    let ips: Vec<String> = resp.iter().map(|i| i.to_string()).collect();
    if ips.is_empty() {
        Err("no records".into())
    } else {
        Ok(format!("resolved: {}", ips.join(", ")))
    }
}
