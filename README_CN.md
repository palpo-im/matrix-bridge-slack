# matrix-bridge-slack

Rust 实现的 Matrix <-> Slack 桥接服务。

## 快速说明

- 本仓库已迁移为 Slack 版本。
- 使用 Slack **Socket Mode** 接收事件。
- 使用 Slack Web API 发送消息、编辑消息、上传文件、查询用户/频道。

## 必需配置

在 `config.yaml` 中至少设置：

```yaml
auth:
  bot_token: "xoxb-..."
  app_token: "xapp-..."
```

并确保 Matrix appservice 的注册信息完整（可用 `slack-registration.yaml`）。

## 运行

```bash
cargo check -p matrix-bridge-slack
cargo run -p matrix-bridge-slack
```

## Slack 官方文档

- Socket Mode: https://api.slack.com/apis/connections/socket
- apps.connections.open: https://docs.slack.dev/reference/methods/apps.connections.open/
- chat.postMessage: https://docs.slack.dev/reference/methods/chat.postMessage/
- chat.update: https://docs.slack.dev/reference/methods/chat.update/
- users.info: https://docs.slack.dev/reference/methods/users.info/
- conversations.info: https://docs.slack.dev/reference/methods/conversations.info/
- files.getUploadURLExternal: https://docs.slack.dev/reference/methods/files.getUploadURLExternal/
- files.completeUploadExternal: https://docs.slack.dev/reference/methods/files.completeUploadExternal/
- message.channels 事件: https://docs.slack.dev/reference/events/message.channels/

更完整的英文说明请查看 [README.md](README.md)。
