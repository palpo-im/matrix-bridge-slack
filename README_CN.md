# matrix-bridge-slack

Rust 实现的 Matrix <-> Slack 桥接服务。

[English](README.md)

维护者：`Palpo Team`  
联系方式：`chris@acroidea.com`

## 概览

- 纯 Rust 实现
- Matrix appservice + Slack bot 桥接核心
- 使用 Slack Socket Mode 接收入站事件
- 使用 Slack Web API 发送消息、编辑消息、上传文件和查询信息
- 提供 health/status/metrics 以及 provisioning 的 HTTP 端点
- 数据库后端：PostgreSQL、SQLite 和 MySQL（通过 feature 控制）

## 仓库结构

- `src/`: 桥接实现
- `config/config.sample.yaml`: 示例配置
- `Dockerfile`: 多阶段容器构建

## 前置条件

- Rust 工具链（Docker 构建使用 Rust 1.93）
- 已配置 appservice 的 Matrix homeserver
- 一个 Slack app，包含：
  - bot token（`xoxb-...`）
  - app token（`xapp-...`），并带有 `connections:write`
- 数据库：PostgreSQL、SQLite 或 MySQL

## 快速开始（本地）

1. 创建配置文件：

```bash
cp config/config.sample.yaml config.yaml
```

2. 在 `config.yaml` 中设置必需项：
   - `bridge.domain`
   - `auth.bot_token`
   - `auth.app_token`
   - `database.url`（或 `database.conn_string` / `database.filename`）
   - 通过以下任一方式提供注册信息：
     - `registration.id`、`registration.as_token`、`registration.hs_token`
     - 放在配置文件同目录下的 `slack-registration.yaml`
     - 环境变量（见下文“环境变量覆盖”）

3. 运行：

```bash
cargo check -p matrix-bridge-slack
cargo test -p matrix-bridge-slack --no-run
cargo run -p matrix-bridge-slack
```

4. 验证：

```bash
curl http://127.0.0.1:9005/health
curl http://127.0.0.1:9005/status
```

## 配置 Slack（分步）

1. 在你的工作区中创建一个 Slack app：
   - https://api.slack.com/apps

2. 启用 **Socket Mode** 并创建 App-Level Token：
   - scope：`connections:write`
   - token 格式：`xapp-...`

3. 在 **OAuth & Permissions** 下添加 Bot Token Scopes（最小推荐）：
   - `chat:write`
   - `channels:history`
   - `channels:read`
   - `users:read`
   - `files:write`
   - 私有频道可选：`groups:history`、`groups:read`
   - 自定义用户名/头像可选：`chat:write.customize`

4. 在 **Event Subscriptions** 中启用事件，并按需订阅 bot events：
   - `message.channels`
   - 可选：`message.groups`、`user_typing`、`user_change`

5. 将应用安装或重新安装到工作区，并复制 token：
   - Bot User OAuth Token -> `auth.bot_token`
   - App-Level Token -> `auth.app_token`

6. 在 `config.yaml` 中填写认证配置：

```yaml
auth:
  bot_token: "xoxb-..."
  app_token: "xapp-..."
  client_id: null
  client_secret: null
```

## Slack API / 规范参考

该桥接实现遵循 Slack 官方文档：

- Socket Mode 概览：https://api.slack.com/apis/connections/socket
- `apps.connections.open`: https://docs.slack.dev/reference/methods/apps.connections.open/
- `chat.postMessage`: https://docs.slack.dev/reference/methods/chat.postMessage/
- `chat.update`: https://docs.slack.dev/reference/methods/chat.update/
- `users.info`: https://docs.slack.dev/reference/methods/users.info/
- `conversations.info`: https://docs.slack.dev/reference/methods/conversations.info/
- 文件上传流程：
  - https://docs.slack.dev/reference/methods/files.getUploadURLExternal/
  - https://docs.slack.dev/reference/methods/files.completeUploadExternal/
- 消息事件 scopes/events：
  - https://docs.slack.dev/reference/events/message.channels/

## 配置 Matrix / Palpo（分步）

1. 在 Palpo 配置（`palpo.toml`）中设置服务器名和 appservice 注册目录：

```toml
server_name = "example.com"
appservice_registration_dir = "appservices"
```

2. 将桥接注册文件放到该目录下，例如：
   - `appservices/slack-registration.yaml`

3. 确保 Palpo 注册文件与桥接配置中的 token 一致：
   - 注册文件中的 `as_token` == 桥接 appservice token
   - 注册文件中的 `hs_token` == 桥接 homeserver token

4. 确保桥接 homeserver 相关字段指向 Palpo：

```yaml
bridge:
  domain: "example.com"
  homeserver_url: "http://127.0.0.1:6006"
```

## 配置 Matrix / Synapse（分步）

创建 `slack-registration.yaml`（或设置 `REGISTRATION_PATH`），并在 Synapse 中加载：

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

在 `homeserver.yaml` 中：

```yaml
app_service_config_files:
  - /path/to/slack-registration.yaml
```

## Docker

构建：

```bash
docker build -t ghcr.io/palpo-im/matrix-bridge-slack:main -f Dockerfile .
```

运行：

```bash
docker run --rm \
  -p 9005:9005 \
  -v "$(pwd)/config:/data" \
  -e CONFIG_PATH=/data/config.yaml \
  ghcr.io/palpo-im/matrix-bridge-slack:main
```

## 环境变量覆盖

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
