use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow};
use futures::{SinkExt, StreamExt};
use once_cell::sync::Lazy;
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::sync::RwLock;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::protocol::Message as WsMessage;
use tracing::{debug, error, info, warn};

use crate::bridge::{BridgeCore, SlackMessageContext};
use crate::config::Config;

const INITIAL_LOGIN_RETRY_SECONDS: u64 = 2;
const MAX_LOGIN_RETRY_SECONDS: u64 = 300;
const PERMISSION_CACHE_TTL_SECONDS: u64 = 300;

static USER_MENTION_REGEX: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"<@([A-Z0-9]+)(?:\|[^>]+)?>").expect("valid user mention regex"));
static CHANNEL_MENTION_REGEX: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"<#([A-Z0-9]+)\|([^>]+)>").expect("valid channel mention regex")
});
static LINK_WITH_LABEL_REGEX: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"<((?:https?|mailto):[^>|]+)\|([^>]+)>").expect("valid labeled link regex")
});
static RAW_LINK_REGEX: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"<((?:https?|mailto):[^>]+)>").expect("valid raw link regex"));

pub mod command_handler;
pub mod embed;

pub use self::command_handler::{SlackCommandHandler, SlackCommandOutcome, ModerationAction};
pub use self::embed::{
    SlackEmbed, EmbedAuthor, EmbedFooter, build_matrix_message_embed, build_reply_embed,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlackUser {
    pub id: String,
    pub username: String,
    pub discriminator: String,
    pub avatar: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlackChannel {
    pub id: String,
    pub name: String,
    pub guild_id: String,
    pub topic: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlackMessage {
    pub id: String,
    pub channel_id: String,
    pub author_id: String,
    pub content: String,
    pub attachments: Vec<String>,
    pub reply_to: Option<String>,
    pub edit_of: Option<String>,
    pub timestamp: String,
}

#[derive(Clone)]
pub struct SlackClient {
    _config: Arc<Config>,
    send_lock: Arc<tokio::sync::Mutex<()>>,
    login_state: Arc<tokio::sync::Mutex<SlackLoginState>>,
    bridge: Arc<RwLock<Option<Arc<BridgeCore>>>>,
    http: reqwest::Client,
    bot_user_id: Arc<RwLock<Option<String>>>,
    bot_id: Arc<RwLock<Option<String>>>,
    team_id: Arc<RwLock<Option<String>>>,
    permission_cache: Arc<tokio::sync::Mutex<HashMap<String, CachedPermission>>>,
}

#[derive(Default)]
struct SlackLoginState {
    is_logged_in: bool,
    gateway_task: Option<tokio::task::JoinHandle<()>>,
}

#[derive(Clone)]
struct CachedPermission {
    permissions: HashSet<String>,
    expires_at: Instant,
}

struct AuthInfo {
    user_id: String,
    bot_id: Option<String>,
    team_id: Option<String>,
}

impl SlackClient {
    pub async fn new(config: Arc<Config>) -> Result<Self> {
        let http = reqwest::Client::builder()
            .user_agent("matrix-bridge-slack")
            .build()
            .context("failed to construct HTTP client")?;

        Ok(Self {
            _config: config,
            send_lock: Arc::new(tokio::sync::Mutex::new(())),
            login_state: Arc::new(tokio::sync::Mutex::new(SlackLoginState::default())),
            bridge: Arc::new(RwLock::new(None)),
            http,
            bot_user_id: Arc::new(RwLock::new(None)),
            bot_id: Arc::new(RwLock::new(None)),
            team_id: Arc::new(RwLock::new(None)),
            permission_cache: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
        })
    }

    pub async fn set_bridge(&self, bridge: Arc<BridgeCore>) {
        *self.bridge.write().await = Some(bridge);
    }

    pub async fn login(&self) -> Result<()> {
        let mut state = self.login_state.lock().await;
        if state.is_logged_in {
            return Ok(());
        }

        let auth = self.auth_test().await?;
        *self.bot_user_id.write().await = Some(auth.user_id.clone());
        *self.bot_id.write().await = auth.bot_id;
        *self.team_id.write().await = auth.team_id;

        let client = self.clone();
        let gateway_task = tokio::spawn(async move {
            client.socket_mode_loop().await;
        });

        state.gateway_task = Some(gateway_task);
        state.is_logged_in = true;
        info!("slack socket mode started user={}", auth.user_id);

        Ok(())
    }

    pub async fn start(&self) -> Result<()> {
        let mut retry_seconds = INITIAL_LOGIN_RETRY_SECONDS;
        loop {
            match self.login().await {
                Ok(()) => {
                    info!("slack client is ready");
                    return Ok(());
                }
                Err(err) => {
                    error!(
                        "failed to start slack client: {err}. retrying in {} seconds",
                        retry_seconds
                    );
                    tokio::time::sleep(Duration::from_secs(retry_seconds)).await;
                    retry_seconds = (retry_seconds * 2).min(MAX_LOGIN_RETRY_SECONDS);
                }
            }
        }
    }

    pub async fn stop(&self) -> Result<()> {
        let mut state = self.login_state.lock().await;
        if !state.is_logged_in {
            return Ok(());
        }

        if let Some(gateway_task) = state.gateway_task.take() {
            gateway_task.abort();
            let _ = gateway_task.await;
        }

        state.is_logged_in = false;
        info!("slack client stopped");
        Ok(())
    }

    pub async fn send_message(&self, channel_id: &str, content: &str) -> Result<String> {
        self.send_message_with_metadata(channel_id, content, &[], None, None)
            .await
    }

    pub async fn send_message_with_metadata(
        &self,
        channel_id: &str,
        content: &str,
        attachments: &[String],
        reply_to: Option<&str>,
        edit_of: Option<&str>,
    ) -> Result<String> {
        self.send_message_with_metadata_as_user(
            channel_id,
            content,
            attachments,
            reply_to,
            edit_of,
            None,
            None,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn send_message_with_metadata_as_user(
        &self,
        channel_id: &str,
        content: &str,
        attachments: &[String],
        reply_to: Option<&str>,
        edit_of: Option<&str>,
        username: Option<&str>,
        avatar_url: Option<&str>,
    ) -> Result<String> {
        let _guard = self.send_lock.lock().await;
        let delay = self._config.limits.slack_send_delay;
        if delay > 0 {
            tokio::time::sleep(Duration::from_millis(delay)).await;
        }

        let mut text = content.trim().to_string();
        for attachment in attachments {
            if !text.is_empty() {
                text.push('\n');
            }
            text.push_str(attachment);
        }
        if text.is_empty() {
            text = "(empty message)".to_string();
        }
        if let Some(name) = username {
            text = format!("*{}*: {}", name, text);
        }

        if let Some(ts) = edit_of {
            return self
                .chat_update(channel_id, ts, &text, username, avatar_url)
                .await;
        }

        self.chat_post_message(channel_id, &text, reply_to, username, avatar_url)
            .await
    }

    pub async fn send_embed_as_user(
        &self,
        channel_id: &str,
        embed: &SlackEmbed,
        username: Option<&str>,
        avatar_url: Option<&str>,
    ) -> Result<String> {
        let mut lines = Vec::new();
        if let Some(title) = &embed.title
            && !title.trim().is_empty()
        {
            lines.push(format!("*{}*", title.trim()));
        }
        if let Some(desc) = &embed.description
            && !desc.trim().is_empty()
        {
            lines.push(desc.trim().to_string());
        }
        for field in &embed.fields {
            if !field.value.trim().is_empty() {
                lines.push(format!("*{}*: {}", field.name.trim(), field.value.trim()));
            }
        }
        if let Some(footer) = &embed.footer
            && !footer.text.trim().is_empty()
        {
            lines.push(format!("_{}_", footer.text.trim()));
        }
        self.send_message_with_metadata_as_user(
            channel_id,
            &lines.join("\n"),
            &[],
            None,
            None,
            username,
            avatar_url,
        )
        .await
    }

    pub async fn send_file_as_user(
        &self,
        channel_id: &str,
        data: &[u8],
        _content_type: &str,
        filename: &str,
        username: Option<&str>,
        avatar_url: Option<&str>,
    ) -> Result<String> {
        let _guard = self.send_lock.lock().await;

        let bot_token = self.bot_token()?;
        let reserve = self
            .slack_api_post(
                "files.getUploadURLExternal",
                &bot_token,
                json!({
                    "filename": filename,
                    "length": data.len()
                }),
            )
            .await?;

        let upload_url = reserve
            .get("upload_url")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("files.getUploadURLExternal missing upload_url"))?;
        let file_id = reserve
            .get("file_id")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("files.getUploadURLExternal missing file_id"))?
            .to_string();

        let upload_response = self
            .http
            .post(upload_url)
            .header("Content-Type", "application/octet-stream")
            .body(data.to_vec())
            .send()
            .await
            .context("failed to upload file payload to Slack upload URL")?;
        if !upload_response.status().is_success() {
            return Err(anyhow!("slack file upload failed: {}", upload_response.status()));
        }

        let mut payload = json!({
            "files": [{ "id": file_id, "title": filename }],
            "channel_id": channel_id
        });
        if let Some(name) = username {
            payload["initial_comment"] = json!(format!("Uploaded by {name}"));
        }

        let _ = self
            .slack_api_post("files.completeUploadExternal", &bot_token, payload)
            .await?;

        let _ = avatar_url;
        Ok(file_id)
    }

    pub async fn add_reaction(&self, channel_id: &str, message_ts: &str, emoji: &str) -> Result<()> {
        let bot_token = self.bot_token()?;
        let payload = json!({
            "channel": channel_id,
            "timestamp": message_ts,
            "name": emoji
        });
        self.slack_api_post("reactions.add", &bot_token, payload).await?;
        Ok(())
    }

    pub async fn remove_reaction(&self, channel_id: &str, message_ts: &str, emoji: &str) -> Result<()> {
        let bot_token = self.bot_token()?;
        let payload = json!({
            "channel": channel_id,
            "timestamp": message_ts,
            "name": emoji
        });
        self.slack_api_post("reactions.remove", &bot_token, payload).await?;
        Ok(())
    }

    pub async fn get_conversation_history(&self, channel_id: &str, limit: Option<u32>, cursor: Option<&str>) -> Result<Value> {
        let bot_token = self.bot_token()?;
        let mut payload = json!({
            "channel": channel_id,
        });
        if let Some(l) = limit {
            payload["limit"] = json!(l);
        }
        if let Some(c) = cursor {
            payload["cursor"] = json!(c);
        }
        let result = self.slack_api_post("conversations.history", &bot_token, payload).await?;
        Ok(result)
    }

    pub async fn get_user(&self, user_id: &str) -> Result<Option<SlackUser>> {
        let bot_token = self.bot_token()?;
        let value = self
            .slack_api_post("users.info", &bot_token, json!({ "user": user_id }))
            .await;
        let value = match value {
            Ok(v) => v,
            Err(err) => {
                warn!("failed to fetch slack user {}: {}", user_id, err);
                return Ok(None);
            }
        };

        let user = match value.get("user") {
            Some(user) => user,
            None => return Ok(None),
        };
        let username = extract_display_name(user).unwrap_or_else(|| user_id.to_string());
        let avatar = user
            .pointer("/profile/image_512")
            .and_then(Value::as_str)
            .or_else(|| user.pointer("/profile/image_192").and_then(Value::as_str))
            .map(ToOwned::to_owned);

        Ok(Some(SlackUser {
            id: user_id.to_string(),
            username,
            discriminator: "0000".to_string(),
            avatar,
        }))
    }

    pub async fn clear_channel_member_overwrite(
        &self,
        channel_id: &str,
        user_id: &str,
    ) -> Result<()> {
        warn!(
            "slack does not support slack-style permission overwrite clear, ignoring channel={} user={}",
            channel_id, user_id
        );
        Ok(())
    }

    pub async fn deny_channel_member_permissions(
        &self,
        channel_id: &str,
        user_id: &str,
    ) -> Result<()> {
        warn!(
            "slack does not support slack-style permission deny, ignoring channel={} user={}",
            channel_id, user_id
        );
        Ok(())
    }

    pub async fn get_channel(&self, channel_id: &str) -> Result<Option<SlackChannel>> {
        let bot_token = self.bot_token()?;
        let value = self
            .slack_api_post(
                "conversations.info",
                &bot_token,
                json!({ "channel": channel_id }),
            )
            .await;
        let value = match value {
            Ok(v) => v,
            Err(err) => {
                warn!("failed to fetch slack channel {}: {}", channel_id, err);
                return Ok(None);
            }
        };

        let channel = match value.get("channel") {
            Some(channel) => channel,
            None => return Ok(None),
        };

        let fallback_team = self
            .team_id
            .read()
            .await
            .clone()
            .unwrap_or_else(|| "slack".to_string());

        Ok(Some(SlackChannel {
            id: channel_id.to_string(),
            name: channel
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or(channel_id)
                .to_string(),
            guild_id: channel
                .get("context_team_id")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned)
                .unwrap_or(fallback_team),
            topic: channel
                .pointer("/topic/value")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned),
        }))
    }

    async fn socket_mode_loop(self) {
        let app_token = match self.app_token() {
            Ok(token) => token,
            Err(err) => {
                error!("{err}");
                return;
            }
        };

        let mut retry_seconds = INITIAL_LOGIN_RETRY_SECONDS;
        loop {
            match self.open_socket_mode_url(&app_token).await {
                Ok(url) => match connect_async(url).await {
                    Ok((mut stream, _)) => {
                        retry_seconds = INITIAL_LOGIN_RETRY_SECONDS;
                        info!("slack socket mode connected");
                        while let Some(frame) = stream.next().await {
                            match frame {
                                Ok(WsMessage::Text(text)) => {
                                    if let Err(err) =
                                        self.handle_socket_text(&mut stream, &text).await
                                    {
                                        warn!("socket payload handling failed: {}", err);
                                    }
                                }
                                Ok(WsMessage::Ping(payload)) => {
                                    let _ = stream.send(WsMessage::Pong(payload)).await;
                                }
                                Ok(WsMessage::Close(_)) => break,
                                Ok(_) => {}
                                Err(err) => {
                                    warn!("socket frame error: {}", err);
                                    break;
                                }
                            }
                        }
                    }
                    Err(err) => warn!("failed to connect Slack websocket: {}", err),
                },
                Err(err) => warn!("failed to open Slack socket mode URL: {}", err),
            }

            tokio::time::sleep(Duration::from_secs(retry_seconds)).await;
            retry_seconds = (retry_seconds * 2).min(MAX_LOGIN_RETRY_SECONDS);
        }
    }

    async fn handle_socket_text(
        &self,
        stream: &mut tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
        text: &str,
    ) -> Result<()> {
        let payload: Value = serde_json::from_str(text).context("invalid socket payload JSON")?;

        if let Some(envelope_id) = payload.get("envelope_id").and_then(Value::as_str) {
            let ack = json!({ "envelope_id": envelope_id });
            stream
                .send(WsMessage::Text(ack.to_string().into()))
                .await
                .context("failed to ack slack envelope")?;
        }

        if payload.get("type").and_then(Value::as_str) == Some("disconnect") {
            return Err(anyhow!("received disconnect from Slack"));
        }

        let Some(events_api) = payload.get("payload") else {
            return Ok(());
        };
        if events_api.get("type").and_then(Value::as_str) != Some("events_api") {
            return Ok(());
        }
        let Some(event) = events_api.get("event") else {
            return Ok(());
        };
        self.handle_event(event).await
    }

    async fn handle_event(&self, event: &Value) -> Result<()> {
        match event.get("type").and_then(Value::as_str).unwrap_or("") {
            "message" => self.handle_message_event(event).await?,
            "user_typing" => self.handle_typing_event(event).await?,
            "user_change" => self.handle_user_change_event(event).await?,
            "reaction_added" => self.handle_reaction_added_event(event).await?,
            "reaction_removed" => self.handle_reaction_removed_event(event).await?,
            "member_joined_channel" => self.handle_member_joined_channel_event(event).await?,
            "member_left_channel" => self.handle_member_left_channel_event(event).await?,
            "channel_marked" => self.handle_channel_marked_event(event).await?,
            _ => {}
        }
        Ok(())
    }

    async fn handle_typing_event(&self, event: &Value) -> Result<()> {
        let Some(channel_id) = event.get("channel").and_then(Value::as_str) else {
            return Ok(());
        };
        let Some(user_id) = event.get("user").and_then(Value::as_str) else {
            return Ok(());
        };

        if self.is_own_message(Some(user_id), None).await {
            return Ok(());
        }

        if let Some(bridge) = self.bridge.read().await.clone()
            && let Err(err) = bridge.handle_slack_typing(channel_id, user_id).await
        {
            error!("failed to forward slack typing event: {}", err);
        }
        Ok(())
    }

    async fn handle_user_change_event(&self, event: &Value) -> Result<()> {
        let Some(user) = event.get("user") else {
            return Ok(());
        };
        let Some(user_id) = user.get("id").and_then(Value::as_str) else {
            return Ok(());
        };
        if self.is_own_message(Some(user_id), None).await {
            return Ok(());
        }

        let display_name = extract_display_name(user).unwrap_or_else(|| user_id.to_string());
        let avatar_url = user
            .pointer("/profile/image_512")
            .and_then(Value::as_str)
            .or_else(|| user.pointer("/profile/image_192").and_then(Value::as_str));

        if let Some(bridge) = self.bridge.read().await.clone()
            && let Err(err) = bridge
                .handle_slack_user_update(user_id, &display_name, avatar_url)
                .await
        {
            error!("failed to forward slack user_change event: {}", err);
        }

        Ok(())
    }

    async fn handle_reaction_added_event(&self, event: &Value) -> Result<()> {
        let Some(channel_id) = event.get("item",).and_then(|i| i.get("channel")).and_then(Value::as_str) else {
            return Ok(());
        };
        let Some(message_ts) = event.get("item",).and_then(|i| i.get("ts")).and_then(Value::as_str) else {
            return Ok(());
        };
        let Some(user_id) = event.get("user").and_then(Value::as_str) else {
            return Ok(());
        };
        let Some(reaction) = event.get("reaction").and_then(Value::as_str) else {
            return Ok(());
        };

        if self.is_own_message(Some(user_id), None).await {
            return Ok(());
        }

        if let Some(bridge) = self.bridge.read().await.clone()
            && let Err(err) = bridge
                .handle_slack_reaction_added(channel_id, message_ts, user_id, reaction)
                .await
        {
            error!("failed to forward slack reaction_added event: {}", err);
        }
        Ok(())
    }

    async fn handle_reaction_removed_event(&self, event: &Value) -> Result<()> {
        let Some(channel_id) = event.get("item",).and_then(|i| i.get("channel")).and_then(Value::as_str) else {
            return Ok(());
        };
        let Some(message_ts) = event.get("item",).and_then(|i| i.get("ts")).and_then(Value::as_str) else {
            return Ok(());
        };
        let Some(user_id) = event.get("user").and_then(Value::as_str) else {
            return Ok(());
        };
        let Some(reaction) = event.get("reaction").and_then(Value::as_str) else {
            return Ok(());
        };

        if self.is_own_message(Some(user_id), None).await {
            return Ok(());
        }

        if let Some(bridge) = self.bridge.read().await.clone()
            && let Err(err) = bridge
                .handle_slack_reaction_removed(channel_id, message_ts, user_id, reaction)
                .await
        {
            error!("failed to forward slack reaction_removed event: {}", err);
        }
        Ok(())
    }

    async fn handle_member_joined_channel_event(&self, event: &Value) -> Result<()> {
        let Some(channel_id) = event.get("channel").and_then(Value::as_str) else {
            return Ok(());
        };
        let Some(user_id) = event.get("user").and_then(Value::as_str) else {
            return Ok(());
        };

        if self.is_own_message(Some(user_id), None).await {
            return Ok(());
        }

        if let Some(bridge) = self.bridge.read().await.clone()
            && let Err(err) = bridge
                .handle_slack_member_joined_channel(channel_id, user_id)
                .await
        {
            error!("failed to forward slack member_joined_channel event: {}", err);
        }
        Ok(())
    }

    async fn handle_member_left_channel_event(&self, event: &Value) -> Result<()> {
        let Some(channel_id) = event.get("channel").and_then(Value::as_str) else {
            return Ok(());
        };
        let Some(user_id) = event.get("user").and_then(Value::as_str) else {
            return Ok(());
        };

        if self.is_own_message(Some(user_id), None).await {
            return Ok(());
        }

        if let Some(bridge) = self.bridge.read().await.clone()
            && let Err(err) = bridge
                .handle_slack_member_left_channel(channel_id, user_id)
                .await
        {
            error!("failed to forward slack member_left_channel event: {}", err);
        }
        Ok(())
    }

    async fn handle_channel_marked_event(&self, event: &Value) -> Result<()> {
        let Some(channel_id) = event.get("channel").and_then(Value::as_str) else {
            return Ok(());
        };
        let Some(user_id) = event.get("user").and_then(Value::as_str) else {
            return Ok(());
        };
        let Some(ts) = event.get("ts").and_then(Value::as_str) else {
            return Ok(());
        };

        if self.is_own_message(Some(user_id), None).await {
            return Ok(());
        }

        if let Some(bridge) = self.bridge.read().await.clone()
            && let Err(err) = bridge
                .handle_slack_channel_marked(channel_id, user_id, ts)
                .await
        {
            error!("failed to forward slack channel_marked event: {}", err);
        }
        Ok(())
    }

    async fn handle_message_event(&self, event: &Value) -> Result<()> {
        match event.get("subtype").and_then(Value::as_str) {
            Some("message_deleted") => {
                let Some(channel_id) = event.get("channel").and_then(Value::as_str) else {
                    return Ok(());
                };
                let Some(deleted_ts) = event
                    .get("deleted_ts")
                    .and_then(Value::as_str)
                    .or_else(|| event.pointer("/previous_message/ts").and_then(Value::as_str))
                else {
                    return Ok(());
                };

                if let Some(bridge) = self.bridge.read().await.clone()
                    && let Err(err) = bridge.handle_slack_message_delete(channel_id, deleted_ts).await
                {
                    error!("failed to forward slack delete event: {}", err);
                }
                return Ok(());
            }
            Some("message_changed") => {
                let Some(channel_id) = event.get("channel").and_then(Value::as_str) else {
                    return Ok(());
                };
                if let Some(message) = event.get("message") {
                    self.forward_message(channel_id, message, true).await?;
                }
                return Ok(());
            }
            Some("bot_message") => return Ok(()),
            Some(_) => return Ok(()),
            None => {}
        }

        let Some(channel_id) = event.get("channel").and_then(Value::as_str) else {
            return Ok(());
        };
        self.forward_message(channel_id, event, false).await
    }

    async fn forward_message(&self, channel_id: &str, message: &Value, is_edit: bool) -> Result<()> {
        let sender_id = message.get("user").and_then(Value::as_str);
        let bot_id = message.get("bot_id").and_then(Value::as_str);
        if self.is_own_message(sender_id, bot_id).await {
            return Ok(());
        }

        let Some(sender_id) = sender_id else {
            return Ok(());
        };
        let Some(message_ts) = message.get("ts").and_then(Value::as_str) else {
            return Ok(());
        };

        let thread_ts = message.get("thread_ts").and_then(Value::as_str);
        let reply_to = thread_ts
            .filter(|thread| *thread != message_ts)
            .map(ToOwned::to_owned);
        let text = normalize_slack_text(
            message
                .get("text")
                .and_then(Value::as_str)
                .unwrap_or_default(),
        );
        let attachments = extract_slack_attachments(message);
        let permissions = self.resolve_permissions(sender_id).await;

        let Some(bridge) = self.bridge.read().await.clone() else {
            debug!("slack message received before bridge binding");
            return Ok(());
        };

        if let Err(err) = bridge
            .handle_slack_message_with_context(SlackMessageContext {
                channel_id: channel_id.to_string(),
                source_message_id: Some(message_ts.to_string()),
                sender_id: sender_id.to_string(),
                content: text,
                attachments,
                reply_to,
                edit_of: if is_edit {
                    Some(message_ts.to_string())
                } else {
                    None
                },
                permissions,
            })
            .await
        {
            error!("failed to forward slack message to bridge: {}", err);
        }

        Ok(())
    }

    async fn resolve_permissions(&self, user_id: &str) -> HashSet<String> {
        let now = Instant::now();
        {
            let cache = self.permission_cache.lock().await;
            if let Some(cached) = cache.get(user_id)
                && cached.expires_at > now
            {
                return cached.permissions.clone();
            }
        }

        let permissions = match self.fetch_permissions(user_id).await {
            Ok(permissions) => permissions,
            Err(err) => {
                warn!("failed to resolve slack permissions for {}: {}", user_id, err);
                HashSet::new()
            }
        };

        self.permission_cache.lock().await.insert(
            user_id.to_string(),
            CachedPermission {
                permissions: permissions.clone(),
                expires_at: Instant::now() + Duration::from_secs(PERMISSION_CACHE_TTL_SECONDS),
            },
        );

        permissions
    }

    async fn fetch_permissions(&self, user_id: &str) -> Result<HashSet<String>> {
        let bot_token = self.bot_token()?;
        let value = self
            .slack_api_post("users.info", &bot_token, json!({ "user": user_id }))
            .await?;
        let user = value
            .get("user")
            .ok_or_else(|| anyhow!("users.info missing user object"))?;

        let is_admin = user.get("is_admin").and_then(Value::as_bool).unwrap_or(false);
        let is_owner = user.get("is_owner").and_then(Value::as_bool).unwrap_or(false);
        let is_primary_owner = user
            .get("is_primary_owner")
            .and_then(Value::as_bool)
            .unwrap_or(false);

        let mut permissions = HashSet::new();
        if is_admin || is_owner || is_primary_owner {
            permissions.insert("MANAGE_WEBHOOKS".to_string());
            permissions.insert("MANAGE_CHANNELS".to_string());
            permissions.insert("BAN_MEMBERS".to_string());
            permissions.insert("KICK_MEMBERS".to_string());
        }
        Ok(permissions)
    }

    async fn is_own_message(&self, sender_user_id: Option<&str>, sender_bot_id: Option<&str>) -> bool {
        let bot_user_id = self.bot_user_id.read().await.clone();
        let bot_id = self.bot_id.read().await.clone();
        sender_user_id.is_some_and(|id| bot_user_id.as_deref() == Some(id))
            || sender_bot_id.is_some_and(|id| bot_id.as_deref() == Some(id))
    }

    async fn chat_post_message(
        &self,
        channel_id: &str,
        text: &str,
        thread_ts: Option<&str>,
        username: Option<&str>,
        avatar_url: Option<&str>,
    ) -> Result<String> {
        let bot_token = self.bot_token()?;
        let mut payload = json!({
            "channel": channel_id,
            "text": text,
            "unfurl_links": false,
            "unfurl_media": false
        });
        if let Some(thread_ts) = thread_ts {
            payload["thread_ts"] = json!(thread_ts);
        }
        let response = self
            .post_chat_payload_with_customize_fallback(
                "chat.postMessage",
                &bot_token,
                payload,
                username,
                avatar_url,
            )
            .await?;
        response
            .get("ts")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned)
            .ok_or_else(|| anyhow!("chat.postMessage missing ts"))
    }

    async fn chat_update(
        &self,
        channel_id: &str,
        message_ts: &str,
        text: &str,
        username: Option<&str>,
        avatar_url: Option<&str>,
    ) -> Result<String> {
        let bot_token = self.bot_token()?;
        let payload = json!({
            "channel": channel_id,
            "ts": message_ts,
            "text": text
        });
        let response = self
            .post_chat_payload_with_customize_fallback(
                "chat.update",
                &bot_token,
                payload,
                username,
                avatar_url,
            )
            .await?;
        response
            .get("ts")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned)
            .ok_or_else(|| anyhow!("chat.update missing ts"))
    }

    async fn post_chat_payload_with_customize_fallback(
        &self,
        method: &str,
        bot_token: &str,
        base_payload: Value,
        username: Option<&str>,
        avatar_url: Option<&str>,
    ) -> Result<Value> {
        if username.is_none() && avatar_url.is_none() {
            return self.slack_api_post(method, bot_token, base_payload).await;
        }

        let mut customized = base_payload.clone();
        if let Some(name) = username {
            customized["username"] = json!(name);
        }
        if let Some(url) = avatar_url {
            customized["icon_url"] = json!(url);
        }

        match self.slack_api_post(method, bot_token, customized).await {
            Ok(value) => Ok(value),
            Err(err) => {
                warn!(
                    "slack {} with custom username/icon failed, retrying without customization: {}",
                    method, err
                );
                self.slack_api_post(method, bot_token, base_payload).await
            }
        }
    }

    async fn auth_test(&self) -> Result<AuthInfo> {
        let bot_token = self.bot_token()?;
        let value = self.slack_api_post("auth.test", &bot_token, json!({})).await?;
        Ok(AuthInfo {
            user_id: value
                .get("user_id")
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow!("auth.test missing user_id"))?
                .to_string(),
            bot_id: value
                .get("bot_id")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned),
            team_id: value
                .get("team_id")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned),
        })
    }

    async fn open_socket_mode_url(&self, app_token: &str) -> Result<String> {
        let value = self
            .slack_api_post("apps.connections.open", app_token, json!({}))
            .await?;
        value
            .get("url")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned)
            .ok_or_else(|| anyhow!("apps.connections.open missing websocket URL"))
    }

    async fn slack_api_post(&self, method: &str, token: &str, payload: Value) -> Result<Value> {
        let response = self
            .http
            .post(format!("https://slack.com/api/{method}"))
            .bearer_auth(token)
            .json(&payload)
            .send()
            .await
            .with_context(|| format!("request to Slack API method {method} failed"))?;
        let status = response.status();
        let value: Value = response
            .json()
            .await
            .with_context(|| format!("Slack API method {method} returned non-JSON body"))?;

        if !status.is_success() {
            return Err(anyhow!(
                "Slack API {} failed status={} body={}",
                method,
                status,
                value
            ));
        }
        if !value.get("ok").and_then(Value::as_bool).unwrap_or(false) {
            let code = value
                .get("error")
                .and_then(Value::as_str)
                .unwrap_or("unknown_error");
            return Err(anyhow!("Slack API {} returned ok=false: {}", method, code));
        }

        Ok(value)
    }

    fn bot_token(&self) -> Result<String> {
        let token = self._config.auth.bot_token.trim();
        if token.is_empty() {
            return Err(anyhow!("auth.bot_token is empty"));
        }
        Ok(token.to_string())
    }

    fn app_token(&self) -> Result<String> {
        if let Some(token) = self
            ._config
            .auth
            .app_token
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            return Ok(token.to_string());
        }

        if let Some(token) = self
            ._config
            .auth
            .client_secret
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            warn!("using auth.client_secret as fallback for Slack app token");
            return Ok(token.to_string());
        }

        Err(anyhow!(
            "auth.app_token is required for Slack Socket Mode (xapp- token)"
        ))
    }
}

fn extract_display_name(user: &Value) -> Option<String> {
    user.pointer("/profile/display_name")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(|value| value.trim().to_string())
        .or_else(|| {
            user.pointer("/profile/real_name")
                .and_then(Value::as_str)
                .filter(|value| !value.trim().is_empty())
                .map(|value| value.trim().to_string())
        })
        .or_else(|| {
            user.get("name")
                .and_then(Value::as_str)
                .filter(|value| !value.trim().is_empty())
                .map(|value| value.trim().to_string())
        })
}

fn extract_slack_attachments(message: &Value) -> Vec<String> {
    let mut output = Vec::new();
    let mut seen = HashSet::new();

    if let Some(files) = message.get("files").and_then(Value::as_array) {
        for file in files {
            let link = file
                .get("permalink_public")
                .and_then(Value::as_str)
                .or_else(|| file.get("permalink").and_then(Value::as_str))
                .or_else(|| file.get("url_private_download").and_then(Value::as_str))
                .or_else(|| file.get("url_private").and_then(Value::as_str));
            if let Some(link) = link
                && seen.insert(link.to_string())
            {
                output.push(link.to_string());
            }
        }
    }

    output
}

fn normalize_slack_text(input: &str) -> String {
    let mut text = input
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">");

    text = USER_MENTION_REGEX
        .replace_all(&text, |caps: &regex::Captures| format!("@{}", &caps[1]))
        .to_string();
    text = CHANNEL_MENTION_REGEX
        .replace_all(&text, |caps: &regex::Captures| format!("#{}", &caps[2]))
        .to_string();
    text = text
        .replace("<!channel>", "@channel")
        .replace("<!here>", "@here")
        .replace("<!everyone>", "@everyone");
    text = LINK_WITH_LABEL_REGEX
        .replace_all(&text, |caps: &regex::Captures| {
            format!("{} ({})", &caps[2], &caps[1])
        })
        .to_string();
    text = RAW_LINK_REGEX
        .replace_all(&text, |caps: &regex::Captures| caps[1].to_string())
        .to_string();

    text
}

