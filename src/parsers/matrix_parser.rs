use std::sync::Arc;

use anyhow::Result;
use chrono::Utc;
use regex::Regex;
use serde_json::Value;

use super::common::{BridgeMessage, MessageUtils, ParsedMessage};
use crate::matrix::{MatrixAppservice, MatrixEvent};

pub struct MatrixMessageParser {
    _client: Arc<MatrixAppservice>,
}

impl MatrixMessageParser {
    pub fn new(client: Arc<MatrixAppservice>) -> Self {
        Self { _client: client }
    }

    pub fn parse_message(&self, content: &str) -> ParsedMessage {
        ParsedMessage::new(content)
    }
}

pub struct MatrixToSlackConverter {
    matrix_client: Arc<MatrixAppservice>,
    ghost_user_regex: Regex,
    ghost_alias_regex: Regex,
    room_alias_regex: Regex,
    mxclink_regex: Regex,
}

impl MatrixToSlackConverter {
    pub fn new(matrix_client: Arc<MatrixAppservice>) -> Self {
        Self {
            matrix_client,
            ghost_user_regex: Regex::new(r"@_slack_(\d+):[A-Za-z0-9.-]+").unwrap(),
            ghost_alias_regex: Regex::new(r"#_slack_(\d+):[A-Za-z0-9.-]+").unwrap(),
            room_alias_regex: Regex::new(r"#([^:]+):([a-zA-Z0-9.-]+)").unwrap(),
            mxclink_regex: Regex::new(r"\[([^\]]+)\]\(mxc://[^)]+\)").unwrap(),
        }
    }

    pub fn format_for_slack(&self, message: &str) -> String {
        let mut result = message.to_string();
        result = self.convert_ghost_users_to_slack(&result);
        result = self.convert_ghost_aliases_to_slack(&result);
        result = self.convert_mxclinks_to_slack(&result);
        result
    }

    pub fn format_html_for_slack(&self, html: &str) -> String {
        let mut result = MessageUtils::convert_html_to_slack_markdown(html);
        result = self.format_for_slack(&result);
        result
    }

    fn convert_ghost_users_to_slack(&self, text: &str) -> String {
        self.ghost_user_regex
            .replace_all(text, |caps: &regex::Captures| {
                let user_id = &caps[1];
                format!("<@{}>", user_id)
            })
            .to_string()
    }

    fn convert_ghost_aliases_to_slack(&self, text: &str) -> String {
        self.ghost_alias_regex
            .replace_all(text, |caps: &regex::Captures| {
                let channel_id = &caps[1];
                format!("<#{}>", channel_id)
            })
            .to_string()
    }

    fn convert_mxclinks_to_slack(&self, text: &str) -> String {
        self.mxclink_regex
            .replace_all(text, |caps: &regex::Captures| {
                let alt_text = &caps[1];
                alt_text.to_string()
            })
            .to_string()
    }

    pub async fn convert_message(
        &self,
        matrix_event: &MatrixEvent,
        slack_channel_id: &str,
    ) -> Result<BridgeMessage> {
        let content = matrix_event.content.as_ref();

        let formatted_body = content
            .and_then(|c| c.get("formatted_body"))
            .and_then(Value::as_str);

        let plain = content
            .map(MessageUtils::extract_plain_text)
            .unwrap_or_default();

        let (final_content, formatted_content) = if let Some(html) = formatted_body {
            let converted = self.format_html_for_slack(html);
            (converted.clone(), Some(converted))
        } else {
            let converted = self.format_for_slack(&plain);
            (converted.clone(), None)
        };

        let attachments = self.extract_matrix_attachments(content);

        Ok(BridgeMessage {
            source_platform: "matrix".to_string(),
            target_platform: "slack".to_string(),
            source_id: format!("{}:{}", matrix_event.room_id, matrix_event.sender),
            target_id: slack_channel_id.to_string(),
            content: final_content,
            formatted_content,
            timestamp: matrix_event
                .timestamp
                .clone()
                .unwrap_or_else(|| Utc::now().to_rfc3339()),
            attachments,
        })
    }

    fn extract_matrix_attachments(&self, content: Option<&Value>) -> Vec<String> {
        let Some(content) = content else {
            return Vec::new();
        };

        let mut attachments = Vec::new();

        if let Some(url) = content.get("url").and_then(Value::as_str) {
            attachments.push(url.to_string());
        }

        if let Some(obj) = content.as_object()
            && let Some(info) = obj.get("info").and_then(Value::as_object)
            && let Some(thumbnail_url) = info.get("thumbnail_url").and_then(Value::as_str)
        {
            attachments.push(thumbnail_url.to_string());
        }

        attachments
    }

    pub fn extract_reply_info(&self, content: Option<&Value>) -> Option<(String, String)> {
        let content = content?.as_object()?;
        let relates_to = content.get("m.relates_to")?.as_object()?;
        let in_reply_to = relates_to.get("m.in_reply_to")?.as_object()?;
        let event_id = in_reply_to.get("event_id")?.as_str()?;

        Some((event_id.to_string(), "Reply".to_string()))
    }

    pub fn extract_edit_info(&self, content: Option<&Value>) -> Option<String> {
        let content = content?.as_object()?;
        let relates_to = content.get("m.relates_to")?.as_object()?;

        if relates_to.get("rel_type")?.as_str()? != "m.replace" {
            return None;
        }

        relates_to.get("event_id")?.as_str().map(ToOwned::to_owned)
    }

    pub fn get_new_content(&self, content: Option<&Value>) -> Option<String> {
        let content = content?.as_object()?;
        let new_content = content.get("m.new_content")?.as_object()?;
        let body = new_content.get("body")?.as_str()?;

        Some(self.format_for_slack(body))
    }

    pub fn is_emote(&self, content: Option<&Value>) -> bool {
        content
            .and_then(|c| c.as_object())
            .and_then(|obj| obj.get("msgtype"))
            .and_then(Value::as_str)
            == Some("m.emote")
    }

    pub fn format_emote(&self, display_name: &str, body: &str) -> String {
        format!("* {} {}", display_name, self.format_for_slack(body))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn make_converter() -> MatrixToSlackConverter {
        let config = std::sync::Arc::new(crate::config::Config {
            bridge: crate::config::BridgeConfig {
                domain: "example.org".to_string(),
                port: 9005,
                bind_address: "127.0.0.1".to_string(),
                homeserver_url: "http://localhost:8008".to_string(),
                presence_interval: 500,
                disable_presence: false,
                disable_typing_notifications: false,
                disable_slack_mentions: false,
                disable_deletion_forwarding: false,
                enable_self_service_bridging: false,
                disable_portal_bridging: false,
                disable_read_receipts: false,
                disable_everyone_mention: false,
                disable_here_mention: false,
                disable_join_leave_notifications: false,
                disable_invite_notifications: false,
                disable_room_topic_notifications: false,
                determine_code_language: false,
                user_limit: None,
                admin_mxid: None,
                invalid_token_message: String::new(),
                user_activity: None,
            },
            registration: crate::config::RegistrationConfig::default(),
            auth: crate::config::AuthConfig {
                bot_token: "test".to_string(),
                app_token: None,
                client_id: None,
                client_secret: None,
                use_privileged_intents: false,
            },
            logging: crate::config::LoggingConfig {
                level: "info".to_string(),
                line_date_format: String::new(),
                format: "pretty".to_string(),
                file: None,
                files: vec![],
            },
            database: crate::config::DatabaseConfig {
                url: Some("sqlite://test.db".to_string()),
                conn_string: None,
                filename: None,
                user_store_path: None,
                room_store_path: None,
                max_connections: None,
                min_connections: None,
            },
            room: crate::config::RoomConfig {
                default_visibility: "private".to_string(),
                room_alias_prefix: "_slack".to_string(),
                enable_room_creation: true,
                kick_for: 0,
            },
            channel: crate::config::ChannelConfig {
                enable_channel_creation: false,
                channel_name_format: String::new(),
                name_pattern: String::new(),
                topic_format: String::new(),
                delete_options: crate::config::ChannelDeleteOptionsConfig::default(),
                enable_webhook: true,
                webhook_name: "_matrix".to_string(),
                webhook_avatar: String::new(),
            },
            limits: crate::config::LimitsConfig::default(),
            ghosts: crate::config::GhostsConfig {
                nick_pattern: String::new(),
                username_pattern: String::new(),
                username_template: String::new(),
                displayname_template: String::new(),
                avatar_url_template: None,
            },
            metrics: crate::config::MetricsConfig::default(),
        });

        MatrixToSlackConverter::new(Arc::new(MatrixAppservice::new(config).await.unwrap()))
    }

    #[tokio::test]
    async fn converts_ghost_user_to_slack_mention() {
        let converter = make_converter().await;
        let result = converter.format_for_slack("Hello @_slack_123456789:example.org!");
        assert_eq!(result, "Hello <@123456789>!");
    }

    #[tokio::test]
    async fn converts_ghost_alias_to_slack_channel() {
        let converter = make_converter().await;
        let result = converter.format_for_slack("Check #_slack_987654321:example.org");
        assert_eq!(result, "Check <#987654321>");
    }

    #[tokio::test]
    async fn leaves_regular_users_unchanged() {
        let converter = make_converter().await;
        let result = converter.format_for_slack("Hello @alice:example.org!");
        assert_eq!(result, "Hello @alice:example.org!");
    }

    #[tokio::test]
    async fn converts_html_bold_to_markdown() {
        let converter = make_converter().await;
        let result = converter.format_html_for_slack("<strong>bold</strong> text");
        assert_eq!(result, "**bold** text");
    }

    #[tokio::test]
    async fn converts_html_italic_to_markdown() {
        let converter = make_converter().await;
        let result = converter.format_html_for_slack("<em>italic</em> text");
        assert_eq!(result, "*italic* text");
    }

    #[tokio::test]
    async fn converts_html_link_to_slack_format() {
        let converter = make_converter().await;
        let result =
            converter.format_html_for_slack(r#"<a href="https://example.com">Example</a>"#);
        assert_eq!(result, "[Example](https://example.com)");
    }

    #[tokio::test]
    async fn converts_html_code_to_markdown() {
        let converter = make_converter().await;
        let result = converter.format_html_for_slack("<code>inline code</code>");
        assert_eq!(result, "`inline code`");
    }

    #[tokio::test]
    async fn extracts_reply_info() {
        let converter = make_converter().await;
        let content = serde_json::json!({
            "m.relates_to": {
                "m.in_reply_to": {
                    "event_id": "$event123"
                }
            }
        });
        let result = converter.extract_reply_info(Some(&content));
        assert_eq!(result, Some(("$event123".to_string(), "Reply".to_string())));
    }

    #[tokio::test]
    async fn extracts_edit_info() {
        let converter = make_converter().await;
        let content = serde_json::json!({
            "m.relates_to": {
                "rel_type": "m.replace",
                "event_id": "$original_event"
            }
        });
        let result = converter.extract_edit_info(Some(&content));
        assert_eq!(result, Some("$original_event".to_string()));
    }

    #[tokio::test]
    async fn formats_emote_correctly() {
        let converter = make_converter().await;
        let result = converter.format_emote("Alice", "waves hello");
        assert_eq!(result, "* Alice waves hello");
    }
}

