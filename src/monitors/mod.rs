use crate::models::*;
use crate::state::AppState;
use chrono::Utc;
use std::time::Duration;
use tokio::net::TcpStream;
use tokio::time::timeout;

pub async fn run_scheduler(state: AppState) {
    loop {
        let monitors: Vec<(String, i64)> = match sqlx::query_as::<_, (String, i64)>(
            "SELECT id, interval_sec FROM monitors WHERE active = 1",
        )
        .fetch_all(&state.db)
        .await
        {
            Ok(m) => m,
            Err(e) => {
                tracing::error!("scheduler query failed: {e}");
                tokio::time::sleep(Duration::from_secs(5)).await;
                continue;
            }
        };

        for (id, _interval) in monitors {
            let due = {
                let mut due_set = state.due_at.lock().await;
                let now = tokio::time::Instant::now();
                match due_set.get(&id) {
                    Some(t) if *t > now => false,
                    _ => {
                        due_set.insert(id.clone(), now + Duration::from_secs(60));
                        true
                    }
                }
            };
            if due {
                let st = state.clone();
                let mid = id.clone();
                tokio::spawn(async move {
                    check_and_schedule(st, mid).await;
                });
            }
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}

async fn check_and_schedule(state: AppState, monitor_id: String) {
    let monitor = match fetch_monitor(&state, &monitor_id).await {
        Some(m) => m,
        None => return,
    };
    run_check(&state, &monitor).await;
    let mut due_set = state.due_at.lock().await;
    due_set.insert(
        monitor_id,
        tokio::time::Instant::now() + Duration::from_secs(monitor.interval_sec.max(1) as u64),
    );
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
        let result = do_check(m).await;
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
        crate::notify::dispatch(state, m, status, &msg).await;
    }
}

async fn do_check(m: &Monitor) -> Result<String, String> {
    match m.monitor_type.as_str() {
        "http" | "keyword" | "json" => check_http(m).await,
        "tcp" => check_tcp(m).await,
        "ping" => check_ping(m).await,
        "dns" => check_dns(m).await,
        _ => Err("unknown monitor type".into()),
    }
}

async fn check_http(m: &Monitor) -> Result<String, String> {
    let url = m.url.clone().ok_or("missing url")?;
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(m.timeout_sec.max(1) as u64))
        .redirect(reqwest::redirect::Policy::limited(m.max_redirects.max(0) as usize))
        .build()
        .map_err(|e| e.to_string())?;
    let method = m.method.clone().unwrap_or_else(|| "GET".into());
    let req = client.request(
        method.parse().unwrap_or(reqwest::Method::GET),
        &url,
    );
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
