use anyhow::Result;
use serde_json::Value;
use tracing::{debug, info, warn};

use crate::bridge::BridgeCore;
use crate::db::RoomMapping;

/// Configuration for backfill operations
#[derive(Debug, Clone)]
pub struct BackfillConfig {
    /// Whether backfill is enabled
    pub enabled: bool,
    /// Maximum number of messages to backfill per channel
    pub max_messages: u32,
    /// Number of conversations to backfill on startup
    pub conversation_count: u32,
}

impl Default for BackfillConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            max_messages: 100,
            conversation_count: 0,
        }
    }
}

impl BridgeCore {
    /// Backfill message history for a specific channel
    pub async fn backfill_channel(
        &self,
        mapping: &RoomMapping,
        max_messages: u32,
    ) -> Result<u32> {
        if max_messages == 0 {
            return Ok(0);
        }

        info!(
            "starting backfill for channel {} -> room {}",
            mapping.slack_channel_id, mapping.matrix_room_id
        );

        let mut total_sent = 0u32;
        let mut cursor: Option<String> = None;
        let remaining = max_messages;

        loop {
            let batch_size = remaining.min(100);
            let history = self
                .slack_client
                .get_conversation_history(
                    &mapping.slack_channel_id,
                    Some(batch_size),
                    cursor.as_deref(),
                )
                .await?;

            let messages = history
                .get("messages")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();

            if messages.is_empty() {
                break;
            }

            // Messages come in reverse chronological order, reverse them
            let mut messages = messages;
            messages.reverse();

            for msg in &messages {
                if total_sent >= max_messages {
                    break;
                }

                let sender_id = msg.get("user").and_then(Value::as_str);
                let text = msg.get("text").and_then(Value::as_str).unwrap_or_default();
                let ts = msg.get("ts").and_then(Value::as_str);
                let thread_ts = msg.get("thread_ts").and_then(Value::as_str);
                let subtype = msg.get("subtype").and_then(Value::as_str);

                // Skip subtypes that aren't real messages
                match subtype {
                    Some("channel_join") | Some("channel_leave") | Some("channel_topic")
                    | Some("channel_purpose") | Some("channel_name") => continue,
                    _ => {}
                }

                let Some(sender_id) = sender_id else {
                    continue;
                };

                if text.is_empty() && msg.get("files").is_none() {
                    continue;
                }

                // Check if we already have this message bridged
                if let Some(ts) = ts {
                    if self
                        .db_manager
                        .message_store()
                        .get_by_slack_message_id(ts)
                        .await?
                        .is_some()
                    {
                        continue;
                    }
                }

                // Extract file attachments
                let attachments = extract_backfill_attachments(msg);

                let reply_to = thread_ts
                    .filter(|thread| ts.is_some_and(|t| *thread != t))
                    .map(ToOwned::to_owned);

                self.matrix_client
                    .ensure_ghost_user_registered(sender_id, None)
                    .await?;

                let outbound = crate::bridge::message_flow::OutboundMatrixMessage {
                    body: crate::parsers::common::MessageUtils::normalize_slack_text_basic(text),
                    formatted_body: None,
                    reply_to,
                    edit_of: None,
                    attachments,
                };

                match if !outbound.attachments.is_empty() {
                    self.send_to_matrix_with_attachments(
                        &mapping.matrix_room_id,
                        sender_id,
                        &outbound,
                    )
                    .await
                } else {
                    self.send_to_matrix_message(
                        &mapping.matrix_room_id,
                        sender_id,
                        outbound,
                    )
                    .await
                } {
                    Ok(event_id) => {
                        if let Some(ts) = ts {
                            let _ = self
                                .db_manager
                                .message_store()
                                .upsert_message_mapping(
                                    &crate::db::MessageMapping {
                                        id: 0,
                                        slack_message_id: ts.to_string(),
                                        matrix_room_id: mapping.matrix_room_id.clone(),
                                        matrix_event_id: event_id,
                                        created_at: chrono::Utc::now(),
                                        updated_at: chrono::Utc::now(),
                                    },
                                )
                                .await;
                        }
                        total_sent += 1;
                    }
                    Err(err) => {
                        warn!("failed to send backfill message to matrix: {}", err);
                    }
                }
            }

            if total_sent >= max_messages {
                break;
            }

            // Check for pagination
            let has_more = history
                .get("has_more")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            if !has_more {
                break;
            }

            cursor = history
                .get("response_metadata")
                .and_then(|m| m.get("next_cursor"))
                .and_then(Value::as_str)
                .filter(|c| !c.is_empty())
                .map(ToOwned::to_owned);

            if cursor.is_none() {
                break;
            }
        }

        info!(
            "backfill complete for channel {} -> room {}: {} messages",
            mapping.slack_channel_id, mapping.matrix_room_id, total_sent
        );

        Ok(total_sent)
    }

    /// Backfill thread replies for a specific thread
    pub async fn backfill_thread(
        &self,
        mapping: &RoomMapping,
        thread_ts: &str,
        max_messages: u32,
    ) -> Result<u32> {
        info!(
            "starting thread backfill for channel {} thread {} -> room {}",
            mapping.slack_channel_id, thread_ts, mapping.matrix_room_id
        );

        let replies = self
            .slack_client
            .get_conversation_replies(
                &mapping.slack_channel_id,
                thread_ts,
                Some(max_messages),
                None,
            )
            .await?;

        let messages = replies
            .get("messages")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();

        let mut total_sent = 0u32;

        for msg in &messages {
            let sender_id = msg.get("user").and_then(Value::as_str);
            let text = msg.get("text").and_then(Value::as_str).unwrap_or_default();
            let ts = msg.get("ts").and_then(Value::as_str);

            let Some(sender_id) = sender_id else {
                continue;
            };

            if text.is_empty() {
                continue;
            }

            // Skip if already bridged
            if let Some(ts) = ts {
                if self
                    .db_manager
                    .message_store()
                    .get_by_slack_message_id(ts)
                    .await?
                    .is_some()
                {
                    continue;
                }
            }

            let reply_to = if ts != Some(thread_ts) {
                Some(thread_ts.to_string())
            } else {
                None
            };

            self.matrix_client
                .ensure_ghost_user_registered(sender_id, None)
                .await?;

            let outbound = crate::bridge::message_flow::OutboundMatrixMessage {
                body: crate::parsers::common::MessageUtils::normalize_slack_text_basic(text),
                formatted_body: None,
                reply_to,
                edit_of: None,
                attachments: extract_backfill_attachments(msg),
            };

            match self
                .send_to_matrix_message(&mapping.matrix_room_id, sender_id, outbound)
                .await
            {
                Ok(event_id) => {
                    if let Some(ts) = ts {
                        let _ = self
                            .db_manager
                            .message_store()
                            .upsert_message_mapping(
                                &crate::db::MessageMapping {
                                    id: 0,
                                    slack_message_id: ts.to_string(),
                                    matrix_room_id: mapping.matrix_room_id.clone(),
                                    matrix_event_id: event_id,
                                    created_at: chrono::Utc::now(),
                                    updated_at: chrono::Utc::now(),
                                },
                            )
                            .await;
                    }
                    total_sent += 1;
                }
                Err(err) => {
                    warn!("failed to send backfill thread reply to matrix: {}", err);
                }
            }
        }

        info!(
            "thread backfill complete: {} messages sent",
            total_sent
        );

        Ok(total_sent)
    }
}

fn extract_backfill_attachments(message: &Value) -> Vec<String> {
    let mut output = Vec::new();
    if let Some(files) = message.get("files").and_then(Value::as_array) {
        for file in files {
            let link = file
                .get("permalink_public")
                .and_then(Value::as_str)
                .or_else(|| file.get("permalink").and_then(Value::as_str))
                .or_else(|| file.get("url_private_download").and_then(Value::as_str))
                .or_else(|| file.get("url_private").and_then(Value::as_str));
            if let Some(link) = link {
                output.push(link.to_string());
            }
        }
    }
    output
}
