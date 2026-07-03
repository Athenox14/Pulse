# Pulse

API-first uptime and status monitoring, written in Rust (axum + sqlx/SQLite + tokio). Same feature set you'd expect from a self-hosted monitoring tool (HTTP/TCP/DNS checks, notifications, status pages), no bundled dashboard required to run it — everything is a REST/WebSocket API, with a minimal HTML page in `/ui` for manual testing.

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
- `PULSE_JWT_SECRET`

## API

- `GET/POST /api/monitors`, `GET/PUT/DELETE /api/monitors/:id`
- `POST /api/monitors/:id/pause|resume|check`
- `GET /api/monitors/:id/heartbeats`, `GET /api/monitors/:id/uptime`
- `GET/POST /api/notifications`, `DELETE /api/notifications/:id`
- `GET/POST /api/status-pages`, `GET /api/status-pages/:slug` (public)
- `GET/POST /api/maintenance`, `DELETE /api/maintenance/:id`
- `WS /ws` — realtime heartbeat feed

## Test UI

Minimal dashboard at `/ui/index.html` (served statically) — add monitors, watch heartbeats live, pause/delete. Not meant for production, just to exercise the API visually.

## Not yet implemented (future work)

- Auth (users table scaffolded, no login endpoints wired yet)
- Push-type "passive" monitors
- Certificate expiry checks
- 2FA, groups/tags, multi-language
