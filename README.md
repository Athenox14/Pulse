# Pulse

Uptime Kuma-style monitoring, API-first, written in Rust (axum + sqlx/SQLite + tokio).

## Features (Kuma parity)

- Monitor types: HTTP(s), keyword-in-body, TCP port, ping (reachability), DNS resolve
- Configurable interval, retries, retry interval, timeout, expected status code, HTTP method/headers/body
- Upside-down mode (invert up/down)
- Heartbeat history per monitor, uptime % (24h / 7d / 30d)
- Pause/resume monitors, force check-now
- Notifications: webhook, Discord, Slack, Telegram (fires on status change only)
- Public status pages (grouped monitors, published/unpublished)
- Maintenance windows
- Realtime updates via WebSocket (`/ws`) broadcasting heartbeat events
- SQLite storage, zero external services required

## Run

```
cargo run
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
