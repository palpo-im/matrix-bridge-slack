use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use async_trait::async_trait;
use chrono::Utc;
use serde_json::json;
use tracing::{debug, info, warn};

use crate::cache::AsyncTimedCache;
use crate::db::{DatabaseManager, MessageMapping, RoomMapping};
use crate::slack::{
    SlackClient, SlackCommandHandler, SlackCommandOutcome, ModerationAction,
};
use crate::emoji::EmojiHandler;
use crate::matrix::{MatrixAppservice, MatrixCommandHandler, MatrixCommandOutcome, MatrixEvent};
use crate::media::MediaHandler;

pub mod blocker;
pub mod logic;
pub mod message_flow;
pub mod presence_handler;
pub mod provisioning;
pub mod queue;
pub mod user_sync;

use self::logic::{
    action_keyword, apply_message_relation_mappings, build_slack_typing_request,
    slack_delete_redaction_request, preview_text, should_forward_slack_typing,
};
use self::message_flow::{
    SlackInboundMessage, MessageFlow, OutboundSlackMessage, OutboundMatrixMessage,
};
use self::presence_handler::{
    SlackPresence, MatrixPresenceState, MatrixPresenceTarget, PresenceHandler,
};
use self::provisioning::{ApprovalResponseStatus, ProvisioningCoordinator, ProvisioningError};
use self::queue::ChannelQueue;

#[derive(Debug, Clone)]
pub struct SlackMessageContext {
    pub channel_id: String,
    pub source_message_id: Option<String>,
    pub sender_id: String,
    pub content: String,
    pub attachments: Vec<String>,
    pub reply_to: Option<String>,
    pub edit_of: Option<String>,
    pub permissions: HashSet<String>,
}

const ROOM_CACHE_TTL_SECS: u64 = 900;

#[derive(Clone)]
pub struct BridgeCore {
    matrix_client: Arc<MatrixAppservice>,
    slack_client: Arc<SlackClient>,
    db_manager: Arc<DatabaseManager>,
    message_flow: Arc<MessageFlow>,
    matrix_command_handler: Arc<MatrixCommandHandler>,
    slack_command_handler: Arc<SlackCommandHandler>,
    presence_handler: Arc<PresenceHandler>,
    provisioning: Arc<ProvisioningCoordinator>,
    media_handler: Arc<MediaHandler>,
    emoji_handler: Arc<EmojiHandler>,
    message_queue: Arc<ChannelQueue>,
    room_cache: Arc<AsyncTimedCache<String, RoomMapping>>,
}

impl BridgeCore {
    pub fn new(
        matrix_client: Arc<MatrixAppservice>,
        slack_client: Arc<SlackClient>,
        db_manager: Arc<DatabaseManager>,
    ) -> Self {
        let bridge_config = matrix_client.config().bridge.clone();
        let homeserver_url = matrix_client.config().bridge.homeserver_url.clone();

        let media_handler = Arc::new(MediaHandler::new(&homeserver_url));
        let emoji_handler = Arc::new(EmojiHandler::new(
            db_manager.clone(),
            media_handler.clone(),
            homeserver_url.clone(),
        ));

        Self {
            message_flow: Arc::new(MessageFlow::with_emoji_handler(
                matrix_client.clone(),
                slack_client.clone(),
                Some(emoji_handler.clone()),
            )),
            matrix_command_handler: Arc::new(MatrixCommandHandler::new(
                bridge_config.enable_self_service_bridging,
                None,
            )),
            slack_command_handler: Arc::new(SlackCommandHandler::new()),
            presence_handler: Arc::new(PresenceHandler::new(None)),
            provisioning: Arc::new(ProvisioningCoordinator::default()),
            media_handler,
            emoji_handler,
            message_queue: Arc::new(ChannelQueue::new()),
            room_cache: Arc::new(AsyncTimedCache::new(Duration::from_secs(
                ROOM_CACHE_TTL_SECS,
            ))),
            matrix_client,
            slack_client,
            db_manager,
        }
    }

    pub async fn start(&self) -> Result<()> {
        self.matrix_client.start().await?;
        self.slack_client.start().await?;

        info!("bridge core started");

        let bridge_config = self.matrix_client.config().bridge.clone();
        let presence_interval_ms = bridge_config.presence_interval.max(250);
        let mut ticker = tokio::time::interval(Duration::from_millis(presence_interval_ms));
        loop {
            ticker.tick().await;
            if !bridge_config.disable_presence {
                self.presence_handler
                    .process_next(self.matrix_client.as_ref())
                    .await?;
            }
        }
    }

    pub async fn send_to_slack(
        &self,
        slack_channel_id: String,
        _matrix_sender: String,
        content: String,
    ) -> Result<()> {
        self.send_to_slack_message(
            &slack_channel_id,
            OutboundSlackMessage {
                content,
                reply_to: None,
                edit_of: None,
                attachments: Vec::new(),
                embed: None,
                use_embed: false,
            },
        )
        .await
    }

    pub async fn send_to_matrix(
        &self,
        matrix_room_id: String,
        slack_sender: String,
        content: String,
    ) -> Result<()> {
        self.send_to_matrix_message(
            &matrix_room_id,
            &slack_sender,
            OutboundMatrixMessage {
                body: content,
                reply_to: None,
                edit_of: None,
                attachments: Vec::new(),
            },
        )
        .await
        .map(|_| ())
    }

    async fn get_room_mapping_cached(&self, matrix_room_id: &str) -> Result<Option<RoomMapping>> {
        if let Some(cached) = self.room_cache.get(&matrix_room_id.to_string()).await {
            debug!("room cache hit for {}", matrix_room_id);
            return Ok(Some(cached));
        }

        debug!("room cache miss for {}", matrix_room_id);
        let mapping = self
            .db_manager
            .room_store()
            .get_room_by_matrix_room(matrix_room_id)
            .await?;

        if let Some(ref m) = mapping {
            self.room_cache
                .insert(matrix_room_id.to_string(), m.clone())
                .await;
        }

        Ok(mapping)
    }

    fn slack_user_id_from_mxid(&self, mxid: &str) -> Option<String> {
        let localpart = mxid.strip_prefix("@_slack_")?;
        let suffix = format!(":{}", self.matrix_client.config().bridge.domain);
        let slack_user_id = localpart.strip_suffix(&suffix)?;
        if slack_user_id.is_empty() || slack_user_id.contains(':') {
            return None;
        }
        Some(slack_user_id.to_string())
    }

    pub async fn handle_matrix_message(&self, event: &MatrixEvent) -> Result<()> {
        if self.matrix_client.is_namespaced_user(&event.sender) {
            debug!(
                "matrix inbound dropped room_id={} sender={} reason=echo_from_ghost",
                event.room_id, event.sender
            );
            return Ok(());
        }

        let body = event
            .content
            .as_ref()
            .map(crate::parsers::MessageUtils::extract_plain_text)
            .unwrap_or_default();

        debug!(
            "matrix inbound message event_id={:?} room_id={} sender={} type={} body_len={} body_preview={}",
            event.event_id,
            event.room_id,
            event.sender,
            event.event_type,
            body.len(),
            preview_text(&body)
        );

        let room_mapping = self
            .db_manager
            .room_store()
            .get_room_by_matrix_room(&event.room_id)
            .await?;

        debug!(
            "matrix inbound mapping lookup room_id={} mapped={}",
            event.room_id,
            room_mapping.is_some()
        );

        if self.matrix_command_handler.is_command(&body) {
            debug!(
                "matrix inbound command detected room_id={} sender={} command_preview={}",
                event.room_id,
                event.sender,
                preview_text(&body)
            );
            let has_permissions = self
                .matrix_client
                .check_permission(
                    &event.sender,
                    &event.room_id,
                    50,
                    "events",
                    "m.room.power_levels",
                )
                .await
                .unwrap_or(false);
            debug!(
                "matrix command permission result room_id={} sender={} granted={}",
                event.room_id, event.sender, has_permissions
            );
            let outcome = self
                .matrix_command_handler
                .handle(&body, room_mapping.is_some(), |_| Ok(has_permissions));
            self.handle_matrix_command_outcome(outcome, event).await?;
            return Ok(());
        }

        let Some(mapping) = room_mapping else {
            debug!(
                "matrix inbound dropped room_id={} reason=no_slack_mapping",
                event.room_id
            );
            return Ok(());
        };
        let Some(message) = MessageFlow::parse_matrix_event(event) else {
            debug!(
                "matrix inbound dropped room_id={} event_id={:?} reason=unsupported_or_unparseable",
                event.room_id, event.event_id
            );
            return Ok(());
        };

        let outbound = self.message_flow.matrix_to_slack(&message);
        debug!(
            "matrix->slack outbound prepared room_id={} slack_channel={} reply_to={:?} edit_of={:?} attachments={} content_len={} content_preview={}",
            mapping.matrix_room_id,
            mapping.slack_channel_id,
            outbound.reply_to,
            outbound.edit_of,
            outbound.attachments.len(),
            outbound.content.len(),
            preview_text(&outbound.content)
        );

        let downloaded_attachments = self
            .download_matrix_attachments(&outbound.attachments)
            .await;

        self.send_to_slack_with_attachments(
            &mapping.slack_channel_id,
            outbound,
            &event.sender,
            downloaded_attachments,
        )
        .await?;
        Ok(())
    }

    async fn download_matrix_attachments(
        &self,
        urls: &[String],
    ) -> Vec<(String, Option<crate::media::MediaInfo>)> {
        let mut results = Vec::new();
        for url in urls {
            if url.starts_with("mxc://") {
                match self.media_handler.download_matrix_media(url).await {
                    Ok(media) => {
                        if media.size > 8 * 1024 * 1024 {
                            warn!(
                                "matrix attachment too large for slack: {} bytes, sending URL instead",
                                media.size
                            );
                            results.push((url.clone(), None));
                        } else {
                            results.push((url.clone(), Some(media)));
                        }
                    }
                    Err(e) => {
                        warn!("failed to download matrix attachment {}: {}", url, e);
                        results.push((url.clone(), None));
                    }
                }
            } else {
                results.push((url.clone(), None));
            }
        }
        results
    }

    async fn get_reply_info(&self, matrix_event_id: &str) -> Option<(String, String)> {
        let mapping = self
            .db_manager
            .message_store()
            .get_by_matrix_event_id(matrix_event_id)
            .await
            .ok()
            .flatten()?;

        let sender_displayname = self
            .matrix_client
            .get_user_profile(&mapping.matrix_room_id)
            .await
            .ok()
            .flatten()
            .map(|(name, _)| name)
            .unwrap_or_else(|| "Unknown".to_string());

        Some((sender_displayname, "(reply)".to_string()))
    }

    pub async fn send_to_slack_with_embed(
        &self,
        slack_channel_id: &str,
        outbound: message_flow::OutboundSlackMessage,
        attachments: Vec<(String, Option<crate::media::MediaInfo>)>,
    ) -> Result<()> {
        for (original_url, media_opt) in &attachments {
            if let Some(media) = media_opt {
                if media.size > 8 * 1024 * 1024 {
                    warn!(
                        "matrix attachment too large for slack: {} bytes, sending URL instead",
                        media.size
                    );
                    let content = format!("{}: {}", media.filename, original_url);
                    self.slack_client
                        .send_message(slack_channel_id, &content)
                        .await?;
                } else {
                    match self
                        .slack_client
                        .send_file_as_user(
                            slack_channel_id,
                            &media.data,
                            &media.content_type,
                            &media.filename,
                            None,
                            None,
                        )
                        .await
                    {
                        Ok(msg_id) => {
                            info!(
                                "uploaded matrix attachment to slack channel={} file={} size={}",
                                slack_channel_id, media.filename, media.size
                            );
                            let _ = msg_id;
                        }
                        Err(e) => {
                            warn!(
                                "failed to upload attachment to slack: {}, sending URL instead",
                                e
                            );
                            let content = format!("{}: {}", media.filename, original_url);
                            self.slack_client
                                .send_message(slack_channel_id, &content)
                                .await?;
                        }
                    }
                }
            } else {
                let content = format!("Attachment: {}", original_url);
                self.slack_client
                    .send_message(slack_channel_id, &content)
                    .await?;
            }
        }

        if let Some(ref embed) = outbound.embed {
            if let Some(ref author) = embed.author {
                self.slack_client
                    .send_embed_as_user(
                        slack_channel_id,
                        embed,
                        Some(&author.name),
                        author.icon_url.as_deref(),
                    )
                    .await?;
            } else {
                self.slack_client
                    .send_embed_as_user(slack_channel_id, embed, None, None)
                    .await?;
            }
        } else if !outbound.content.is_empty() {
            self.slack_client
                .send_message(slack_channel_id, &outbound.content)
                .await?;
        }

        Ok(())
    }

    pub async fn send_to_slack_with_attachments(
        &self,
        slack_channel_id: &str,
        outbound: OutboundSlackMessage,
        matrix_sender: &str,
        attachments: Vec<(String, Option<crate::media::MediaInfo>)>,
    ) -> Result<()> {
        let (username, avatar_url) = self
            .matrix_client
            .get_user_profile(matrix_sender)
            .await
            .unwrap_or(None)
            .unwrap_or_else(|| (matrix_sender.to_string(), None));

        let avatar_for_slack = avatar_url.as_ref().map(|url| {
            if url.starts_with("mxc://") {
                let mxc_url = url.trim_start_matches("mxc://");
                let homeserver = &self.matrix_client.config().bridge.homeserver_url;
                format!(
                    "{}/_matrix/media/r0/download/{}",
                    homeserver.trim_end_matches('/'),
                    mxc_url
                )
            } else {
                url.to_string()
            }
        });

        for (original_url, media_opt) in &attachments {
            if let Some(media) = media_opt {
                if media.size > 8 * 1024 * 1024 {
                    warn!(
                        "matrix attachment too large for slack: {} bytes, sending URL instead",
                        media.size
                    );
                    let content = format!("{}: {}", media.filename, original_url);
                    self.slack_client
                        .send_message_with_metadata_as_user(
                            slack_channel_id,
                            &content,
                            &[],
                            None,
                            None,
                            Some(&username),
                            avatar_for_slack.as_deref(),
                        )
                        .await?;
                } else {
                    match self
                        .slack_client
                        .send_file_as_user(
                            slack_channel_id,
                            &media.data,
                            &media.content_type,
                            &media.filename,
                            Some(&username),
                            avatar_for_slack.as_deref(),
                        )
                        .await
                    {
                        Ok(msg_id) => {
                            info!(
                                "uploaded matrix attachment to slack channel={} file={} size={}",
                                slack_channel_id, media.filename, media.size
                            );
                            let _ = msg_id;
                        }
                        Err(e) => {
                            warn!(
                                "failed to upload attachment to slack: {}, sending URL instead",
                                e
                            );
                            let content = format!("{}: {}", media.filename, original_url);
                            self.slack_client
                                .send_message_with_metadata_as_user(
                                    slack_channel_id,
                                    &content,
                                    &[],
                                    None,
                                    None,
                                    Some(&username),
                                    avatar_for_slack.as_deref(),
                                )
                                .await?;
                        }
                    }
                }
            } else {
                let content = format!("Attachment: {}", original_url);
                self.slack_client
                    .send_message_with_metadata_as_user(
                        slack_channel_id,
                        &content,
                        &[],
                        None,
                        None,
                        Some(&username),
                        avatar_for_slack.as_deref(),
                    )
                    .await?;
            }
        }

        if !outbound.content.is_empty() {
            self.slack_client
                .send_message_with_metadata_as_user(
                    slack_channel_id,
                    &outbound.content,
                    &[],
                    outbound.reply_to.as_deref(),
                    outbound.edit_of.as_deref(),
                    Some(&username),
                    avatar_for_slack.as_deref(),
                )
                .await?;
        }

        Ok(())
    }

    async fn handle_matrix_command_outcome(
        &self,
        outcome: MatrixCommandOutcome,
        event: &MatrixEvent,
    ) -> Result<()> {
        match outcome {
            MatrixCommandOutcome::Ignored => {}
            MatrixCommandOutcome::Reply(reply) => {
                self.matrix_client
                    .send_notice(&event.room_id, &reply)
                    .await?;
            }
            MatrixCommandOutcome::BridgeRequested {
                guild_id,
                channel_id,
            } => {
                let reply = self
                    .request_bridge_matrix_room(
                        &event.room_id,
                        &event.sender,
                        &guild_id,
                        &channel_id,
                    )
                    .await?;
                self.matrix_client
                    .send_notice(&event.room_id, &reply)
                    .await?;
            }
            MatrixCommandOutcome::UnbridgeRequested => {
                let reply = self.unbridge_matrix_room(&event.room_id).await?;
                self.matrix_client
                    .send_notice(&event.room_id, &reply)
                    .await?;
            }
        }
        Ok(())
    }

    pub async fn handle_matrix_member(&self, event: &MatrixEvent) -> Result<()> {
        if let Some(content) = event.content.as_ref().and_then(|c| c.as_object())
            && let Some(membership) = content.get("membership").and_then(|v| v.as_str())
        {
            let bot_user_id = self.matrix_client.bot_user_id();
            if membership == "invite" && event.state_key.as_deref() == Some(bot_user_id.as_str()) {
                match self
                    .matrix_client
                    .appservice
                    .client
                    .join_room(&event.room_id)
                    .await
                {
                    Ok(joined) => {
                        info!("joined invited room {}", joined);
                    }
                    Err(err) => {
                        warn!("failed to join invited room {}: {}", event.room_id, err);
                    }
                }
                return Ok(());
            }
            if membership == "invite" {
                debug!(
                    "matrix invite ignored room_id={} state_key={:?} expected_bot={} sender={}",
                    event.room_id, event.state_key, bot_user_id, event.sender
                );
            }

            if self.matrix_client.is_namespaced_user(&event.sender) {
                debug!(
                    "matrix member dropped room_id={} sender={} reason=echo_from_ghost",
                    event.room_id, event.sender
                );
                return Ok(());
            }

            if (membership == "leave" || membership == "ban")
                && let Some(state_key) = &event.state_key
                && event.sender != *state_key
            {
                let room_mapping = self.get_room_mapping_cached(&event.room_id).await?;
                let Some(mapping) = room_mapping else {
                    debug!(
                        "matrix moderation ignored room_id={} reason=no_slack_mapping",
                        event.room_id
                    );
                    return Ok(());
                };

                let Some(slack_user_id) = self.slack_user_id_from_mxid(state_key) else {
                    debug!(
                        "matrix moderation ignored room_id={} state_key={} reason=not_slack_ghost",
                        event.room_id, state_key
                    );
                    return Ok(());
                };

                let reason = content
                    .get("reason")
                    .and_then(|v| v.as_str())
                    .unwrap_or("No reason provided");

                if let Err(err) = self
                    .slack_client
                    .deny_channel_member_permissions(&mapping.slack_channel_id, &slack_user_id)
                    .await
                {
                    warn!(
                        "failed to apply slack deny overwrite for user={} channel={} room={} membership={}: {}",
                        slack_user_id, mapping.slack_channel_id, event.room_id, membership, err
                    );
                }

                let action_word = if membership == "ban" {
                    "banned"
                } else {
                    "kicked"
                };
                let notice = format!(
                    "Matrix moderation: `{}` was {} by `{}`. Reason: {}",
                    state_key, action_word, event.sender, reason
                );
                if let Err(err) = self
                    .slack_client
                    .send_message(&mapping.slack_channel_id, &notice)
                    .await
                {
                    warn!(
                        "failed to post matrix moderation notice to channel {}: {}",
                        mapping.slack_channel_id, err
                    );
                }

                if membership == "leave" {
                    let kick_for = self.matrix_client.config().room.kick_for;
                    if kick_for > 0 {
                        let target_user = state_key.clone();
                        let room_id = event.room_id.clone();
                        let channel_id = mapping.slack_channel_id.clone();
                        let slack_client = self.slack_client.clone();
                        let restore_user_id = slack_user_id.clone();

                        tokio::spawn(async move {
                            tokio::time::sleep(tokio::time::Duration::from_millis(kick_for)).await;
                            match slack_client
                                .clear_channel_member_overwrite(&channel_id, &restore_user_id)
                                .await
                            {
                                Ok(()) => {
                                    info!(
                                        "restored slack channel permissions for user={} matrix_user={} room={} channel={} after {}ms",
                                        restore_user_id, target_user, room_id, channel_id, kick_for
                                    );
                                }
                                Err(err) => {
                                    warn!(
                                        "failed to restore slack channel permissions for user={} matrix_user={} room={} channel={} after {}ms: {}",
                                        restore_user_id,
                                        target_user,
                                        room_id,
                                        channel_id,
                                        kick_for,
                                        err
                                    );
                                }
                            }
                        });
                    }
                }
            }
        }
        Ok(())
    }

    pub async fn handle_matrix_encryption(&self, event: &MatrixEvent) -> Result<()> {
        let room_mapping = self.get_room_mapping_cached(&event.room_id).await?;

        let Some(mapping) = room_mapping else {
            debug!(
                "matrix encryption ignored room_id={} reason=no_slack_mapping",
                event.room_id
            );
            return Ok(());
        };

        info!(
            "encryption enabled in room {}, leaving to prevent message sync issues",
            event.room_id
        );

        self.matrix_client
            .send_notice(
                &event.room_id,
                "You have turned on encryption in this room, so the service will not bridge any new messages.",
            )
            .await?;

        self.matrix_client.leave_room(&event.room_id).await?;

        self.db_manager
            .room_store()
            .delete_room_mapping(mapping.id)
            .await?;

        self.room_cache.remove(&event.room_id).await;

        info!("removed room mapping for encrypted room {}", event.room_id);
        Ok(())
    }

    pub async fn handle_matrix_room_name(&self, event: &MatrixEvent) -> Result<()> {
        if self
            .matrix_client
            .config()
            .bridge
            .disable_room_topic_notifications
        {
            return Ok(());
        }

        let room_mapping = self.get_room_mapping_cached(&event.room_id).await?;
        let Some(mapping) = room_mapping else {
            return Ok(());
        };

        let new_name = event
            .content
            .as_ref()
            .and_then(|c| c.get("name").and_then(|n| n.as_str()))
            .unwrap_or("");

        let sender_displayname = self
            .matrix_client
            .get_user_profile(&event.sender)
            .await
            .ok()
            .flatten()
            .map(|(name, _)| name)
            .unwrap_or_else(|| event.sender.clone());

        let message = format!(
            "**{}** changed the room name to: {}",
            sender_displayname, new_name
        );

        self.slack_client
            .send_message(&mapping.slack_channel_id, &message)
            .await?;

        debug!(
            "forwarded room name change to slack channel={}",
            mapping.slack_channel_id
        );
        Ok(())
    }

    pub async fn handle_matrix_room_topic(&self, event: &MatrixEvent) -> Result<()> {
        if self
            .matrix_client
            .config()
            .bridge
            .disable_room_topic_notifications
        {
            return Ok(());
        }

        let room_mapping = self.get_room_mapping_cached(&event.room_id).await?;
        let Some(mapping) = room_mapping else {
            debug!(
                "matrix room topic ignored room_id={} reason=no_slack_mapping",
                event.room_id
            );
            return Ok(());
        };

        let new_topic = event
            .content
            .as_ref()
            .and_then(|c| c.get("topic").and_then(|t| t.as_str()))
            .unwrap_or("");

        let sender_displayname = self
            .matrix_client
            .get_user_profile(&event.sender)
            .await
            .ok()
            .flatten()
            .map(|(name, _)| name)
            .unwrap_or_else(|| event.sender.clone());

        let message = format!(
            "**{}** changed the room topic to: {}",
            sender_displayname, new_topic
        );

        self.slack_client
            .send_message(&mapping.slack_channel_id, &message)
            .await?;

        debug!(
            "forwarded room topic change to slack channel={}",
            mapping.slack_channel_id
        );
        Ok(())
    }

    pub async fn handle_matrix_power_levels(&self, event: &MatrixEvent) -> Result<()> {
        let room_mapping = self.get_room_mapping_cached(&event.room_id).await?;

        let Some(mapping) = room_mapping else {
            debug!(
                "matrix power levels ignored room_id={} reason=no_slack_mapping",
                event.room_id
            );
            return Ok(());
        };

        let domain_suffix = format!(":{}", self.matrix_client.config().bridge.domain);
        let mut changed_users = Vec::new();
        if let Some(content) = event.content.as_ref().and_then(|c| c.as_object())
            && let Some(users) = content.get("users").and_then(|u| u.as_object())
        {
            for (mxid, level_json) in users {
                let Some(level) = level_json.as_i64() else {
                    continue;
                };
                let Some(localpart) = mxid.strip_prefix("@_slack_") else {
                    continue;
                };
                let Some(slack_user_id) = localpart.strip_suffix(&domain_suffix) else {
                    continue;
                };
                if slack_user_id.is_empty() || slack_user_id.contains(':') {
                    continue;
                }
                changed_users.push(format!("{} -> {}", slack_user_id, level));
            }
        }

        if changed_users.is_empty() {
            return Ok(());
        }

        let sender_displayname = self
            .matrix_client
            .get_user_profile(&event.sender)
            .await
            .ok()
            .flatten()
            .map(|(name, _)| name)
            .unwrap_or_else(|| event.sender.clone());

        let message = format!(
            "**{}** updated Matrix power levels for bridged users: {}",
            sender_displayname,
            changed_users.join(", ")
        );

        self.slack_client
            .send_message(&mapping.slack_channel_id, &message)
            .await?;

        debug!(
            "forwarded matrix power level change to slack channel={} users={}",
            mapping.slack_channel_id,
            changed_users.len()
        );

        Ok(())
    }

    pub async fn request_bridge_matrix_room(
        &self,
        matrix_room_id: &str,
        matrix_requestor: &str,
        guild_id: &str,
        channel_id: &str,
    ) -> Result<String> {
        if let Some(limit_message) = self.check_room_limit().await? {
            return Ok(limit_message);
        }

        if self
            .db_manager
            .room_store()
            .get_room_by_slack_channel(channel_id)
            .await?
            .is_some()
        {
            return Ok("This Slack channel is already bridged.".to_string());
        }

        let Some(channel) = self.slack_client.get_channel(channel_id).await? else {
            return Ok(
                "There was a problem bridging that channel - channel was not found.".to_string(),
            );
        };

        self.matrix_client
            .send_notice(
                matrix_room_id,
                "I'm asking permission from the guild administrators to make this bridge.",
            )
            .await?;

        match self
            .provisioning
            .ask_bridge_permission(self.slack_client.as_ref(), &channel.id, matrix_requestor)
            .await
        {
            Ok(()) => {
                self.bridge_matrix_room(matrix_room_id, guild_id, channel_id)
                    .await
            }
            Err(ProvisioningError::TimedOut) => {
                Ok("Timed out waiting for a response from the Slack owners.".to_string())
            }
            Err(ProvisioningError::Declined) => {
                Ok("The bridge has been declined by the Slack guild.".to_string())
            }
            Err(ProvisioningError::DeliveryFailed) => {
                Ok("Failed to send approval request to Slack. Ensure the bot can send messages in that channel.".to_string())
            }
            Err(err) => {
                warn!(
                    "failed to obtain bridge approval for matrix_room={} channel={}: {}",
                    matrix_room_id, channel_id, err
                );
                Ok("There was a problem bridging that channel - has the guild owner approved the bridge?".to_string())
            }
        }
    }

    pub async fn bridge_matrix_room(
        &self,
        matrix_room_id: &str,
        guild_id: &str,
        channel_id: &str,
    ) -> Result<String> {
        if let Some(limit_message) = self.check_room_limit().await? {
            return Ok(limit_message);
        }

        if self
            .db_manager
            .room_store()
            .get_room_by_slack_channel(channel_id)
            .await?
            .is_some()
        {
            return Ok("This Slack channel is already bridged.".to_string());
        }

        let Some(channel) = self.slack_client.get_channel(channel_id).await? else {
            return Ok(
                "There was a problem bridging that channel - channel was not found.".to_string(),
            );
        };

        let mapping = RoomMapping {
            id: 0,
            matrix_room_id: matrix_room_id.to_string(),
            slack_channel_id: channel.id.clone(),
            slack_channel_name: channel.name.clone(),
            slack_team_id: guild_id.to_string(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        self.db_manager
            .room_store()
            .create_room_mapping(&mapping)
            .await?;

        let name_pattern = &self.matrix_client.config().channel.name_pattern;
        let formatted_name = crate::utils::formatting::apply_pattern_string(
            name_pattern,
            &[
                ("guild", &channel.guild_id.clone()),
                ("name", &format!("#{}", mapping.slack_channel_name)),
            ],
        );

        let event_content = serde_json::json!({
            "name": formatted_name
        });
        let _ = self
            .matrix_client
            .appservice
            .client
            .send_state_event(matrix_room_id, "m.room.name", "", &event_content)
            .await;

        Ok("I have bridged this room to your channel".to_string())
    }

    async fn check_room_limit(&self) -> Result<Option<String>> {
        let room_count_limit = self.matrix_client.config().limits.room_count;
        if room_count_limit < 0 {
            return Ok(None);
        }

        let current_count = self.db_manager.room_store().count_rooms().await?;
        if current_count >= room_count_limit as i64 {
            Ok(Some(format!(
                "This bridge has reached its room limit of {}. Unbridge another room to allow for new connections.",
                room_count_limit
            )))
        } else {
            Ok(None)
        }
    }

    pub async fn unbridge_matrix_room(&self, matrix_room_id: &str) -> Result<String> {
        let room_mapping = self.get_room_mapping_cached(matrix_room_id).await?;

        let Some(mapping) = room_mapping else {
            return Ok("This room is not bridged.".to_string());
        };

        let delete_options = &self.matrix_client.config().channel.delete_options;
        let client = &self.matrix_client.appservice.client;

        if let Some(prefix) = &delete_options.name_prefix
            && let Ok(state) = client
                .get_room_state_event(matrix_room_id, "m.room.name", "")
                .await
            && let Some(name) = state.get("name").and_then(|n| n.as_str())
        {
            let new_name = format!("{}{}", prefix, name);
            let event_content = serde_json::json!({ "name": new_name });
            let _ = client
                .send_state_event(matrix_room_id, "m.room.name", "", &event_content)
                .await;
        }

        if let Some(prefix) = &delete_options.topic_prefix
            && let Ok(state) = client
                .get_room_state_event(matrix_room_id, "m.room.topic", "")
                .await
            && let Some(topic) = state.get("topic").and_then(|t| t.as_str())
        {
            let new_topic = format!("{}{}", prefix, topic);
            let event_content = serde_json::json!({ "topic": new_topic });
            let _ = client
                .send_state_event(matrix_room_id, "m.room.topic", "", &event_content)
                .await;
        }

        if delete_options.unset_room_alias {
            let alias_localpart = format!(
                "{}{}",
                self.matrix_client.config().room.room_alias_prefix,
                mapping.slack_channel_id
            );
            let alias = format!(
                "#{}:{}",
                alias_localpart,
                self.matrix_client.config().bridge.domain
            );
            let _ = client.delete_room_alias(&alias).await;
        }

        self.db_manager
            .room_store()
            .delete_room_mapping(mapping.id)
            .await?;

        self.room_cache.remove(&mapping.matrix_room_id).await;

        Ok("This room has been unbridged".to_string())
    }

    pub async fn send_to_slack_message(
        &self,
        slack_channel_id: &str,
        outbound: OutboundSlackMessage,
    ) -> Result<()> {
        let content = outbound.render_content();
        debug!(
            "sending slack message channel_id={} reply_to={:?} edit_of={:?} attachments={} content_len={} content_preview={}",
            slack_channel_id,
            outbound.reply_to,
            outbound.edit_of,
            outbound.attachments.len(),
            content.len(),
            preview_text(&content)
        );
        self.slack_client
            .send_message_with_metadata(
                slack_channel_id,
                &content,
                &outbound.attachments,
                outbound.reply_to.as_deref(),
                outbound.edit_of.as_deref(),
            )
            .await?;
        debug!(
            "slack message sent channel_id={} content_len={}",
            slack_channel_id,
            content.len()
        );
        Ok(())
    }

    pub async fn send_to_slack_message_as_user(
        &self,
        slack_channel_id: &str,
        outbound: OutboundSlackMessage,
        matrix_sender: &str,
    ) -> Result<()> {
        let content = outbound.render_content();

        let (username, avatar_url) = self
            .matrix_client
            .get_user_profile(matrix_sender)
            .await
            .unwrap_or(None)
            .unwrap_or_else(|| (matrix_sender.to_string(), None));

        let avatar_url_ref = avatar_url.as_deref();
        let avatar_for_slack = avatar_url_ref.map(|url| {
            if url.starts_with("mxc://") {
                let mxc_url = url.trim_start_matches("mxc://");
                let homeserver = &self.matrix_client.config().bridge.homeserver_url;
                format!(
                    "{}/_matrix/media/r0/download/{}",
                    homeserver.trim_end_matches('/'),
                    mxc_url
                )
            } else {
                url.to_string()
            }
        });

        debug!(
            "sending slack message via webhook channel_id={} sender={} username={} reply_to={:?} edit_of={:?} attachments={} content_len={} content_preview={}",
            slack_channel_id,
            matrix_sender,
            username,
            outbound.reply_to,
            outbound.edit_of,
            outbound.attachments.len(),
            content.len(),
            preview_text(&content)
        );

        self.slack_client
            .send_message_with_metadata_as_user(
                slack_channel_id,
                &content,
                &outbound.attachments,
                outbound.reply_to.as_deref(),
                outbound.edit_of.as_deref(),
                Some(&username),
                avatar_for_slack.as_deref(),
            )
            .await?;

        debug!(
            "slack message sent channel_id={} content_len={}",
            slack_channel_id,
            content.len()
        );
        Ok(())
    }

    pub async fn send_to_matrix_message(
        &self,
        matrix_room_id: &str,
        slack_sender: &str,
        outbound: OutboundMatrixMessage,
    ) -> Result<String> {
        let body = outbound.render_body();
        debug!(
            "sending matrix message room_id={} sender={} reply_to={:?} edit_of={:?} attachments={} body_len={} body_preview={}",
            matrix_room_id,
            slack_sender,
            outbound.reply_to,
            outbound.edit_of,
            outbound.attachments.len(),
            body.len(),
            preview_text(&body)
        );
        let event_id = self
            .matrix_client
            .send_message_with_metadata(
                matrix_room_id,
                slack_sender,
                &body,
                &outbound.attachments,
                outbound.reply_to.as_deref(),
                outbound.edit_of.as_deref(),
            )
            .await?;
        debug!(
            "matrix message sent room_id={} sender={} body_len={}",
            matrix_room_id,
            slack_sender,
            body.len()
        );
        Ok(event_id)
    }

    pub async fn send_to_matrix_with_attachments(
        &self,
        matrix_room_id: &str,
        slack_sender: &str,
        outbound: &OutboundMatrixMessage,
    ) -> Result<String> {
        let mut last_event_id: Option<String> = None;

        for attachment_url in &outbound.attachments {
            match self.media_handler.download_from_url(attachment_url).await {
                Ok(media) => {
                    if media.size > 50 * 1024 * 1024 {
                        warn!(
                            "attachment too large for Matrix: {} bytes, sending as URL instead",
                            media.size
                        );
                        let body = format!("{}: {}", media.filename, attachment_url);
                        last_event_id = Some(
                            self.matrix_client
                                .send_message_with_metadata(
                                    matrix_room_id,
                                    slack_sender,
                                    &body,
                                    &[],
                                    outbound.reply_to.as_deref(),
                                    None,
                                )
                                .await?,
                        );
                    } else {
                        let msgtype = match media.content_type.as_str() {
                            ct if ct.starts_with("image/") => "m.image",
                            ct if ct.starts_with("video/") => "m.video",
                            ct if ct.starts_with("audio/") => "m.audio",
                            _ => "m.file",
                        };

                        match self.matrix_client.upload_media(&media).await {
                            Ok(mxc_url) => {
                                let info = json!({
                                    "mimetype": media.content_type,
                                    "size": media.size,
                                });

                                last_event_id = Some(
                                    self.matrix_client
                                        .send_media_message(
                                            matrix_room_id,
                                            slack_sender,
                                            msgtype,
                                            &media.filename,
                                            &mxc_url,
                                            Some(&info),
                                            outbound.reply_to.as_deref(),
                                        )
                                        .await?,
                                );
                                info!(
                                    "uploaded slack attachment to matrix room={} file={} size={} mxc={}",
                                    matrix_room_id, media.filename, media.size, mxc_url
                                );
                            }
                            Err(e) => {
                                warn!("failed to upload attachment to matrix: {}, sending URL", e);
                                let body = format!("{}: {}", media.filename, attachment_url);
                                last_event_id = Some(
                                    self.matrix_client
                                        .send_message_with_metadata(
                                            matrix_room_id,
                                            slack_sender,
                                            &body,
                                            &[],
                                            outbound.reply_to.as_deref(),
                                            None,
                                        )
                                        .await?,
                                );
                            }
                        }
                    }
                }
                Err(e) => {
                    warn!(
                        "failed to download attachment from slack: {}, sending URL",
                        e
                    );
                    let body = format!("Attachment: {}", attachment_url);
                    last_event_id = Some(
                        self.matrix_client
                            .send_message_with_metadata(
                                matrix_room_id,
                                slack_sender,
                                &body,
                                &[],
                                outbound.reply_to.as_deref(),
                                None,
                            )
                            .await?,
                    );
                }
            }
        }

        if !outbound.body.is_empty() {
            last_event_id = Some(
                self.matrix_client
                    .send_message_with_metadata(
                        matrix_room_id,
                        slack_sender,
                        &outbound.body,
                        &[],
                        outbound.reply_to.as_deref(),
                        outbound.edit_of.as_deref(),
                    )
                    .await?,
            );
        }

        last_event_id.ok_or_else(|| anyhow::anyhow!("no message was sent"))
    }

    pub async fn handle_slack_message_with_context(
        &self,
        ctx: SlackMessageContext,
    ) -> Result<()> {
        debug!(
            "slack inbound message channel_id={} sender={} reply_to={:?} edit_of={:?} attachments={} content_len={} content_preview={}",
            ctx.channel_id,
            ctx.sender_id,
            ctx.reply_to,
            ctx.edit_of,
            ctx.attachments.len(),
            ctx.content.len(),
            preview_text(&ctx.content)
        );

        let room_mapping = self
            .db_manager
            .room_store()
            .get_room_by_slack_channel(&ctx.channel_id)
            .await?;

        debug!(
            "slack inbound mapping lookup channel_id={} mapped={}",
            ctx.channel_id,
            room_mapping.is_some()
        );

        if self.slack_command_handler.is_command(&ctx.content) {
            debug!(
                "slack inbound command detected channel_id={} sender={} command_preview={}",
                ctx.channel_id,
                ctx.sender_id,
                preview_text(&ctx.content)
            );
            let outcome = self.slack_command_handler.handle(
                &ctx.content,
                room_mapping.is_some(),
                &ctx.permissions,
            );
            self.handle_slack_command_outcome(outcome, &ctx, room_mapping.as_ref())
                .await?;
            return Ok(());
        }

        let Some(mapping) = room_mapping else {
            debug!(
                "slack inbound dropped channel_id={} reason=no_matrix_mapping",
                ctx.channel_id
            );
            return Ok(());
        };

        if let Some(slack_user) = self.slack_client.get_user(&ctx.sender_id).await? {
            let vars = [
                ("id", slack_user.id.as_str()),
                ("tag", slack_user.discriminator.as_str()),
                ("username", slack_user.username.as_str()),
            ];
            let display_name = crate::utils::formatting::apply_pattern_string(
                &self.matrix_client.config().ghosts.username_pattern,
                &vars,
            );
            self.matrix_client
                .ensure_ghost_user_registered(&ctx.sender_id, Some(&display_name))
                .await?;
        } else {
            self.matrix_client
                .ensure_ghost_user_registered(&ctx.sender_id, None)
                .await?;
        }

        let mut outbound = self.message_flow.slack_to_matrix(&SlackInboundMessage {
            channel_id: ctx.channel_id,
            sender_id: ctx.sender_id.clone(),
            content: ctx.content,
            attachments: ctx.attachments,
            reply_to: ctx.reply_to,
            edit_of: ctx.edit_of,
        });

        let reply_mapping = if let Some(reply_slack_message_id) = outbound.reply_to.clone() {
            self.db_manager
                .message_store()
                .get_by_slack_message_id(&reply_slack_message_id)
                .await?
        } else {
            None
        };

        let edit_mapping = if let Some(edit_slack_message_id) = outbound.edit_of.clone() {
            self.db_manager
                .message_store()
                .get_by_slack_message_id(&edit_slack_message_id)
                .await?
        } else {
            None
        };

        apply_message_relation_mappings(
            &mut outbound,
            reply_mapping.as_ref(),
            edit_mapping.as_ref(),
        );
        debug!(
            "slack->matrix outbound prepared channel_id={} matrix_room={} sender={} reply_to={:?} edit_of={:?} attachments={} body_len={} body_preview={}",
            mapping.slack_channel_id,
            mapping.matrix_room_id,
            ctx.sender_id,
            outbound.reply_to,
            outbound.edit_of,
            outbound.attachments.len(),
            outbound.body.len(),
            preview_text(&outbound.body)
        );

        let matrix_event_id = if !outbound.attachments.is_empty() {
            self.send_to_matrix_with_attachments(&mapping.matrix_room_id, &ctx.sender_id, &outbound)
                .await?
        } else {
            self.send_to_matrix_message(&mapping.matrix_room_id, &ctx.sender_id, outbound)
                .await?
        };

        if let Some(source_message_id) = ctx.source_message_id {
            self.db_manager
                .message_store()
                .upsert_message_mapping(&MessageMapping {
                    id: 0,
                    slack_message_id: source_message_id,
                    matrix_room_id: mapping.matrix_room_id.clone(),
                    matrix_event_id,
                    created_at: Utc::now(),
                    updated_at: Utc::now(),
                })
                .await?;
        }
        Ok(())
    }

    pub async fn handle_slack_message_delete(
        &self,
        _slack_channel_id: &str,
        slack_message_id: &str,
    ) -> Result<()> {
        let link = self
            .db_manager
            .message_store()
            .get_by_slack_message_id(slack_message_id)
            .await?;

        let Some(request) = slack_delete_redaction_request(link.as_ref()) else {
            return Ok(());
        };

        self.matrix_client
            .redact_message(&request.room_id, &request.event_id, Some(request.reason))
            .await?;
        self.db_manager
            .message_store()
            .delete_by_slack_message_id(slack_message_id)
            .await?;
        Ok(())
    }

    pub async fn handle_slack_typing(
        &self,
        slack_channel_id: &str,
        slack_sender_id: &str,
    ) -> Result<()> {
        let disable_typing_notifications = self
            .matrix_client
            .config()
            .bridge
            .disable_typing_notifications;

        let room_mapping = self
            .db_manager
            .room_store()
            .get_room_by_slack_channel(slack_channel_id)
            .await?;

        if !should_forward_slack_typing(disable_typing_notifications, room_mapping.as_ref()) {
            return Ok(());
        }

        let Some(mapping) = room_mapping else {
            return Ok(());
        };

        self.matrix_client
            .ensure_ghost_user_registered(slack_sender_id, None)
            .await?;

        let request = build_slack_typing_request(&mapping.matrix_room_id, slack_sender_id);

        self.matrix_client
            .set_slack_user_typing(
                &request.room_id,
                &request.slack_user_id,
                request.typing,
                request.timeout_ms,
            )
            .await?;

        debug!(
            "slack typing forwarded channel_id={} sender={} mapped_room={}",
            slack_channel_id, slack_sender_id, mapping.matrix_room_id
        );

        Ok(())
    }

    pub async fn handle_slack_reaction_added(
        &self,
        slack_channel_id: &str,
        slack_message_id: &str,
        slack_user_id: &str,
        reaction: &str,
    ) -> Result<()> {
        let Some(mapping) = self
            .db_manager
            .room_store()
            .get_room_by_slack_channel(slack_channel_id)
            .await?
        else {
            debug!("no room mapping for reaction channel={}", slack_channel_id);
            return Ok(());
        };

        let Some(message_mapping) = self
            .db_manager
            .message_store()
            .get_by_slack_message_id(slack_message_id)
            .await?
        else {
            debug!("no message mapping for reaction ts={}", slack_message_id);
            return Ok(());
        };

        self.matrix_client
            .ensure_ghost_user_registered(slack_user_id, None)
            .await?;

        let emoji = if reaction.starts_with(':') && reaction.ends_with(':') {
            reaction[1..reaction.len()-1].to_string()
        } else {
            reaction.to_string()
        };

        self.matrix_client
            .send_reaction_as_ghost(
                &mapping.matrix_room_id,
                &message_mapping.matrix_event_id,
                slack_user_id,
                &emoji,
            )
            .await?;

        debug!(
            "slack reaction added forwarded channel={} ts={} user={} reaction={}",
            slack_channel_id, slack_message_id, slack_user_id, reaction
        );

        Ok(())
    }

    pub async fn handle_slack_reaction_removed(
        &self,
        slack_channel_id: &str,
        slack_message_id: &str,
        slack_user_id: &str,
        reaction: &str,
    ) -> Result<()> {
        let Some(mapping) = self
            .db_manager
            .room_store()
            .get_room_by_slack_channel(slack_channel_id)
            .await?
        else {
            return Ok(());
        };

        let Some(message_mapping) = self
            .db_manager
            .message_store()
            .get_by_slack_message_id(slack_message_id)
            .await?
        else {
            return Ok(());
        };

        let emoji = if reaction.starts_with(':') && reaction.ends_with(':') {
            reaction[1..reaction.len()-1].to_string()
        } else {
            reaction.to_string()
        };

        self.matrix_client
            .redact_reaction_as_ghost(
                &mapping.matrix_room_id,
                &message_mapping.matrix_event_id,
                slack_user_id,
                &emoji,
            )
            .await?;

        debug!(
            "slack reaction removed forwarded channel={} ts={} user={} reaction={}",
            slack_channel_id, slack_message_id, slack_user_id, reaction
        );

        Ok(())
    }

    pub async fn handle_slack_member_joined_channel(
        &self,
        slack_channel_id: &str,
        slack_user_id: &str,
    ) -> Result<()> {
        let Some(mapping) = self
            .db_manager
            .room_store()
            .get_room_by_slack_channel(slack_channel_id)
            .await?
        else {
            return Ok(());
        };

        if self.matrix_client.config().bridge.disable_join_leave_notifications {
            return Ok(());
        }

        self.matrix_client
            .ensure_ghost_user_registered(slack_user_id, None)
            .await?;

        self.matrix_client
            .invite_ghost_to_room(slack_user_id, &mapping.matrix_room_id)
            .await?;

        debug!(
            "slack member joined channel forwarded channel={} user={}",
            slack_channel_id, slack_user_id
        );

        Ok(())
    }

    pub async fn handle_slack_member_left_channel(
        &self,
        slack_channel_id: &str,
        slack_user_id: &str,
    ) -> Result<()> {
        let Some(mapping) = self
            .db_manager
            .room_store()
            .get_room_by_slack_channel(slack_channel_id)
            .await?
        else {
            return Ok(());
        };

        if self.matrix_client.config().bridge.disable_join_leave_notifications {
            return Ok(());
        }

        self.matrix_client
            .kick_ghost_from_room(slack_user_id, &mapping.matrix_room_id)
            .await?;

        debug!(
            "slack member left channel forwarded channel={} user={}",
            slack_channel_id, slack_user_id
        );

        Ok(())
    }

    pub async fn handle_slack_channel_marked(
        &self,
        slack_channel_id: &str,
        slack_user_id: &str,
        _ts: &str,
    ) -> Result<()> {
        let Some(_mapping) = self
            .db_manager
            .room_store()
            .get_room_by_slack_channel(slack_channel_id)
            .await?
        else {
            return Ok(());
        };

        if self.matrix_client.config().bridge.disable_read_receipts {
            return Ok(());
        }

        self.matrix_client
            .ensure_ghost_user_registered(slack_user_id, None)
            .await?;

        debug!(
            "slack channel marked (read receipt) channel={} user={}",
            slack_channel_id, slack_user_id
        );

        Ok(())
    }

    async fn handle_slack_command_outcome(
        &self,
        outcome: SlackCommandOutcome,
        ctx: &SlackMessageContext,
        room_mapping: Option<&RoomMapping>,
    ) -> Result<()> {
        match outcome {
            SlackCommandOutcome::Ignored => {}
            SlackCommandOutcome::Reply(reply) => {
                self.slack_client
                    .send_message(&ctx.channel_id, &reply)
                    .await?;
            }
            SlackCommandOutcome::ApproveRequested => {
                let reply = match self.provisioning.mark_approval(&ctx.channel_id, true) {
                    ApprovalResponseStatus::Applied => {
                        "Thanks for your response! The matrix bridge has been approved."
                    }
                    ApprovalResponseStatus::Expired => {
                        "Thanks for your response, however it has arrived after the deadline - sorry!"
                    }
                };
                self.slack_client
                    .send_message(&ctx.channel_id, reply)
                    .await?;
            }
            SlackCommandOutcome::DenyRequested => {
                let reply = match self.provisioning.mark_approval(&ctx.channel_id, false) {
                    ApprovalResponseStatus::Applied => {
                        "Thanks for your response! The matrix bridge has been declined."
                    }
                    ApprovalResponseStatus::Expired => {
                        "Thanks for your response, however it has arrived after the deadline - sorry!"
                    }
                };
                self.slack_client
                    .send_message(&ctx.channel_id, reply)
                    .await?;
            }
            SlackCommandOutcome::ModerationRequested {
                action,
                matrix_user,
            } => {
                let action_word = match action {
                    ModerationAction::Kick => "Kicked",
                    ModerationAction::Ban => "Banned",
                    ModerationAction::Unban => "Unbanned",
                };

                let Some(mapping) = room_mapping else {
                    self.slack_client
                        .send_message(
                            &ctx.channel_id,
                            "This channel is not bridged to a plumbed matrix room",
                        )
                        .await?;
                    return Ok(());
                };

                let guild_rooms = self
                    .db_manager
                    .room_store()
                    .get_rooms_by_guild(&mapping.slack_team_id)
                    .await?;

                let target_rooms: Vec<String> = if guild_rooms.is_empty() {
                    vec![mapping.matrix_room_id.clone()]
                } else {
                    let mut seen = std::collections::HashSet::new();
                    guild_rooms
                        .into_iter()
                        .filter_map(|room| {
                            if seen.insert(room.matrix_room_id.clone()) {
                                Some(room.matrix_room_id)
                            } else {
                                None
                            }
                        })
                        .collect()
                };

                let mut success_count = 0usize;
                let mut failed_count = 0usize;

                for room_id in &target_rooms {
                    let reason = format!(
                        "Slack moderation request by {} from channel {}",
                        ctx.sender_id, ctx.channel_id
                    );
                    let result = match action {
                        ModerationAction::Kick => {
                            self.matrix_client
                                .kick_user_from_room(room_id, &matrix_user, Some(&reason))
                                .await
                        }
                        ModerationAction::Ban => {
                            self.matrix_client
                                .ban_user_from_room(room_id, &matrix_user, Some(&reason))
                                .await
                        }
                        ModerationAction::Unban => {
                            self.matrix_client
                                .unban_user_from_room(room_id, &matrix_user)
                                .await
                        }
                    };

                    match result {
                        Ok(()) => {
                            success_count += 1;
                            let notice = format!(
                                "Slack moderation request: {} {} (requested by {})",
                                action_keyword(&action),
                                matrix_user,
                                ctx.sender_id
                            );
                            if let Err(err) = self.matrix_client.send_notice(room_id, &notice).await
                            {
                                warn!(
                                    "failed to send moderation notice to room {}: {}",
                                    room_id, err
                                );
                            }
                        }
                        Err(err) => {
                            failed_count += 1;
                            warn!(
                                "failed to apply moderation action={} user={} room={}: {}",
                                action_keyword(&action),
                                matrix_user,
                                room_id,
                                err
                            );
                        }
                    }
                }

                let reply = if failed_count == 0 {
                    format!("{action_word} {matrix_user} in {success_count} bridged room(s).")
                } else {
                    format!(
                        "{action_word} {matrix_user} in {success_count} room(s), failed in {failed_count} room(s)."
                    )
                };
                self.slack_client
                    .send_message(&ctx.channel_id, &reply)
                    .await?;
            }
            SlackCommandOutcome::UnbridgeRequested => {
                if let Some(mapping) = room_mapping {
                    let matrix_room_id = mapping.matrix_room_id.clone();
                    self.db_manager
                        .room_store()
                        .delete_room_mapping(mapping.id)
                        .await?;
                    self.room_cache.remove(&matrix_room_id).await;
                    self.slack_client
                        .send_message(&ctx.channel_id, "This channel has been unbridged")
                        .await?;
                } else {
                    self.slack_client
                        .send_message(
                            &ctx.channel_id,
                            "This channel is not bridged to a plumbed matrix room",
                        )
                        .await?;
                }
            }
            SlackCommandOutcome::BridgeRequested {
                guild_id,
                channel_id,
            } => {
                let reply = self
                    .request_bridge_slack_channel(
                        &ctx.channel_id,
                        &ctx.sender_id,
                        &guild_id,
                        &channel_id,
                    )
                    .await?;
                self.slack_client
                    .send_message(&ctx.channel_id, &reply)
                    .await?;
            }
        }
        Ok(())
    }

    async fn request_bridge_slack_channel(
        &self,
        _slack_channel_id: &str,
        _requestor_id: &str,
        guild_id: &str,
        channel_id: &str,
    ) -> Result<String> {
        if self
            .db_manager
            .room_store()
            .get_room_by_slack_channel(channel_id)
            .await?
            .is_some()
        {
            return Ok("That Slack channel is already bridged.".to_string());
        }

        let Some(channel) = self.slack_client.get_channel(channel_id).await? else {
            return Ok("Could not find the specified Slack channel.".to_string());
        };

        let matrix_room_id = match self
            .matrix_client
            .create_room(
                &channel.id,
                &format!("[Slack] #{}", channel.name),
                channel.topic.as_deref(),
            )
            .await
        {
            Ok(room_id) => room_id,
            Err(e) => {
                warn!("failed to create matrix room for bridge: {}", e);
                return Ok("Failed to create Matrix room for the bridge.".to_string());
            }
        };

        let mapping = RoomMapping {
            id: 0,
            matrix_room_id: matrix_room_id.clone(),
            slack_channel_id: channel.id.clone(),
            slack_channel_name: channel.name.clone(),
            slack_team_id: guild_id.to_string(),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };

        self.db_manager
            .room_store()
            .create_room_mapping(&mapping)
            .await?;

        info!(
            "created bridge from slack channel {} to matrix room {}",
            channel.id, matrix_room_id
        );
        Ok(format!(
            "Successfully bridged to Matrix room: {}",
            matrix_room_id
        ))
    }

    pub async fn handle_slack_message(
        &self,
        slack_channel_id: &str,
        slack_sender: &str,
        content: &str,
    ) -> Result<()> {
        self.handle_slack_message_with_context(SlackMessageContext {
            channel_id: slack_channel_id.to_string(),
            source_message_id: None,
            sender_id: slack_sender.to_string(),
            content: content.to_string(),
            attachments: Vec::new(),
            reply_to: None,
            edit_of: None,
            permissions: HashSet::new(),
        })
        .await
    }

    pub fn enqueue_slack_presence(&self, presence: SlackPresence) {
        self.presence_handler.enqueue_user(presence);
    }

    pub async fn handle_slack_channel_update(
        &self,
        slack_channel_id: &str,
        new_name: &str,
        new_topic: Option<&str>,
    ) -> Result<()> {
        let room_mapping = self
            .db_manager
            .room_store()
            .get_room_by_slack_channel(slack_channel_id)
            .await?;

        let Some(mapping) = room_mapping else {
            debug!(
                "ignoring channel update for unmapped channel {}",
                slack_channel_id
            );
            return Ok(());
        };

        let name_pattern = &self.matrix_client.config().channel.name_pattern;
        let formatted_name = crate::utils::formatting::apply_pattern_string(
            name_pattern,
            &[
                ("guild", &mapping.slack_team_id),
                ("name", &format!("#{}", new_name)),
            ],
        );

        let current_name = self
            .matrix_client
            .get_room_name(&mapping.matrix_room_id)
            .await?;
        if current_name.as_deref() != Some(&formatted_name) {
            self.matrix_client
                .set_room_name(&mapping.matrix_room_id, &formatted_name)
                .await?;

            let mut updated = mapping.clone();
            updated.slack_channel_name = new_name.to_string();
            updated.updated_at = chrono::Utc::now();
            self.db_manager
                .room_store()
                .update_room_mapping(&updated)
                .await?;

            info!(
                "updated room name for channel {} to {}",
                slack_channel_id, formatted_name
            );
        }

        if let Some(topic) = new_topic {
            let current_topic = self
                .matrix_client
                .get_room_topic(&mapping.matrix_room_id)
                .await?;
            if current_topic.as_deref() != Some(topic) {
                self.matrix_client
                    .set_room_topic(&mapping.matrix_room_id, topic)
                    .await?;
                info!("updated room topic for channel {}", slack_channel_id);
            }
        }

        Ok(())
    }

    pub async fn handle_slack_channel_delete(&self, slack_channel_id: &str) -> Result<()> {
        let room_mapping = self
            .db_manager
            .room_store()
            .get_room_by_slack_channel(slack_channel_id)
            .await?;

        let Some(mapping) = room_mapping else {
            debug!(
                "ignoring channel delete for unmapped channel {}",
                slack_channel_id
            );
            return Ok(());
        };

        let delete_options = &self.matrix_client.config().channel.delete_options;
        let client = &self.matrix_client.appservice.client;

        if let Some(prefix) = &delete_options.name_prefix
            && let Ok(state) = client
                .get_room_state_event(&mapping.matrix_room_id, "m.room.name", "")
                .await
            && let Some(name) = state.get("name").and_then(|n| n.as_str())
        {
            let new_name = format!("{}{}", prefix, name);
            let event_content = serde_json::json!({ "name": new_name });
            let _ = client
                .send_state_event(&mapping.matrix_room_id, "m.room.name", "", &event_content)
                .await;
        }

        if let Some(prefix) = &delete_options.topic_prefix
            && let Ok(state) = client
                .get_room_state_event(&mapping.matrix_room_id, "m.room.topic", "")
                .await
            && let Some(topic) = state.get("topic").and_then(|t| t.as_str())
        {
            let new_topic = format!("{}{}", prefix, topic);
            let event_content = serde_json::json!({ "topic": new_topic });
            let _ = client
                .send_state_event(&mapping.matrix_room_id, "m.room.topic", "", &event_content)
                .await;
        }

        self.db_manager
            .room_store()
            .delete_room_mapping(mapping.id)
            .await?;

        self.room_cache.remove(&mapping.matrix_room_id).await;

        info!(
            "removed room mapping for deleted channel {}",
            slack_channel_id
        );
        Ok(())
    }

    pub async fn handle_slack_guild_update(
        &self,
        _slack_guild_id: &str,
        _new_name: &str,
        _new_icon_url: Option<&str>,
    ) -> Result<()> {
        // Future: Update all bridged rooms with new guild info
        // For now, we just log the event
        debug!(
            "guild update event received, guild_id={}",
            _slack_guild_id
        );
        Ok(())
    }

    pub async fn handle_slack_user_update(
        &self,
        slack_user_id: &str,
        new_username: &str,
        new_avatar_url: Option<&str>,
    ) -> Result<()> {
        let user_mapping = self
            .db_manager
            .user_store()
            .get_user_by_slack_id(slack_user_id)
            .await?;

        let Some(mapping) = user_mapping else {
            debug!("ignoring user update for unmapped user {}", slack_user_id);
            return Ok(());
        };

        let mut updated = mapping.clone();
        updated.slack_username = new_username.to_string();
        updated.slack_avatar = new_avatar_url.map(ToOwned::to_owned);
        updated.updated_at = chrono::Utc::now();
        self.db_manager
            .user_store()
            .update_user_mapping(&updated)
            .await?;

        info!(
            "updated user mapping for {} with new username {}",
            slack_user_id, new_username
        );
        Ok(())
    }

    pub async fn handle_slack_guild_member_update(
        &self,
        slack_team_id: &str,
        slack_user_id: &str,
        new_nick: &str,
        new_avatar_url: Option<&str>,
        roles: &[String],
    ) -> Result<()> {
        let user_mapping = self
            .db_manager
            .user_store()
            .get_user_by_slack_id(slack_user_id)
            .await?;

        let Some(mapping) = user_mapping else {
            debug!(
                "ignoring guild member update for unmapped user {}",
                slack_user_id
            );
            return Ok(());
        };

        let mut updated = mapping.clone();
        if new_avatar_url.is_some() {
            updated.slack_avatar = new_avatar_url.map(ToOwned::to_owned);
        }
        updated.updated_at = chrono::Utc::now();
        self.db_manager
            .user_store()
            .update_user_mapping(&updated)
            .await?;

        let room_mappings = self
            .db_manager
            .room_store()
            .get_rooms_by_guild(slack_team_id)
            .await?;
        for room in room_mappings {
            if let Err(err) = self
                .matrix_client
                .set_ghost_room_roles(slack_user_id, &room.matrix_room_id, roles)
                .await
            {
                warn!(
                    "failed to sync member roles for user={} guild={} room={}: {}",
                    slack_user_id, slack_team_id, room.matrix_room_id, err
                );
            }
        }

        info!(
            "updated user mapping for {} in guild {} with new nick {}",
            slack_user_id, slack_team_id, new_nick
        );
        Ok(())
    }

    pub async fn handle_slack_guild_delete(&self, slack_team_id: &str) -> Result<()> {
        let room_mappings = self
            .db_manager
            .room_store()
            .list_room_mappings(i64::MAX, 0)
            .await?;

        let affected_rooms: Vec<_> = room_mappings
            .iter()
            .filter(|m| m.slack_team_id == slack_team_id)
            .collect();

        for mapping in &affected_rooms {
            if let Err(err) = self
                .handle_slack_channel_delete(&mapping.slack_channel_id)
                .await
            {
                warn!(
                    "failed to clean up room mapping for guild {}: {}",
                    slack_team_id, err
                );
            }
        }

        info!(
            "cleaned up {} room mappings for deleted guild {}",
            affected_rooms.len(),
            slack_team_id
        );
        Ok(())
    }

    pub async fn handle_slack_guild_member_add(
        &self,
        slack_team_id: &str,
        slack_user_id: &str,
        display_name: &str,
        _avatar_url: Option<&str>,
        roles: &[String],
    ) -> Result<()> {
        debug!(
            "slack guild member add guild_id={} user_id={} display_name={}",
            slack_team_id, slack_user_id, display_name
        );

        let room_mappings = self
            .db_manager
            .room_store()
            .list_room_mappings(i64::MAX, 0)
            .await?;

        let guild_rooms: Vec<_> = room_mappings
            .iter()
            .filter(|m| m.slack_team_id == slack_team_id)
            .collect();

        if guild_rooms.is_empty() {
            debug!(
                "no rooms mapped for guild {}, skipping member add",
                slack_team_id
            );
            return Ok(());
        }

        self.matrix_client
            .ensure_ghost_user_registered(slack_user_id, Some(display_name))
            .await?;

        for mapping in guild_rooms {
            if !self
                .matrix_client
                .config()
                .bridge
                .disable_join_leave_notifications
            {
                let ghost_user_id = format!(
                    "@_slack_{}:{}",
                    slack_user_id,
                    self.matrix_client.config().bridge.domain
                );

                match self
                    .matrix_client
                    .invite_user_to_room(&mapping.matrix_room_id, &ghost_user_id)
                    .await
                {
                    Ok(_) => {
                        info!(
                            "invited ghost user {} to room {} for guild member add",
                            ghost_user_id, mapping.matrix_room_id
                        );
                    }
                    Err(e) => {
                        warn!(
                            "failed to invite ghost user {} to room {}: {}",
                            ghost_user_id, mapping.matrix_room_id, e
                        );
                    }
                }
            }

            if let Err(err) = self
                .matrix_client
                .set_ghost_room_roles(slack_user_id, &mapping.matrix_room_id, roles)
                .await
            {
                warn!(
                    "failed to sync member roles for user={} guild={} room={}: {}",
                    slack_user_id, slack_team_id, mapping.matrix_room_id, err
                );
            }
        }

        Ok(())
    }

    pub async fn handle_slack_guild_member_remove(
        &self,
        slack_team_id: &str,
        slack_user_id: &str,
    ) -> Result<()> {
        debug!(
            "slack guild member remove guild_id={} user_id={}",
            slack_team_id, slack_user_id
        );

        let room_mappings = self
            .db_manager
            .room_store()
            .list_room_mappings(i64::MAX, 0)
            .await?;

        let guild_rooms: Vec<_> = room_mappings
            .iter()
            .filter(|m| m.slack_team_id == slack_team_id)
            .collect();

        if guild_rooms.is_empty() {
            debug!(
                "no rooms mapped for guild {}, skipping member remove",
                slack_team_id
            );
            return Ok(());
        }

        for mapping in guild_rooms {
            if !self
                .matrix_client
                .config()
                .bridge
                .disable_join_leave_notifications
            {
                let ghost_user_id = format!(
                    "@_slack_{}:{}",
                    slack_user_id,
                    self.matrix_client.config().bridge.domain
                );

                match self
                    .matrix_client
                    .kick_user_from_room(
                        &mapping.matrix_room_id,
                        &ghost_user_id,
                        Some("Left Slack server"),
                    )
                    .await
                {
                    Ok(_) => {
                        info!(
                            "kicked ghost user {} from room {} for guild member remove",
                            ghost_user_id, mapping.matrix_room_id
                        );
                    }
                    Err(e) => {
                        warn!(
                            "failed to kick ghost user {} from room {}: {}",
                            ghost_user_id, mapping.matrix_room_id, e
                        );
                    }
                }
            }
        }

        Ok(())
    }

    pub fn db(&self) -> Arc<DatabaseManager> {
        self.db_manager.clone()
    }

    pub async fn slack_client(&self) -> Arc<SlackClient> {
        self.slack_client.clone()
    }

    pub fn blocker(&self) -> Arc<blocker::BridgeBlocker> {
        Arc::new(blocker::BridgeBlocker::new(
            self.db_manager.clone(),
            self.matrix_client.config(),
        ))
    }
}

#[async_trait]
impl MatrixPresenceTarget for MatrixAppservice {
    async fn set_presence(
        &self,
        slack_user_id: &str,
        presence: MatrixPresenceState,
        status_message: &str,
    ) -> Result<()> {
        let presence = match presence {
            MatrixPresenceState::Online => "online",
            MatrixPresenceState::Offline => "offline",
            MatrixPresenceState::Unavailable => "unavailable",
        };
        self.set_slack_user_presence(slack_user_id, presence, status_message)
            .await
    }

    async fn ensure_user_registered(
        &self,
        slack_user_id: &str,
        username: Option<&str>,
    ) -> Result<()> {
        self.ensure_ghost_user_registered(slack_user_id, username)
            .await
    }
}

