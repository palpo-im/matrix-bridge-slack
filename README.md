# matrix-bridge-slack

A Matrix <-> Slack bridge written in Rust.

[中文文档](README_CN.md)

Maintainer: `Palpo Team`  
Contact: `chris@acroidea.com`

## Overview

- Rust-only implementation
- Matrix appservice + Slack bot bridge core
- Slack Socket Mode for inbound events
- Slack Web API for outbound messaging, edits, file uploads, and lookups
- HTTP endpoints for health/status/metrics and provisioning
- Database backends: PostgreSQL, SQLite, and MySQL (feature-gated)

## Repository Layout

- `src/`: bridge implementation
- `config/config.sample.yaml`: sample configuration
- `Dockerfile`: multi-stage container build

## Prerequisites

- Rust toolchain (Docker build uses Rust 1.93)
- A Matrix homeserver configured for appservices
- A Slack app with:
  - bot token (`xoxb-...`)
  - app token (`xapp-...`) with `connections:write`
- Database: PostgreSQL, SQLite, or MySQL

## Quick Start (Local)

1. Create your config file:

```bash
cp config/config.sample.yaml config.yaml
```

2. Set required values in `config.yaml`:
   - `bridge.domain`
   - `auth.bot_token`
   - `auth.app_token`
   - `database.url` (or `database.conn_string` / `database.filename`)
   - registration values via either:
     - `registration.id`, `registration.as_token`, `registration.hs_token`, or
     - `slack-registration.yaml` next to your config file, or
     - env vars (see Environment Overrides below)

3. Run:

```bash
cargo check -p matrix-bridge-slack
cargo test -p matrix-bridge-slack --no-run
cargo run -p matrix-bridge-slack
```

4. Verify:

```bash
curl http://127.0.0.1:9005/health
curl http://127.0.0.1:9005/status
```

## Configure Slack (Step by Step)

1. Create a Slack app in your workspace:
   - https://api.slack.com/apps

2. Enable **Socket Mode** and create an App-Level Token:
   - scope: `connections:write`
   - token format: `xapp-...`

3. Add Bot Token Scopes under **OAuth & Permissions** (minimum recommended):
   - `chat:write`
   - `channels:history`
   - `channels:read`
   - `users:read`
   - `files:write`
   - optional for private channels: `groups:history`, `groups:read`
   - optional for username/icon customization: `chat:write.customize`

4. Under **Event Subscriptions**, enable events and subscribe bot events as needed:
   - `message.channels`
   - optional: `message.groups`, `user_typing`, `user_change`

5. Install/reinstall the app to your workspace and copy tokens:
   - Bot User OAuth Token -> `auth.bot_token`
   - App-Level Token -> `auth.app_token`

6. Fill auth values in `config.yaml`:

```yaml
auth:
  bot_token: "xoxb-..."
  app_token: "xapp-..."
  client_id: null
  client_secret: null
```

## Slack API/Spec References

This bridge implementation follows Slack official docs:

- Socket Mode overview: https://api.slack.com/apis/connections/socket
- `apps.connections.open`: https://docs.slack.dev/reference/methods/apps.connections.open/
- `chat.postMessage`: https://docs.slack.dev/reference/methods/chat.postMessage/
- `chat.update`: https://docs.slack.dev/reference/methods/chat.update/
- `users.info`: https://docs.slack.dev/reference/methods/users.info/
- `conversations.info`: https://docs.slack.dev/reference/methods/conversations.info/
- File upload flow:
  - https://docs.slack.dev/reference/methods/files.getUploadURLExternal/
  - https://docs.slack.dev/reference/methods/files.completeUploadExternal/
- Message event scopes/events:
  - https://docs.slack.dev/reference/events/message.channels/

## Configure Matrix / Palpo (Step by Step)

1. In Palpo config (`palpo.toml`), set server name and appservice registration directory:

```toml
server_name = "example.com"
appservice_registration_dir = "appservices"
```

2. Place your bridge registration file there, for example:
   - `appservices/slack-registration.yaml`

3. Ensure tokens are consistent between Palpo registration and bridge config:
   - `as_token` in registration == bridge appservice token
   - `hs_token` in registration == bridge homeserver token

4. Ensure bridge homeserver fields point to Palpo:

```yaml
bridge:
  domain: "example.com"
  homeserver_url: "http://127.0.0.1:6006"
```

## Configure Matrix / Synapse (Step by Step)

Create `slack-registration.yaml` (or set `REGISTRATION_PATH`) and load it from Synapse:

```yaml
id: "slack"
url: "http://127.0.0.1:9005"
as_token: "CHANGE_ME_AS_TOKEN"
hs_token: "CHANGE_ME_HS_TOKEN"
sender_localpart: "_slack_"
rate_limited: false
protocols: ["slack"]
namespaces:
  users:
    - exclusive: true
      regex: "@_slack_.*:example.com"
  aliases:
    - exclusive: true
      regex: "#_slack_.*:example.com"
  rooms: []
```

In `homeserver.yaml`:

```yaml
app_service_config_files:
  - /path/to/slack-registration.yaml
```

## Docker

Build:

```bash
docker build -t ghcr.io/palpo-im/matrix-bridge-slack:main -f Dockerfile .
```

Run:

```bash
docker run --rm \
  -p 9005:9005 \
  -v "$(pwd)/config:/data" \
  -e CONFIG_PATH=/data/config.yaml \
  ghcr.io/palpo-im/matrix-bridge-slack:main
```

## Environment Overrides

- `CONFIG_PATH`
- `REGISTRATION_PATH`
- `APPSERVICE_SLACK_AUTH_BOT_TOKEN`
- `APPSERVICE_SLACK_AUTH_APP_TOKEN`
- `APPSERVICE_SLACK_AUTH_CLIENT_ID`
- `APPSERVICE_SLACK_AUTH_CLIENT_SECRET`
- `APPSERVICE_SLACK_REGISTRATION_ID`
- `APPSERVICE_SLACK_REGISTRATION_AS_TOKEN`
- `APPSERVICE_SLACK_REGISTRATION_HS_TOKEN`
- `APPSERVICE_SLACK_REGISTRATION_SENDER_LOCALPART`
