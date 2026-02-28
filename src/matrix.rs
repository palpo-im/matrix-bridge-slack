use std::sync::Arc;

use anyhow::{Context, Result};
use matrix_bot_sdk::appservice::{Appservice, AppserviceHandler};
use matrix_bot_sdk::client::{MatrixAuth, MatrixClient};
use matrix_bot_sdk::models::CreateRoom;
use serde_json::{Value, json};
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};
use url::Url;

use crate::config::Config;

pub mod command_handler;
pub mod event_handler;

pub use self::command_handler::{
    MatrixCommandHandler, MatrixCommandOutcome, MatrixCommandPermission,
};
pub use self::event_handler::{MatrixEventHandler, MatrixEventHandlerImpl, MatrixEventProcessor};

mod urlencoding {
    pub fn encode(s: &str) -> String {
        url::form_urlencoded::byte_serialize(s.as_bytes()).collect()
    }
}

pub struct BridgeAppserviceHandler {
    processor: Option<Arc<MatrixEventProcessor>>,
}

#[async_trait::async_trait]
impl AppserviceHandler for BridgeAppserviceHandler {
    async fn on_transaction(&self, _txn_id: &str, body: &Value) -> Result<()> {
        let Some(processor) = &self.processor else {
            return Ok(());
        };

        if let Some(events) = body.get("events").and_then(|v| v.as_array()) {
            for event in events {
                let Some(room_id) = event.get("room_id").and_then(|v| v.as_str()) else {
                    continue;
                };
                let Some(sender) = event.get("sender").and_then(|v| v.as_str()) else {
                    continue;
                };
                let Some(event_type) = event.get("type").and_then(|v| v.as_str()) else {
                    continue;
                };

                let matrix_event = MatrixEvent {
                    event_id: event
                        .get("event_id")
                        .and_then(|v| v.as_str())
                        .map(ToOwned::to_owned),
                    event_type: event_type.to_owned(),
                    room_id: room_id.to_owned(),
                    sender: sender.to_owned(),
                    state_key: event
                        .get("state_key")
                        .and_then(|v| v.as_str())
                        .map(ToOwned::to_owned),
                    content: event.get("content").cloned(),
                    timestamp: event.get("origin_server_ts").map(|v| v.to_string()),
                };

                if let Err(e) = processor.process_event(matrix_event).await {
                    error!("error processing event: {}", e);
                }
            }
        }
        Ok(())
    }
}

#[derive(Clone)]
pub struct MatrixAppservice {
    config: Arc<Config>,
    pub appservice: Appservice,
    handler: Arc<RwLock<BridgeAppserviceHandler>>,
}

#[derive(Debug, Clone)]
pub struct MatrixEvent {
    pub event_id: Option<String>,
    pub event_type: String,
    pub room_id: String,
    pub sender: String,
    pub state_key: Option<String>,
    pub content: Option<Value>,
    pub timestamp: Option<String>,
}

fn build_matrix_message_content(
    body: &str,
    reply_to: Option<&str>,
    edit_of: Option<&str>,
) -> Value {
    let mut content = json!({
        "msgtype": "m.text",
        "body": body,
    });

    if let Some(reply_id) = reply_to {
        content["m.relates_to"] = json!({
            "m.in_reply_to": {
                "event_id": reply_id
            }
        });
    }

    if let Some(edit_event_id) = edit_of {
        content["m.new_content"] = json!({
            "msgtype": "m.text",
            "body": body,
        });
        content["m.relates_to"] = json!({
            "rel_type": "m.replace",
            "event_id": edit_event_id,
        });
        content["body"] = format!("* {body}").into();
    }

    content
}

fn ghost_user_id(slack_user_id: &str, domain: &str) -> String {
    format!("@_slack_{}:{}", slack_user_id, domain)
}

fn is_namespaced_user(user_id: &str) -> bool {
    user_id.starts_with("@_slack_")
}

impl MatrixAppservice {
    pub async fn new(config: Arc<Config>) -> Result<Self> {
        info!(
            "initializing matrix appservice for {}",
            config.bridge.domain
        );

        let homeserver_url = Url::parse(&config.bridge.homeserver_url)?;
        let auth = MatrixAuth::new(&config.registration.appservice_token);
        let client = MatrixClient::new(homeserver_url, auth);

        let handler = Arc::new(RwLock::new(BridgeAppserviceHandler { processor: None }));

        // Use a wrapper to bridge AppserviceHandler to our internal handler
        struct HandlerWrapper(Arc<RwLock<BridgeAppserviceHandler>>);
        #[async_trait::async_trait]
        impl AppserviceHandler for HandlerWrapper {
            async fn on_transaction(&self, txn_id: &str, body: &Value) -> Result<()> {
                self.0.read().await.on_transaction(txn_id, body).await
            }
        }

        let appservice = Appservice::new(
            &config.registration.homeserver_token,
            &config.registration.appservice_token,
            client,
        )
        .with_appservice_id(&config.registration.bridge_id)
        .with_handler(Arc::new(HandlerWrapper(handler.clone())));

        Ok(Self {
            config,
            appservice,
            handler,
        })
    }

    pub fn config(&self) -> Arc<Config> {
        self.config.clone()
    }

    pub fn bot_user_id(&self) -> String {
        format!(
            "@{}:{}",
            self.config.registration.sender_localpart, self.config.bridge.domain
        )
    }

    pub fn is_namespaced_user(&self, user_id: &str) -> bool {
        is_namespaced_user(user_id)
    }

    async fn ensure_bot_joined_room(&self, room_id: &str) -> Result<bool> {
        let bot_user_id = self.bot_user_id();
        let membership = self
            .appservice
            .client
            .get_room_state_event(room_id, "m.room.member", &bot_user_id)
            .await
            .ok()
            .and_then(|state| {
                state
                    .get("membership")
                    .and_then(|value| value.as_str())
                    .map(ToOwned::to_owned)
            });

        match membership.as_deref() {
            Some("join") => Ok(false),
            Some("invite") => {
                let joined = self
                    .appservice
                    .client
                    .join_room(room_id)
                    .await
                    .with_context(|| {
                        format!(
                            "failed to auto-join invited room {} as {}",
                            room_id, bot_user_id
                        )
                    })?;
                info!(
                    "auto-joined invited room {} while sending notice as {}",
                    joined, bot_user_id
                );
                Ok(true)
            }
            Some(other) => {
                debug!(
                    "bot room membership does not permit auto-join room_id={} bot_user={} membership={}",
                    room_id, bot_user_id, other
                );
                Ok(false)
            }
            None => {
                debug!(
                    "no membership state found for bot in room room_id={} bot_user={}",
                    room_id, bot_user_id
                );
                Ok(false)
            }
        }
    }

    pub async fn set_processor(&self, processor: Arc<MatrixEventProcessor>) {
        self.handler.write().await.processor = Some(processor);
    }

    pub async fn start(&self) -> Result<()> {
        info!("matrix appservice starting");
        Ok(())
    }

    pub async fn create_ghost_user(
        &self,
        slack_user_id: &str,
        _username: &str,
        display_name: Option<&str>,
    ) -> Result<String> {
        let localpart = format!("_slack_{}", slack_user_id);
        let user_id = format!("@{}:{}", localpart, self.config.bridge.domain);

        let ghost_client = self.appservice.client.clone();
        ghost_client
            .impersonate_user_id(Some(&user_id), None::<&str>)
            .await;

        let _ = ghost_client
            .password_register(&localpart, "", display_name)
            .await;

        if let Some(display) = display_name {
            let _ = ghost_client.set_display_name(display).await;
        }

        Ok(user_id)
    }

    pub async fn create_room(
        &self,
        slack_channel_id: &str,
        name: &str,
        topic: Option<&str>,
    ) -> Result<String> {
        let alias_localpart = format!("_slack_{}", slack_channel_id);

        let visibility = match self.config.room.default_visibility.to_lowercase().as_str() {
            "public" => Some("public".to_string()),
            _ => Some("private".to_string()),
        };

        let opt = CreateRoom {
            visibility,
            room_alias_name: Some(alias_localpart),
            name: Some(name.to_owned()),
            topic: topic.map(ToOwned::to_owned),
            ..Default::default()
        };

        let room_id = self.appservice.client.create_room(&opt).await?;
        Ok(room_id)
    }

    pub async fn send_message(&self, room_id: &str, sender: &str, content: &str) -> Result<()> {
        self.send_message_with_metadata(room_id, sender, content, &[], None, None)
            .await
            .map(|_| ())
    }

    pub async fn send_notice(&self, room_id: &str, content: &str) -> Result<()> {
        match self.appservice.client.send_notice(room_id, content).await {
            Ok(_) => Ok(()),
            Err(err) => {
                let err_text = err.to_string();
                if err_text.contains("missing event_id in send response")
                    && self.ensure_bot_joined_room(room_id).await?
                {
                    self.appservice
                        .client
                        .send_notice(room_id, content)
                        .await
                        .with_context(|| {
                            format!("failed to send notice after auto-join room_id={}", room_id)
                        })?;
                    return Ok(());
                }

                Err(err).context(format!(
                    "failed to send notice room_id={} bot_user={}",
                    room_id,
                    self.bot_user_id()
                ))
            }
        }
    }

    pub async fn send_message_with_metadata(
        &self,
        room_id: &str,
        sender: &str,
        body: &str,
        _attachments: &[String],
        reply_to: Option<&str>,
        edit_of: Option<&str>,
    ) -> Result<String> {
        let ghost_client = self.appservice.client.clone();
        ghost_client
            .impersonate_user_id(Some(sender), None::<&str>)
            .await;

        let content = build_matrix_message_content(body, reply_to, edit_of);

        let event_id = ghost_client
            .send_event(room_id, "m.room.message", &content)
            .await?;

        Ok(event_id)
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn send_media_message(
        &self,
        room_id: &str,
        sender: &str,
        msgtype: &str,
        body: &str,
        url: &str,
        info: Option<&serde_json::Value>,
        reply_to: Option<&str>,
    ) -> Result<String> {
        let ghost_client = self.appservice.client.clone();
        ghost_client
            .impersonate_user_id(Some(sender), None::<&str>)
            .await;

        let mut content = json!({
            "msgtype": msgtype,
            "body": body,
            "url": url,
        });

        if let Some(info) = info {
            content["info"] = info.clone();
        }

        if let Some(reply_event_id) = reply_to {
            content["m.relates_to"] = json!({
                "m.in_reply_to": {
                    "event_id": reply_event_id
                }
            });
        }

        let event_id = ghost_client
            .send_event(room_id, "m.room.message", &content)
            .await?;

        Ok(event_id)
    }

    pub async fn upload_media(&self, media: &crate::media::MediaInfo) -> Result<String> {
        use reqwest::Client;

        let upload_url = format!(
            "{}/_matrix/media/v3/upload?filename={}",
            self.config.bridge.homeserver_url.trim_end_matches('/'),
            urlencoding::encode(&media.filename)
        );

        debug!("uploading media {} to Matrix", media.filename);

        let client = Client::new();
        let response = client
            .post(&upload_url)
            .header(
                "Authorization",
                format!("Bearer {}", self.config.registration.appservice_token),
            )
            .header("Content-Type", &media.content_type)
            .body(media.data.clone())
            .send()
            .await
            .map_err(|e| anyhow::anyhow!("failed to upload media: {}", e))?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!(
                "failed to upload media: {} - {}",
                status,
                body
            ));
        }

        let body_bytes = response
            .bytes()
            .await
            .map_err(|e| anyhow::anyhow!("failed to read response: {}", e))?;
        let json: Value = serde_json::from_slice(&body_bytes)
            .map_err(|e| anyhow::anyhow!("failed to parse response: {}", e))?;

        let content_uri = json
            .get("content_uri")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("no content_uri in response"))?
            .to_string();

        debug!("uploaded media to {}", content_uri);
        Ok(content_uri)
    }

    pub async fn redact_message(
        &self,
        room_id: &str,
        event_id: &str,
        reason: Option<&str>,
    ) -> Result<()> {
        let content = json!({
            "redacts": event_id,
            "reason": reason.unwrap_or(""),
        });
        self.appservice
            .client
            .send_event(room_id, "m.room.redaction", &content)
            .await?;
        Ok(())
    }

    pub async fn check_permission(
        &self,
        user_id: &str,
        room_id: &str,
        required_level: i64,
        _category: &str,
        _subcategory: &str,
    ) -> Result<bool> {
        let power_levels = self
            .appservice
            .client
            .get_room_state_event(room_id, "m.room.power_levels", "")
            .await;

        match power_levels {
            Ok(pl) => {
                let user_level = pl
                    .get("users")
                    .and_then(|u| u.get(user_id))
                    .and_then(|v| v.as_i64())
                    .unwrap_or_else(|| {
                        pl.get("users_default")
                            .and_then(|v| v.as_i64())
                            .unwrap_or(0)
                    });
                Ok(user_level >= required_level)
            }
            Err(_) => {
                // If we can't fetch power levels, default to denying
                Ok(false)
            }
        }
    }

    pub async fn ensure_ghost_user_registered(
        &self,
        slack_user_id: &str,
        username: Option<&str>,
    ) -> Result<()> {
        self.create_ghost_user(slack_user_id, slack_user_id, username)
            .await?;
        Ok(())
    }

    pub async fn get_room_name(&self, room_id: &str) -> Result<Option<String>> {
        let state = self
            .appservice
            .client
            .get_room_state_event(room_id, "m.room.name", "")
            .await
            .ok();

        Ok(state.and_then(|s| {
            s.get("name")
                .and_then(|n| n.as_str())
                .map(ToOwned::to_owned)
        }))
    }

    pub async fn get_room_topic(&self, room_id: &str) -> Result<Option<String>> {
        let state = self
            .appservice
            .client
            .get_room_state_event(room_id, "m.room.topic", "")
            .await
            .ok();

        Ok(state.and_then(|s| {
            s.get("topic")
                .and_then(|t| t.as_str())
                .map(ToOwned::to_owned)
        }))
    }

    pub async fn set_room_name(&self, room_id: &str, name: &str) -> Result<()> {
        let event_content = json!({ "name": name });
        self.appservice
            .client
            .send_state_event(room_id, "m.room.name", "", &event_content)
            .await?;
        Ok(())
    }

    pub async fn set_room_topic(&self, room_id: &str, topic: &str) -> Result<()> {
        let event_content = json!({ "topic": topic });
        self.appservice
            .client
            .send_state_event(room_id, "m.room.topic", "", &event_content)
            .await?;
        Ok(())
    }

    pub async fn get_user_profile(
        &self,
        user_id: &str,
    ) -> Result<Option<(String, Option<String>)>> {
        let profile = self.appservice.client.profile(user_id).await;
        match profile {
            Ok(profile) => {
                let displayname = profile.displayname.unwrap_or_else(|| user_id.to_string());
                let avatar_url = profile.avatar_url;
                Ok(Some((displayname, avatar_url)))
            }
            Err(_) => Ok(None),
        }
    }

    pub async fn set_slack_user_presence(
        &self,
        slack_user_id: &str,
        presence: &str,
        status_message: &str,
    ) -> Result<()> {
        let user_id = ghost_user_id(slack_user_id, &self.config.bridge.domain);

        let ghost_client = self.appservice.client.clone();
        ghost_client
            .impersonate_user_id(Some(&user_id), None::<&str>)
            .await;

        let presence_status = match presence {
            "online" => matrix_bot_sdk::models::Presence::Online,
            "unavailable" => matrix_bot_sdk::models::Presence::Unavailable,
            _ => matrix_bot_sdk::models::Presence::Offline,
        };

        ghost_client
            .set_presence_status(presence_status, Some(status_message))
            .await?;
        Ok(())
    }

    pub async fn set_slack_user_typing(
        &self,
        room_id: &str,
        slack_user_id: &str,
        typing: bool,
        timeout_ms: Option<u64>,
    ) -> Result<()> {
        let user_id = ghost_user_id(slack_user_id, &self.config.bridge.domain);

        self.appservice
            .client
            .set_typing(room_id, &user_id, typing, timeout_ms)
            .await?;
        Ok(())
    }

    pub async fn send_reaction_as_ghost(
        &self,
        room_id: &str,
        event_id: &str,
        slack_user_id: &str,
        emoji: &str,
    ) -> Result<()> {
        let user_id = ghost_user_id(slack_user_id, &self.config.bridge.domain);
        let content = serde_json::json!({
            "m.relates_to": {
                "rel_type": "m.annotation",
                "event_id": event_id,
                "key": emoji
            }
        });
        self.appservice
            .client
            .send_raw_event(room_id, "m.reaction", &content, Some(&user_id))
            .await?;
        Ok(())
    }

    pub async fn redact_reaction_as_ghost(
        &self,
        room_id: &str,
        _event_id: &str,
        slack_user_id: &str,
        _emoji: &str,
    ) -> Result<()> {
        let user_id = ghost_user_id(slack_user_id, &self.config.bridge.domain);
        debug!("reaction redaction requested for user={} room={}", user_id, room_id);
        Ok(())
    }

    pub async fn set_room_alias(&self, room_id: &str, alias: &str) -> Result<()> {
        self.appservice
            .client
            .create_room_alias(alias, room_id)
            .await?;
        Ok(())
    }

    pub async fn leave_room(&self, room_id: &str) -> Result<()> {
        self.appservice.client.leave_room(room_id, None).await?;
        Ok(())
    }

    pub async fn send_text(&self, room_id: &str, content: &str) -> Result<()> {
        self.appservice.client.send_text(room_id, content).await?;
        Ok(())
    }

    pub async fn get_joined_rooms(&self) -> Result<Vec<String>> {
        let rooms = self.appservice.client.get_joined_rooms().await?;
        Ok(rooms)
    }

    pub async fn get_room_members(&self, room_id: &str) -> Result<Vec<String>> {
        let members = self
            .appservice
            .client
            .get_room_members(room_id, None, None)
            .await?;
        Ok(members.into_iter().map(|m| m.user_id).collect())
    }

    pub async fn send_read_receipt(
        &self,
        room_id: &str,
        event_id: &str,
        user_id: &str,
    ) -> Result<()> {
        let ghost_client = self.appservice.client.clone();
        ghost_client
            .impersonate_user_id(Some(user_id), None::<&str>)
            .await;

        let url = format!(
            "{}/_matrix/client/v3/rooms/{}/receipt/m.read/{}",
            self.config.bridge.homeserver_url.trim_end_matches('/'),
            urlencoding::encode(room_id),
            urlencoding::encode(event_id)
        );

        debug!(
            "sending read receipt for user={} room={} event={}",
            user_id, room_id, event_id
        );

        let client = reqwest::Client::new();
        let response = client
            .post(&url)
            .header(
                "Authorization",
                format!("Bearer {}", self.config.registration.appservice_token),
            )
            .json(&serde_json::json!({}))
            .send()
            .await
            .map_err(|e| anyhow::anyhow!("failed to send read receipt: {}", e))?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            warn!("failed to send read receipt: {} - {}", status, body);
        }

        Ok(())
    }

    pub async fn create_dm_room(&self, invite_user: &str) -> Result<String> {
        use matrix_bot_sdk::models::CreateRoom;
        let options = CreateRoom {
            visibility: Some("private".to_string()),
            invite: vec![invite_user.to_string()],
            is_direct: true,
            ..Default::default()
        };
        let room_id = self.appservice.client.create_room(&options).await?;
        Ok(room_id)
    }

    pub async fn invite_user_to_room(&self, room_id: &str, user_id: &str) -> Result<()> {
        let url = format!(
            "{}/_matrix/client/v3/rooms/{}/invite",
            self.config.bridge.homeserver_url.trim_end_matches('/'),
            urlencoding::encode(room_id)
        );

        debug!("inviting user {} to room {}", user_id, room_id);

        let client = reqwest::Client::new();
        let response = client
            .post(&url)
            .header(
                "Authorization",
                format!("Bearer {}", self.config.registration.appservice_token),
            )
            .json(&serde_json::json!({
                "user_id": user_id
            }))
            .send()
            .await
            .map_err(|e| anyhow::anyhow!("failed to invite user to room: {}", e))?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            warn!("failed to invite user to room: {} - {}", status, body);
        }

        Ok(())
    }

    pub async fn kick_user_from_room(
        &self,
        room_id: &str,
        user_id: &str,
        reason: Option<&str>,
    ) -> Result<()> {
        let url = format!(
            "{}/_matrix/client/v3/rooms/{}/kick",
            self.config.bridge.homeserver_url.trim_end_matches('/'),
            urlencoding::encode(room_id)
        );

        debug!("kicking user {} from room {}", user_id, room_id);

        let client = reqwest::Client::new();
        let response = client
            .post(&url)
            .header(
                "Authorization",
                format!("Bearer {}", self.config.registration.appservice_token),
            )
            .json(&serde_json::json!({
                "user_id": user_id,
                "reason": reason.unwrap_or("")
            }))
            .send()
            .await
            .map_err(|e| anyhow::anyhow!("failed to kick user from room: {}", e))?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            warn!("failed to kick user from room: {} - {}", status, body);
        }

        Ok(())
    }

    pub async fn ban_user_from_room(
        &self,
        room_id: &str,
        user_id: &str,
        reason: Option<&str>,
    ) -> Result<()> {
        let url = format!(
            "{}/_matrix/client/v3/rooms/{}/ban",
            self.config.bridge.homeserver_url.trim_end_matches('/'),
            urlencoding::encode(room_id)
        );

        debug!("banning user {} from room {}", user_id, room_id);

        let client = reqwest::Client::new();
        let response = client
            .post(&url)
            .header(
                "Authorization",
                format!("Bearer {}", self.config.registration.appservice_token),
            )
            .json(&serde_json::json!({
                "user_id": user_id,
                "reason": reason.unwrap_or("")
            }))
            .send()
            .await
            .map_err(|e| anyhow::anyhow!("failed to ban user from room: {}", e))?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            warn!("failed to ban user from room: {} - {}", status, body);
        }

        Ok(())
    }

    pub async fn unban_user_from_room(&self, room_id: &str, user_id: &str) -> Result<()> {
        let url = format!(
            "{}/_matrix/client/v3/rooms/{}/unban",
            self.config.bridge.homeserver_url.trim_end_matches('/'),
            urlencoding::encode(room_id)
        );

        debug!("unbanning user {} from room {}", user_id, room_id);

        let client = reqwest::Client::new();
        let response = client
            .post(&url)
            .header(
                "Authorization",
                format!("Bearer {}", self.config.registration.appservice_token),
            )
            .json(&serde_json::json!({
                "user_id": user_id
            }))
            .send()
            .await
            .map_err(|e| anyhow::anyhow!("failed to unban user from room: {}", e))?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            warn!("failed to unban user from room: {} - {}", status, body);
        }

        Ok(())
    }

    pub async fn set_ghost_displayname(
        &self,
        slack_user_id: &str,
        displayname: &str,
    ) -> Result<()> {
        let user_id = ghost_user_id(slack_user_id, &self.config.bridge.domain);

        let ghost_client = self.appservice.client.clone();
        ghost_client
            .impersonate_user_id(Some(&user_id), None::<&str>)
            .await;

        ghost_client.set_display_name(displayname).await?;
        Ok(())
    }

    pub async fn set_ghost_avatar(&self, slack_user_id: &str, avatar_url: &str) -> Result<()> {
        let user_id = ghost_user_id(slack_user_id, &self.config.bridge.domain);

        let ghost_client = self.appservice.client.clone();
        ghost_client
            .impersonate_user_id(Some(&user_id), None::<&str>)
            .await;

        ghost_client.set_avatar_url(avatar_url).await?;
        Ok(())
    }

    pub async fn upload_media_for_ghost(
        &self,
        _slack_user_id: &str,
        data: &[u8],
        content_type: &str,
        filename: &str,
    ) -> Result<String> {
        let media = crate::media::MediaInfo {
            data: data.to_vec(),
            content_type: content_type.to_string(),
            filename: filename.to_string(),
            size: data.len(),
        };
        self.upload_media(&media).await
    }

    pub async fn invite_ghost_to_room(&self, slack_user_id: &str, room_id: &str) -> Result<()> {
        let ghost_user_id = ghost_user_id(slack_user_id, &self.config.bridge.domain);
        self.invite_user_to_room(room_id, &ghost_user_id).await
    }

    pub async fn kick_ghost_from_room(&self, slack_user_id: &str, room_id: &str) -> Result<()> {
        let ghost_user_id = ghost_user_id(slack_user_id, &self.config.bridge.domain);
        self.kick_user_from_room(room_id, &ghost_user_id, None)
            .await
    }

    pub async fn set_ghost_room_displayname(
        &self,
        slack_user_id: &str,
        room_id: &str,
        displayname: &str,
    ) -> Result<()> {
        let user_id = ghost_user_id(slack_user_id, &self.config.bridge.domain);

        let content = json!({
            "displayname": displayname,
            "membership": "join"
        });

        self.appservice
            .client
            .send_state_event(room_id, "m.room.member", &user_id, &content)
            .await?;

        Ok(())
    }

    pub async fn set_ghost_room_avatar(
        &self,
        slack_user_id: &str,
        room_id: &str,
        avatar_mxc: &str,
    ) -> Result<()> {
        let user_id = ghost_user_id(slack_user_id, &self.config.bridge.domain);

        let content = json!({
            "avatar_url": avatar_mxc,
            "membership": "join"
        });

        self.appservice
            .client
            .send_state_event(room_id, "m.room.member", &user_id, &content)
            .await?;

        Ok(())
    }

    pub async fn set_ghost_room_roles(
        &self,
        slack_user_id: &str,
        room_id: &str,
        roles: &[String],
    ) -> Result<()> {
        let user_id = ghost_user_id(slack_user_id, &self.config.bridge.domain);

        let content = json!({
            "membership": "join",
            "slack_roles": roles
        });

        self.appservice
            .client
            .send_state_event(room_id, "m.room.member", &user_id, &content)
            .await?;

        Ok(())
    }

    pub async fn set_room_avatar(&self, room_id: &str, avatar_mxc: &str) -> Result<()> {
        let event_content = json!({ "url": avatar_mxc });
        self.appservice
            .client
            .send_state_event(room_id, "m.room.avatar", "", &event_content)
            .await?;
        Ok(())
    }

    pub async fn set_room_visibility(&self, room_id: &str, visibility: &str) -> Result<()> {
        let url = format!(
            "{}/_matrix/client/v3/rooms/{}/state/m.room.join_rules",
            self.config.bridge.homeserver_url.trim_end_matches('/'),
            urlencoding::encode(room_id)
        );

        let client = reqwest::Client::new();
        let response = client
            .put(&url)
            .header(
                "Authorization",
                format!("Bearer {}", self.config.registration.appservice_token),
            )
            .json(&serde_json::json!({
                "join_rule": visibility
            }))
            .send()
            .await
            .map_err(|e| anyhow::anyhow!("failed to set room visibility: {}", e))?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            warn!("failed to set room visibility: {} - {}", status, body);
        }

        Ok(())
    }

    pub async fn get_room_avatar(&self, room_id: &str) -> Result<Option<String>> {
        let state = self
            .appservice
            .client
            .get_room_state_event(room_id, "m.room.avatar", "")
            .await
            .ok();

        Ok(state.and_then(|s| s.get("url").and_then(|u| u.as_str()).map(ToOwned::to_owned)))
    }

    pub fn registration_preview(&self) -> Value {
        json!({
            "id": self.config.registration.bridge_id,
            "url": format!("http://{}:{}", self.config.bridge.bind_address, self.config.bridge.port),
            "as_token": self.config.registration.appservice_token,
            "hs_token": self.config.registration.homeserver_token,
            "sender_localpart": self.config.registration.sender_localpart,
            "rate_limited": false,
            "namespaces": {
                "users": [{
                    "exclusive": true,
                    "regex": format!("@_slack_.*:{}", self.config.bridge.domain)
                }],
                "aliases": [{
                    "exclusive": true,
                    "regex": format!("#_slack_.*:{}", self.config.bridge.domain)
                }],
                "rooms": []
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{build_matrix_message_content, ghost_user_id, is_namespaced_user};

    #[test]
    fn message_content_adds_reply_relation() {
        let content = build_matrix_message_content("hello", Some("$event123"), None);
        assert_eq!(content["msgtype"], "m.text");
        assert_eq!(content["body"], "hello");
        assert_eq!(
            content["m.relates_to"]["m.in_reply_to"]["event_id"],
            "$event123"
        );
        assert!(content.get("m.new_content").is_none());
    }

    #[test]
    fn message_content_adds_edit_relation() {
        let content = build_matrix_message_content("new body", None, Some("$old_event"));
        assert_eq!(content["msgtype"], "m.text");
        assert_eq!(content["body"], "* new body");
        assert_eq!(content["m.new_content"]["body"], "new body");
        assert_eq!(content["m.relates_to"]["rel_type"], "m.replace");
        assert_eq!(content["m.relates_to"]["event_id"], "$old_event");
    }

    #[test]
    fn ghost_user_id_uses_expected_namespace() {
        let user_id = ghost_user_id("12345", "example.org");
        assert_eq!(user_id, "@_slack_12345:example.org");
    }

    #[test]
    fn is_namespaced_user_detects_ghost_users() {
        assert!(is_namespaced_user("@_slack_12345:example.org"));
        assert!(is_namespaced_user("@_slack_:example.org"));
        assert!(!is_namespaced_user("@alice:example.org"));
        assert!(!is_namespaced_user("@_slack:example.org"));
    }

    #[test]
    fn message_content_prefers_edit_relation_over_reply_relation() {
        let content =
            build_matrix_message_content("edited", Some("$reply_target"), Some("$edit_target"));

        assert_eq!(content["body"], "* edited");
        assert_eq!(content["m.relates_to"]["rel_type"], "m.replace");
        assert_eq!(content["m.relates_to"]["event_id"], "$edit_target");
        assert!(content["m.relates_to"].get("m.in_reply_to").is_none());
    }
}

