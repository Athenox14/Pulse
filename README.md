# Pulse

API-first uptime and status monitoring, written in Rust (axum + sqlx/SQLite + tokio). Same feature set you'd expect from a self-hosted monitoring tool (HTTP/TCP/DNS checks, notifications, status pages), no bundled dashboard required to run it — everything is a REST/WebSocket API.

Actively used in production as the monitoring engine behind **OxaDash**, the hosting control panel from [OxalisHeberg](https://oxalisheberg.fr).

## Why

Most self-hosted monitoring tools ship as a Node.js app with a bundled SPA — fine at a small scale, but RAM and CPU climb fast once you're watching hundreds or thousands of endpoints. Pulse is built async-first in Rust specifically to stay cheap at scale: one lightweight task per monitor instead of a polling loop, a shared connection pool, WAL-mode SQLite, and no framework overhead from a frontend bundled into the same process.

## Real numbers (measured, not marketing)

Single instance, single core doing real work, checks running on real intervals:

| Monitors active | RAM (RSS) | CPU (avg, sustained) |
|---|---|---|
| 1,001 | 32.6 MB | ~9s CPU accumulated per steady-state sample |
| 10,530 | 87.9 MB | ~20% of one logical core sustained |

Measured on a release build (`cargo build --release`) on Windows, `Get-Process` RSS + wall-clock CPU sampling, monitors hitting real distinct domains on 300s intervals with jittered startup to avoid thundering-herd bursts.

## How it compares

| Tool | Stack | Typical RAM | Notes |
|---|---|---|---|
| **Pulse** | Rust / tokio / SQLite | ~33 MB @ 1k monitors, ~88 MB @ 10k monitors | measured above |
| [Uptime Kuma](https://github.com/louislam/uptime-kuma) | Node.js / SQLite | ~700 MB physical, virtual memory reported up to 21.3 GB with ~99 ping monitors | 88.6k GitHub stars; users report memory growing and struggling past ~500 monitors ([GitHub issue](https://github.com/louislam/uptime-kuma/issues/3039), [GitHub issue](https://github.com/louislam/uptime-kuma/issues/5654)) |
| [Gatus](https://github.com/TwiN/gatus) | Go | ~10 MB idle, ~30-40 MB in use | YAML/config-as-code, no built-in DB persistence by default ([comparison source](https://openalternative.co/compare/gatus/vs/uptime-kuma)) |
| Beszel | Go | ~10 MB | lightweight server-monitoring agent ([source](https://instapods.com/blog/best-server-monitoring-tools/)) |
| Datadog agent | Various | ~500 MB | commercial SaaS agent, for reference ([source](https://instapods.com/blog/best-server-monitoring-tools/)) |

Pulse sits closer to the Go-based lightweight tools (Gatus, Beszel) in resource usage while keeping the batteries-included feature set of Uptime Kuma (GUI-optional REST API, notifications, status pages, maintenance windows) rather than requiring YAML-only configuration.

## Features

- Monitor types: HTTP(s), keyword-in-body, TCP port, ping (reachability), DNS resolve
- Configurable interval, retries, retry interval, timeout, expected status code, HTTP method/headers/body
- Upside-down mode (invert up/down)
- Heartbeat history per monitor, uptime % (24h / 7d / 30d)
- Pause/resume monitors, force check-now
- Notifications: webhook, Discord, Slack, Telegram (fires on status change only)
- Public status pages (grouped monitors, published/unpublished)
- Maintenance windows
- Realtime updates via WebSocket (`/ws`) broadcasting heartbeat events
- SQLite storage (WAL mode), zero external services required

## Run

```
cargo run --release
```

Env vars:
- `PULSE_ADDR` (default `0.0.0.0:3939`)
- `PULSE_DB` (default `pulse.db`)

## Prebuilt binaries

Don't want to compile it yourself? Grab a binary for Linux, Windows, or macOS from the [Releases page](../../releases).

## API

All request/response bodies are JSON. Base URL defaults to `http://localhost:3939`.

### Monitors

**`GET /api/monitors`** — list all monitors.

**`POST /api/monitors`** — create a monitor.

Body:
```json
{
  "name": "My API",
  "type": "http",
  "url": "https://example.com/health",
  "interval_sec": 60,
  "retries": 1,
  "retry_interval_sec": 30,
  "timeout_sec": 10,
  "expected_status": 200,
  "method": "GET",
  "headers": "{\"Authorization\": \"Bearer ...\"}",
  "body": null,
  "active": true,
  "upside_down": false,
  "max_redirects": 10,
  "notification_ids": "[\"<notification-id>\"]"
}
```
Only `name` and `type` are required; everything else has a default. `type` is one of `http`, `keyword`, `tcp`, `ping`, `dns`, `json`.
- `keyword` also requires `"keyword": "some text"` — the check fails if the response body doesn't contain it.
- `tcp` / `ping` use `hostname` + `port` instead of `url`.
- `dns` uses `hostname` and resolves it, failing if there are no records.

Returns `201` with `{"id": "<uuid>"}`.

**`GET /api/monitors/:id`** — fetch one monitor.

**`PUT /api/monitors/:id`** — update a monitor. Same body shape as create; replaces the monitor's config and restarts its check task with the new settings.

**`DELETE /api/monitors/:id`** — delete a monitor and its heartbeat history.

**`POST /api/monitors/:id/pause`** — stop checking (task is torn down, no more DB writes).

**`POST /api/monitors/:id/resume`** — restart checking.

**`POST /api/monitors/:id/check`** — force an immediate check outside the normal interval.

**`GET /api/monitors/:id/heartbeats`** — last 200 heartbeats, most recent first.

Response item:
```json
{
  "id": "...",
  "monitor_id": "...",
  "status": 1,
  "msg": "200 OK",
  "ping_ms": 174,
  "important": true,
  "time": "2026-07-03T19:15:45.367708Z"
}
```
`status`: `0` = down, `1` = up, `2` = pending, `3` = maintenance. `important` is true when this heartbeat's status differs from the previous one (that's also what triggers notifications).

**`GET /api/monitors/:id/uptime`** — uptime ratios:
```json
{ "uptime_24h": 1.0, "uptime_7d": 0.998, "uptime_30d": 0.995 }
```

### Notifications

**`GET /api/notifications`** — list notification channels.

**`POST /api/notifications`** — create one.

Body:
```json
{ "name": "team-discord", "type": "discord", "config": { "webhook_url": "https://discord.com/api/webhooks/..." }, "active": true }
```
`type` is one of `webhook`, `discord`, `slack`, `telegram`. `config` shape depends on `type`:
- `webhook`: `{"url": "..."}` — receives a POST with `{"monitor": name, "status": "UP"|"DOWN", "msg": "..."}`
- `discord`: see below
- `slack`: `{"webhook_url": "..."}`
- `telegram`: `{"bot_token": "...", "chat_id": "..."}`

Attach a notification to a monitor by putting its id in that monitor's `notification_ids` field (JSON array as a string, e.g. `"[\"<id>\"]"`). Notifications fire only when a monitor's status changes, not on every check.

**`GET /api/notifications/:id`** — fetch one channel.

**`PUT /api/notifications/:id`** — update a channel in place (same body shape as `POST`).

**`DELETE /api/notifications/:id`**

#### Discord embeds

By default `discord` notifications render a rich embed like Uptime Kuma's — a colored title (red/green/yellow/blue for down/up/pending/maintenance), and fields for service name, URL, time, and the error message or ping:

```json
{
  "name": "team-discord",
  "type": "discord",
  "config": {
    "webhook_url": "https://discord.com/api/webhooks/...",
    "content": "<@&123456789012345678>",
    "username": "Pulse",
    "avatar_url": "https://example.com/icon.png",
    "use_embed": true
  },
  "active": true
}
```

All fields except `webhook_url` are optional:
- `content`: plain text posted above the embed, e.g. a role/`@here`/`@everyone` mention (`"<@&ROLE_ID>"`, `"@here"`). Omit for no extra text.
- `username` / `avatar_url`: override the webhook's default name/avatar for this message.
- `use_embed`: set to `false` to fall back to the old plain-text message (`content` only, no embed) instead of the rich embed.

### Status pages

**`GET /api/status-pages`** — list configured status pages (admin view).

**`POST /api/status-pages`**

Body:
```json
{ "slug": "public", "title": "Service Status", "description": "Live status of our services", "monitor_ids": ["<id1>", "<id2>"], "published": true }
```

**`GET /api/status-pages/:slug`** — public endpoint (no auth), returns only published pages:
```json
{ "title": "Service Status", "description": "...", "monitors": [{ "id": "...", "name": "My API", "status": 1 }] }
```

### Maintenance windows

**`GET /api/maintenance`** — list windows.

**`POST /api/maintenance`**
```json
{ "title": "Scheduled DB migration", "monitor_ids": ["<id>"], "start_time": "2026-07-10T02:00:00Z", "end_time": "2026-07-10T04:00:00Z" }
```

**`DELETE /api/maintenance/:id`**

### Realtime

**`WS /ws`** — subscribe for live updates. Every heartbeat broadcasts:
```json
{ "type": "heartbeat", "data": { "id": "...", "monitor_id": "...", "status": 1, "msg": "200 OK", "ping_ms": 174, "important": true, "time": "..." } }
```

### Health

**`GET /api/health`** — `{"status": "ok", "service": "pulse"}`, for load balancers / your own monitor-of-monitors.
