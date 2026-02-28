use std::sync::Arc;

use serde_json::Value;

use crate::slack::{SlackClient, SlackEmbed, EmbedAuthor, EmbedFooter};
use crate::emoji::EmojiHandler;
use crate::matrix::{MatrixAppservice, MatrixEvent};
use crate::parsers::{SlackToMatrixConverter, MatrixToSlackConverter, MessageUtils};

const ATTACHMENT_TYPES: &[&str] = &["m.image", "m.audio", "m.video", "m.file", "m.sticker"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MessageRelation {
    Reply { event_id: String },
    Replace { event_id: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageAttachment {
    pub name: String,
    pub url: String,
    pub kind: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatrixInboundMessage {
    pub event_id: Option<String>,
    pub room_id: String,
    pub sender: String,
    pub body: String,
    pub relation: Option<MessageRelation>,
    pub attachments: Vec<MessageAttachment>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlackInboundMessage {
    pub channel_id: String,
    pub sender_id: String,
    pub content: String,
    pub attachments: Vec<String>,
    pub reply_to: Option<String>,
    pub edit_of: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutboundSlackMessage {
    pub content: String,
    pub reply_to: Option<String>,
    pub edit_of: Option<String>,
    pub attachments: Vec<String>,
    pub embed: Option<SlackEmbed>,
    pub use_embed: bool,
}

impl OutboundSlackMessage {
    pub fn new(content: String) -> Self {
        Self {
            content,
            reply_to: None,
            edit_of: None,
            attachments: Vec::new(),
            embed: None,
            use_embed: false,
        }
    }

    pub fn with_embed(mut self, embed: SlackEmbed) -> Self {
        self.embed = Some(embed);
        self.use_embed = true;
        self
    }

    pub fn render_content(&self) -> String {
        let mut parts = Vec::new();
        if let Some(reply_to) = &self.reply_to {
            parts.push(format!("(reply:{reply_to})"));
        }
        if let Some(edit_of) = &self.edit_of {
            parts.push(format!("(edit:{edit_of})"));
        }
        if !self.content.is_empty() {
            parts.push(self.content.clone());
        }
        if !self.attachments.is_empty() {
            parts.push(self.attachments.join(" "));
        }
        parts.join("\n")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutboundMatrixMessage {
    pub body: String,
    pub reply_to: Option<String>,
    pub edit_of: Option<String>,
    pub attachments: Vec<String>,
}

impl OutboundMatrixMessage {
    pub fn render_body(&self) -> String {
        let mut body = self.body.clone();
        if let Some(reply_to) = &self.reply_to {
            body = format!("> reply to {reply_to}\n{body}");
        }
        if let Some(edit_of) = &self.edit_of {
            body = format!("* {body}\n(edit:{edit_of})");
        }
        if !self.attachments.is_empty() {
            if !body.is_empty() {
                body.push('\n');
            }
            body.push_str(&self.attachments.join("\n"));
        }
        body
    }
}

#[derive(Clone)]
pub struct MessageFlow {
    matrix_converter: Arc<MatrixToSlackConverter>,
    slack_converter: Arc<SlackToMatrixConverter>,
}

impl MessageFlow {
    pub fn new(matrix_client: Arc<MatrixAppservice>, slack_client: Arc<SlackClient>) -> Self {
        Self::with_emoji_handler(matrix_client, slack_client, None)
    }

    pub fn with_emoji_handler(
        matrix_client: Arc<MatrixAppservice>,
        slack_client: Arc<SlackClient>,
        emoji_handler: Option<Arc<EmojiHandler>>,
    ) -> Self {
        let domain = matrix_client.config().bridge.domain.clone();
        let mut converter = SlackToMatrixConverter::new(slack_client).with_domain(domain);

        if let Some(handler) = emoji_handler {
            converter = converter.with_emoji_handler(handler);
        }

        Self {
            matrix_converter: Arc::new(MatrixToSlackConverter::new(matrix_client)),
            slack_converter: Arc::new(converter),
        }
    }

    pub fn parse_matrix_event(event: &MatrixEvent) -> Option<MatrixInboundMessage> {
        if !matches!(event.event_type.as_str(), "m.room.message" | "m.sticker") {
            return None;
        }

        let content = event.content.as_ref()?;
        let content_for_body = content
            .get("m.new_content")
            .filter(|value| value.is_object())
            .unwrap_or(content);
        let body = MessageUtils::extract_plain_text(content_for_body);

        let msgtype = content_for_body
            .get("msgtype")
            .and_then(Value::as_str)
            .unwrap_or_else(|| {
                if event.event_type == "m.sticker" {
                    "m.sticker"
                } else {
                    "m.text"
                }
            })
            .to_string();

        let relation = parse_relation(content);
        let attachments = parse_attachments(content_for_body, &msgtype);

        if body.is_empty() && attachments.is_empty() {
            return None;
        }

        Some(MatrixInboundMessage {
            event_id: event.event_id.clone(),
            room_id: event.room_id.clone(),
            sender: event.sender.clone(),
            body,
            relation,
            attachments,
        })
    }

    pub fn matrix_to_slack(&self, message: &MatrixInboundMessage) -> OutboundSlackMessage {
        let reply_to = match &message.relation {
            Some(MessageRelation::Reply { event_id }) => Some(event_id.clone()),
            _ => None,
        };
        let edit_of = match &message.relation {
            Some(MessageRelation::Replace { event_id }) => Some(event_id.clone()),
            _ => None,
        };
        let attachments = message
            .attachments
            .iter()
            .map(|attachment| attachment.url.clone())
            .collect();

        OutboundSlackMessage {
            content: self.matrix_converter.format_for_slack(&message.body),
            reply_to,
            edit_of,
            attachments,
            embed: None,
            use_embed: false,
        }
    }

    pub fn matrix_to_slack_with_embed(
        &self,
        message: &MatrixInboundMessage,
        sender_displayname: &str,
        sender_avatar_url: Option<&str>,
        reply_info: Option<(&str, &str)>,
    ) -> OutboundSlackMessage {
        let reply_to = match &message.relation {
            Some(MessageRelation::Reply { event_id }) => Some(event_id.clone()),
            _ => None,
        };
        let edit_of = match &message.relation {
            Some(MessageRelation::Replace { event_id }) => Some(event_id.clone()),
            _ => None,
        };
        let attachments = message
            .attachments
            .iter()
            .map(|attachment| attachment.url.clone())
            .collect();

        let embed = crate::slack::build_matrix_message_embed(
            sender_displayname,
            sender_avatar_url,
            &message.body,
            reply_info,
        );

        OutboundSlackMessage {
            content: String::new(),
            reply_to,
            edit_of,
            attachments,
            embed: Some(embed),
            use_embed: true,
        }
    }

    pub fn slack_to_matrix(&self, message: &SlackInboundMessage) -> OutboundMatrixMessage {
        OutboundMatrixMessage {
            body: self.slack_converter.format_for_matrix(&message.content),
            reply_to: message.reply_to.clone(),
            edit_of: message.edit_of.clone(),
            attachments: message.attachments.clone(),
        }
    }

    pub async fn slack_to_matrix_async(
        &self,
        message: &SlackInboundMessage,
    ) -> (String, Option<String>) {
        let plain = self.slack_converter.format_for_matrix(&message.content);
        let formatted = self
            .slack_converter
            .format_as_html_async(&message.content)
            .await;
        (plain, Some(formatted))
    }

    pub fn slack_converter(&self) -> &SlackToMatrixConverter {
        &self.slack_converter
    }
}

fn parse_relation(content: &Value) -> Option<MessageRelation> {
    let relates_to = content.get("m.relates_to")?;
    if let Some(reply_event_id) = relates_to
        .get("m.in_reply_to")
        .and_then(|inner| inner.get("event_id"))
        .and_then(Value::as_str)
    {
        return Some(MessageRelation::Reply {
            event_id: reply_event_id.to_string(),
        });
    }
    if relates_to
        .get("rel_type")
        .and_then(Value::as_str)
        .is_some_and(|kind| kind == "m.replace")
        && let Some(edit_event_id) = relates_to.get("event_id").and_then(Value::as_str)
    {
        return Some(MessageRelation::Replace {
            event_id: edit_event_id.to_string(),
        });
    }
    None
}

fn parse_attachments(content: &Value, msgtype: &str) -> Vec<MessageAttachment> {
    if !ATTACHMENT_TYPES.contains(&msgtype) {
        return Vec::new();
    }
    let Some(url) = content.get("url").and_then(Value::as_str) else {
        return Vec::new();
    };
    let name = content
        .get("body")
        .and_then(Value::as_str)
        .unwrap_or("matrix-media")
        .to_string();

    vec![MessageAttachment {
        name,
        url: url.to_string(),
        kind: msgtype.to_string(),
    }]
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use serde_json::json;

    use super::{SlackInboundMessage, MessageFlow, MessageRelation};
    use crate::config::{
        AuthConfig, BridgeConfig, ChannelConfig, ChannelDeleteOptionsConfig, Config,
        DatabaseConfig, GhostsConfig, LimitsConfig, LoggingConfig, MetricsConfig,
        RegistrationConfig, RoomConfig,
    };
    use crate::slack::SlackClient;
    use crate::matrix::{MatrixAppservice, MatrixEvent};

    fn test_config() -> Arc<Config> {
        Arc::new(Config {
            bridge: BridgeConfig {
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
                invalid_token_message: "Your Slack bot token seems to be invalid".to_string(),
                user_activity: None,
            },
            registration: RegistrationConfig {
                bridge_id: "test-bridge".to_string(),
                appservice_token: "test_as_token".to_string(),
                homeserver_token: "test_hs_token".to_string(),
                ..Default::default()
            },
            auth: AuthConfig {
                app_token: None,
                bot_token: "token".to_string(),
                client_id: None,
                client_secret: None,
                use_privileged_intents: false,
            },
            logging: LoggingConfig {
                level: "info".to_string(),
                line_date_format: "MMM-D HH:mm:ss.SSS".to_string(),
                format: "pretty".to_string(),
                file: None,
                files: vec![],
            },
            database: DatabaseConfig {
                url: Some("postgres://localhost/bridge".to_string()),
                conn_string: None,
                filename: None,
                user_store_path: None,
                room_store_path: None,
                max_connections: Some(1),
                min_connections: Some(1),
            },
            room: RoomConfig {
                default_visibility: "private".to_string(),
                room_alias_prefix: "_slack".to_string(),
                enable_room_creation: true,
                kick_for: 30000,
            },
            channel: ChannelConfig {
                enable_channel_creation: false,
                channel_name_format: ":name".to_string(),
                name_pattern: "[Slack] :guild :name".to_string(),
                topic_format: ":topic".to_string(),
                delete_options: ChannelDeleteOptionsConfig::default(),
                enable_webhook: true,
                webhook_name: "_matrix".to_string(),
                webhook_avatar: String::new(),
            },
            limits: LimitsConfig::default(),
            ghosts: GhostsConfig {
                nick_pattern: ":nick".to_string(),
                username_pattern: ":username#:tag".to_string(),
                username_template: "_slack_:id".to_string(),
                displayname_template: ":username".to_string(),
                avatar_url_template: None,
            },
            metrics: MetricsConfig::default(),
        })
    }

    #[tokio::test]
    async fn parse_matrix_event_extracts_reply_and_attachment() {
        let event = MatrixEvent {
            event_id: Some("$event".to_string()),
            event_type: "m.room.message".to_string(),
            room_id: "!room:example.org".to_string(),
            sender: "@alice:example.org".to_string(),
            state_key: None,
            content: Some(json!({
                "msgtype": "m.image",
                "body": "cat.png",
                "url": "mxc://example.org/cat",
                "m.relates_to": {
                    "m.in_reply_to": {
                        "event_id": "$source"
                    }
                }
            })),
            timestamp: None,
        };

        let parsed = MessageFlow::parse_matrix_event(&event).expect("matrix message should parse");
        assert_eq!(
            parsed.relation,
            Some(MessageRelation::Reply {
                event_id: "$source".to_string()
            })
        );
        assert_eq!(parsed.attachments.len(), 1);
        assert_eq!(parsed.attachments[0].url, "mxc://example.org/cat");
    }

    #[tokio::test]
    async fn matrix_to_slack_marks_edit_messages() {
        let config = test_config();
        let matrix_client = Arc::new(MatrixAppservice::new(config.clone()).await.expect("matrix"));
        let slack_client = Arc::new(SlackClient::new(config).await.expect("slack"));
        let flow = MessageFlow::new(matrix_client, slack_client);

        let event = MatrixEvent {
            event_id: Some("$event".to_string()),
            event_type: "m.room.message".to_string(),
            room_id: "!room:example.org".to_string(),
            sender: "@alice:example.org".to_string(),
            state_key: None,
            content: Some(json!({
                "msgtype": "m.text",
                "body": "new body",
                "m.relates_to": {
                    "rel_type": "m.replace",
                    "event_id": "$old"
                },
                "m.new_content": {
                    "msgtype": "m.text",
                    "body": "new body"
                }
            })),
            timestamp: None,
        };
        let inbound = MessageFlow::parse_matrix_event(&event).expect("matrix message");
        let outbound = flow.matrix_to_slack(&inbound);
        assert_eq!(outbound.edit_of, Some("$old".to_string()));
        assert_eq!(outbound.content, "new body".to_string());
    }

    #[tokio::test]
    async fn slack_to_matrix_sanitizes_markdown_and_keeps_reply() {
        let config = test_config();
        let matrix_client = Arc::new(MatrixAppservice::new(config.clone()).await.expect("matrix"));
        let slack_client = Arc::new(SlackClient::new(config).await.expect("slack"));
        let flow = MessageFlow::new(matrix_client, slack_client);

        let outbound = flow.slack_to_matrix(&SlackInboundMessage {
            channel_id: "123".to_string(),
            sender_id: "55".to_string(),
            content: "*bold*".to_string(),
            attachments: vec!["https://example.org/a.png".to_string()],
            reply_to: Some("slack-msg-1".to_string()),
            edit_of: None,
        });

        assert_eq!(outbound.body, "*bold*".to_string());
        assert_eq!(outbound.reply_to, Some("slack-msg-1".to_string()));
        assert_eq!(
            outbound.attachments,
            vec!["https://example.org/a.png".to_string()]
        );
    }
}

