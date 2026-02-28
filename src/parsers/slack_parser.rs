use std::sync::Arc;

use anyhow::Result;
use chrono::Utc;
use regex::Regex;
use serde_json::{Value, json};

use super::common::{BridgeMessage, EmojiMention, MessageUtils, ParsedMessage};
use crate::slack::SlackClient;
use crate::emoji::EmojiHandler;

pub struct SlackMessageParser {
    _client: Arc<SlackClient>,
}

impl SlackMessageParser {
    pub fn new(client: Arc<SlackClient>) -> Self {
        Self { _client: client }
    }

    pub fn parse_message(&self, content: &str) -> ParsedMessage {
        ParsedMessage::new(content)
    }
}

pub struct SlackToMatrixConverter {
    slack_client: Arc<SlackClient>,
    emoji_handler: Option<Arc<EmojiHandler>>,
    domain: String,
    mention_regex: Regex,
    channel_regex: Regex,
    role_regex: Regex,
    emoji_regex: Regex,
    animated_emoji_regex: Regex,
    custom_emoji_regex: Regex,
    everyone_regex: Regex,
    here_regex: Regex,
    code_block_regex: Regex,
    inline_code_regex: Regex,
    bold_regex: Regex,
    italic_regex: Regex,
    underline_regex: Regex,
    strikethrough_regex: Regex,
    spoiler_regex: Regex,
    quote_regex: Regex,
    link_regex: Regex,
    unordered_list_regex: Regex,
    ordered_list_regex: Regex,
}

impl SlackToMatrixConverter {
    pub fn new(slack_client: Arc<SlackClient>) -> Self {
        Self {
            slack_client,
            emoji_handler: None,
            domain: String::new(),
            mention_regex: Regex::new(r"(?:<|&lt;)@([A-Z0-9]+)(?:\|[^>&]+)?(?:>|&gt;)").unwrap(),
            channel_regex: Regex::new(r"(?:<|&lt;)#([A-Z0-9]+)(?:\|[^>&]+)?(?:>|&gt;)").unwrap(),
            role_regex: Regex::new(r"(?:<|&lt;)@&(\d+)(?:>|&gt;)").unwrap(),
            emoji_regex: Regex::new(r":([a-zA-Z0-9_+-]+):").unwrap(),
            animated_emoji_regex: Regex::new(r"(?:<|&lt;)a:([a-zA-Z0-9_]+):(\d+)(?:>|&gt;)")
                .unwrap(),
            custom_emoji_regex: Regex::new(r"(?:<|&lt;):([a-zA-Z0-9_+-]+):(\d+)(?:>|&gt;)")
                .unwrap(),
            everyone_regex: Regex::new(r"<!everyone>").unwrap(),
            here_regex: Regex::new(r"<!here>|<!channel>").unwrap(),
            code_block_regex: Regex::new(r"```(?:([a-z]*)\n)?([\s\S]*?)```").unwrap(),
            inline_code_regex: Regex::new(r"`([^`]+)`").unwrap(),
            bold_regex: Regex::new(r"\*([^*]+)\*").unwrap(),
            italic_regex: Regex::new(r"_([^_]+)_").unwrap(),
            underline_regex: Regex::new(r"_([^_]+)_").unwrap(),
            strikethrough_regex: Regex::new(r"~([^~]+)~").unwrap(),
            spoiler_regex: Regex::new(r"\|\|([^|]+)\|\|").unwrap(),
            quote_regex: Regex::new(r"^> (.+)$").unwrap(),
            link_regex: Regex::new(r"(?:<|&lt;)(https?://.*?)(?:\|([^>&]+))?(?:>|&gt;)")
                .unwrap(),
            unordered_list_regex: Regex::new(r"^• (.+)$").unwrap(),
            ordered_list_regex: Regex::new(r"^(\d+)\. (.+)$").unwrap(),
        }
    }

    pub fn with_emoji_handler(mut self, handler: Arc<EmojiHandler>) -> Self {
        self.emoji_handler = Some(handler);
        self
    }

    pub fn with_domain(mut self, domain: String) -> Self {
        self.domain = domain;
        self
    }

    pub fn format_for_matrix(&self, message: &str) -> String {
        let mut result = message.to_string();
        result = self.convert_code_blocks_to_matrix(&result);
        result = self.convert_inline_code_to_matrix(&result);
        result = self.convert_links_to_matrix(&result);
        result = self.convert_mentions_to_matrix(&result);
        result = self.convert_channels_to_matrix(&result);
        result = self.convert_roles_to_matrix(&result);
        result = self.convert_emojis_to_matrix(&result);
        result = self.convert_everyone_here(&result);
        result
    }

    pub fn format_as_html(&self, message: &str) -> String {
        let mut result = message.to_string();

        result = self.escape_html(&result);

        result = self.convert_code_blocks_to_html(&result);
        result = self.convert_inline_code_to_html(&result);

        result = self.convert_slack_formatting_to_html(&result);
        result = self.convert_links_to_html(&result);
        result = self.convert_lists_to_html(&result);
        result = self.convert_quotes_to_html(&result);

        result = self.convert_mentions_to_html(&result);
        result = self.convert_channels_to_html(&result);
        result = self.convert_roles_to_html(&result);
        result = self.convert_emojis_to_html(&result);

        result = self.convert_everyone_here_to_html(&result);

        result = self.convert_newlines_to_html(&result);

        result
    }

    fn escape_html(&self, text: &str) -> String {
        text.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
    }

    fn convert_code_blocks_to_matrix(&self, text: &str) -> String {
        self.code_block_regex
            .replace_all(text, |caps: &regex::Captures| {
                let lang = caps.get(1).map(|m| m.as_str()).unwrap_or("");
                let code = &caps[2];
                if lang.is_empty() {
                    format!("<pre><code>{}</code></pre>", code)
                } else {
                    format!(
                        "<pre><code class=\"language-{}\">{}</code></pre>",
                        lang, code
                    )
                }
            })
            .to_string()
    }

    fn convert_inline_code_to_matrix(&self, text: &str) -> String {
        self.inline_code_regex
            .replace_all(text, |caps: &regex::Captures| {
                format!("<code>{}</code>", &caps[1])
            })
            .to_string()
    }

    fn convert_code_blocks_to_html(&self, text: &str) -> String {
        self.code_block_regex
            .replace_all(text, |caps: &regex::Captures| {
                let lang = caps.get(1).map(|m| m.as_str()).unwrap_or("");
                let code = &caps[2];
                if lang.is_empty() {
                    format!("<pre><code>{}</code></pre>", code)
                } else {
                    format!(
                        "<pre><code class=\"language-{}\">{}</code></pre>",
                        lang, code
                    )
                }
            })
            .to_string()
    }

    fn convert_inline_code_to_html(&self, text: &str) -> String {
        self.inline_code_regex
            .replace_all(text, |caps: &regex::Captures| {
                format!("<code>{}</code>", &caps[1])
            })
            .to_string()
    }

    fn convert_slack_formatting_to_html(&self, text: &str) -> String {
        let mut result = text.to_string();

        result = self
            .bold_regex
            .replace_all(&result, "<strong>$1</strong>")
            .to_string();

        result = self
            .italic_regex
            .replace_all(&result, "<em>$1</em>")
            .to_string();

        result = self
            .underline_regex
            .replace_all(&result, "<u>$1</u>")
            .to_string();

        result = self
            .strikethrough_regex
            .replace_all(&result, "<del>$1</del>")
            .to_string();

        result = self
            .spoiler_regex
            .replace_all(&result, "<span data-mx-spoiler>$1</span>")
            .to_string();

        result
    }

    fn convert_mentions_to_matrix(&self, text: &str) -> String {
        if self.domain.is_empty() {
            return text.to_string();
        }
        self.mention_regex
            .replace_all(text, |caps: &regex::Captures| {
                let user_id = &caps[1];
                format!(
                    "<a href=\"https://matrix.to/#/@_slack_{}:{}\">@_slack_{}</a>",
                    user_id, self.domain, user_id
                )
            })
            .to_string()
    }

    fn convert_mentions_to_html(&self, text: &str) -> String {
        if self.domain.is_empty() {
            return text.to_string();
        }
        self.mention_regex
            .replace_all(text, |caps: &regex::Captures| {
                let user_id = &caps[1];
                format!(
                    "<a href=\"https://matrix.to/#/@_slack_{}:{}\">@_slack_{}</a>",
                    user_id, self.domain, user_id
                )
            })
            .to_string()
    }

    fn convert_channels_to_matrix(&self, text: &str) -> String {
        if self.domain.is_empty() {
            return text.to_string();
        }
        self.channel_regex
            .replace_all(text, |caps: &regex::Captures| {
                let channel_id = &caps[1];
                format!(
                    "<a href=\"https://matrix.to/#/#_slack_{}:{}\">#_slack_{}</a>",
                    channel_id, self.domain, channel_id
                )
            })
            .to_string()
    }

    fn convert_channels_to_html(&self, text: &str) -> String {
        if self.domain.is_empty() {
            return text.to_string();
        }
        self.channel_regex
            .replace_all(text, |caps: &regex::Captures| {
                let channel_id = &caps[1];
                format!(
                    "<a href=\"https://matrix.to/#/#_slack_{}:{}\">#_slack_{}</a>",
                    channel_id, self.domain, channel_id
                )
            })
            .to_string()
    }

    fn convert_roles_to_matrix(&self, text: &str) -> String {
        if self.domain.is_empty() {
            return text.to_string();
        }
        self.role_regex
            .replace_all(text, |caps: &regex::Captures| {
                let role_id = &caps[1];
                format!("@role_{}", role_id)
            })
            .to_string()
    }

    fn convert_roles_to_html(&self, text: &str) -> String {
        if self.domain.is_empty() {
            return text.to_string();
        }
        self.role_regex
            .replace_all(text, |caps: &regex::Captures| {
                let role_id = &caps[1];
                format!("<font color=\"#99AAB5\">@role_{}</font>", role_id)
            })
            .to_string()
    }

    fn convert_emojis_to_matrix(&self, text: &str) -> String {
        let mut result = text.to_string();

        result = self
            .animated_emoji_regex
            .replace_all(&result, |caps: &regex::Captures| {
                let emoji_name = &caps[1];
                format!(":{}:", emoji_name)
            })
            .to_string();

        result = self
            .custom_emoji_regex
            .replace_all(&result, |caps: &regex::Captures| {
                let emoji_name = &caps[1];
                format!(":{}:", emoji_name)
            })
            .to_string();

        result = self
            .emoji_regex
            .replace_all(&result, |caps: &regex::Captures| {
                let emoji_name = &caps[1];
                format!(":{}:", emoji_name)
            })
            .to_string();

        result
    }

    fn convert_emojis_to_html(&self, text: &str) -> String {
        let mut result = text.to_string();

        result = self.animated_emoji_regex
            .replace_all(&result, |caps: &regex::Captures| {
                let emoji_name = &caps[1];
                let emoji_id = &caps[2];
                format!("<img data-mx-emoticon src=\"https://cdn.slackapp.com/emojis/{}.gif\" alt=\":{}:\" title=\":{}:\" height=\"32\" width=\"32\" />", 
                    emoji_id, emoji_name, emoji_name)
            })
            .to_string();

        result = self.custom_emoji_regex
            .replace_all(&result, |caps: &regex::Captures| {
                let emoji_name = &caps[1];
                let emoji_id = &caps[2];
                format!("<img data-mx-emoticon src=\"https://cdn.slackapp.com/emojis/{}.png\" alt=\":{}:\" title=\":{}:\" height=\"32\" width=\"32\" />", 
                    emoji_id, emoji_name, emoji_name)
            })
            .to_string();

        result
    }

    pub async fn convert_emojis_to_html_with_cache(&self, text: &str) -> String {
        let Some(handler) = &self.emoji_handler else {
            return self.convert_emojis_to_html(text);
        };

        let mut result = text.to_string();

        result = self
            .animated_emoji_regex
            .replace_all(&result, |caps: &regex::Captures| {
                format!("__ANIMATED_EMOJI_{}__", &caps[2])
            })
            .to_string();

        result = self
            .custom_emoji_regex
            .replace_all(&result, |caps: &regex::Captures| {
                format!("__STATIC_EMOJI_{}__", &caps[2])
            })
            .to_string();

        let mut emoji_info: Vec<(String, String, bool)> = Vec::new();
        for caps in self.custom_emoji_regex.captures_iter(text) {
            emoji_info.push((caps[1].to_string(), caps[2].to_string(), false));
        }
        for caps in self.animated_emoji_regex.captures_iter(text) {
            emoji_info.push((caps[1].to_string(), caps[2].to_string(), true));
        }

        for (emoji_name, emoji_id, animated) in emoji_info {
            let placeholder = if animated {
                format!("__ANIMATED_EMOJI_{}__", emoji_id)
            } else {
                format!("__STATIC_EMOJI_{}__", emoji_id)
            };

            match handler
                .get_or_upload_emoji(&emoji_id, &emoji_name, animated)
                .await
            {
                Ok(mxc_url) => {
                    let html = handler.emoji_to_matrix_html(&mxc_url, &emoji_name);
                    result = result.replace(&placeholder, &html);
                }
                Err(e) => {
                    tracing::warn!(
                        "Failed to upload emoji {} ({}): {}",
                        emoji_name,
                        emoji_id,
                        e
                    );
                    let ext = if animated { "gif" } else { "png" };
                    let fallback = format!(
                        "<img data-mx-emoticon src=\"https://cdn.slackapp.com/emojis/{}.{}\" alt=\":{}:\" title=\":{}:\" height=\"32\" width=\"32\" />",
                        emoji_id, ext, emoji_name, emoji_name
                    );
                    result = result.replace(&placeholder, &fallback);
                }
            }
        }

        result
    }

    pub async fn format_as_html_async(&self, message: &str) -> String {
        let mut result = message.to_string();

        result = self.escape_html(&result);

        result = self.convert_code_blocks_to_html(&result);
        result = self.convert_inline_code_to_html(&result);

        result = self.convert_slack_formatting_to_html(&result);

        result = self.convert_mentions_to_html(&result);
        result = self.convert_channels_to_html(&result);
        result = self.convert_roles_to_html(&result);
        result = self.convert_emojis_to_html_with_cache(&result).await;

        result = self.convert_everyone_here_to_html(&result);

        result = self.convert_newlines_to_html(&result);

        result
    }

    fn convert_everyone_here(&self, text: &str) -> String {
        let mut result = text.to_string();
        result = self
            .everyone_regex
            .replace_all(&result, "@everyone")
            .to_string();
        result = self.here_regex.replace_all(&result, "@here").to_string();
        result
    }

    fn convert_links_to_matrix(&self, text: &str) -> String {
        self.link_regex
            .replace_all(text, |caps: &regex::Captures| {
                let url = &caps[1];
                if let Some(label) = caps.get(2) {
                    format!("[{}]({})", label.as_str(), url)
                } else {
                    url.to_string()
                }
            })
            .to_string()
    }

    fn convert_links_to_html(&self, text: &str) -> String {
        self.link_regex
            .replace_all(text, |caps: &regex::Captures| {
                let url = &caps[1];
                if let Some(label) = caps.get(2) {
                    format!("<a href=\"{}\">{}</a>", url, label.as_str())
                } else {
                    format!("<a href=\"{}\">{}</a>", url, url)
                }
            })
            .to_string()
    }

    fn convert_lists_to_html(&self, text: &str) -> String {
        let lines: Vec<&str> = text.lines().collect();
        let mut result = String::new();
        let mut in_unordered_list = false;
        let mut in_ordered_list = false;

        for line in lines {
            if let Some(caps) = self.unordered_list_regex.captures(line) {
                if !in_unordered_list {
                    result.push_str("<ul>");
                    in_unordered_list = true;
                }
                result.push_str(&format!("<li>{}</li>", &caps[1]));
            } else if let Some(caps) = self.ordered_list_regex.captures(line) {
                if in_unordered_list {
                    result.push_str("</ul>");
                    in_unordered_list = false;
                }
                if !in_ordered_list {
                    result.push_str("<ol>");
                    in_ordered_list = true;
                }
                result.push_str(&format!("<li>{}</li>", &caps[2]));
            } else {
                if in_unordered_list {
                    result.push_str("</ul>");
                    in_unordered_list = false;
                }
                if in_ordered_list {
                    result.push_str("</ol>");
                    in_ordered_list = false;
                }
                result.push_str(line);
                result.push('\n');
            }
        }

        if in_unordered_list {
            result.push_str("</ul>");
        }
        if in_ordered_list {
            result.push_str("</ol>");
        }

        result
    }

    fn convert_quotes_to_html(&self, text: &str) -> String {
        self.quote_regex
            .replace_all(text, |caps: &regex::Captures| {
                format!("<blockquote>{}</blockquote>", &caps[1])
            })
            .to_string()
    }

    fn convert_everyone_here_to_html(&self, text: &str) -> String {
        let mut result = text.to_string();
        result = self
            .everyone_regex
            .replace_all(&result, "<font color=\"#FF0000\">@everyone</font>")
            .to_string();
        result = self
            .here_regex
            .replace_all(&result, "<font color=\"#FF0000\">@here</font>")
            .to_string();
        result
    }

    fn convert_newlines_to_html(&self, text: &str) -> String {
        text.replace("\n", "<br/>")
    }

    pub async fn convert_message(
        &self,
        slack_message: &str,
        matrix_room_id: &str,
    ) -> Result<BridgeMessage> {
        let formatted = self.format_as_html_async(slack_message).await;

        Ok(BridgeMessage {
            source_platform: "slack".to_string(),
            target_platform: "matrix".to_string(),
            source_id: format!("slack:{}", matrix_room_id),
            target_id: matrix_room_id.to_string(),
            content: self.format_for_matrix(slack_message),
            formatted_content: Some(formatted),
            timestamp: Utc::now().to_rfc3339(),
            attachments: Vec::new(),
        })
    }

    pub async fn convert_message_with_emoji_cache(
        &self,
        slack_message: &str,
        matrix_room_id: &str,
    ) -> Result<BridgeMessage> {
        let formatted = self.format_as_html_async(slack_message).await;
        let plain = self.format_for_matrix(slack_message);

        Ok(BridgeMessage {
            source_platform: "slack".to_string(),
            target_platform: "matrix".to_string(),
            source_id: format!("slack:{}", matrix_room_id),
            target_id: matrix_room_id.to_string(),
            content: plain,
            formatted_content: Some(formatted),
            timestamp: Utc::now().to_rfc3339(),
            attachments: Vec::new(),
        })
    }

    pub fn to_matrix_content(&self, message: &BridgeMessage) -> Value {
        if let Some(ref formatted) = message.formatted_content {
            json!({
                "msgtype": "m.text",
                "body": message.content,
                "format": "org.matrix.custom.html",
                "formatted_body": formatted
            })
        } else {
            json!({
                "msgtype": "m.text",
                "body": message.content
            })
        }
    }

    pub fn create_reply_content(
        &self,
        body: &str,
        reply_event_id: &str,
        reply_body: &str,
        reply_sender: &str,
    ) -> Value {
        let formatted_reply = format!(
            "<mx-reply><blockquote><a href=\"https://matrix.to/#/{}\">In reply to</a> <a href=\"https://matrix.to/#/{}\">{}</a><br/>{}</blockquote></mx-reply>{}",
            reply_event_id, reply_sender, reply_sender, reply_body, body
        );

        let plain_reply = format!("> <{}> {}\n\n{}", reply_sender, reply_body, body);

        json!({
            "msgtype": "m.text",
            "body": plain_reply,
            "format": "org.matrix.custom.html",
            "formatted_body": formatted_reply,
            "m.relates_to": {
                "m.in_reply_to": {
                    "event_id": reply_event_id
                }
            }
        })
    }

    pub fn create_edit_content(
        &self,
        new_body: &str,
        original_event_id: &str,
        new_formatted_body: Option<&str>,
    ) -> Value {
        let new_content = if let Some(formatted) = new_formatted_body {
            json!({
                "msgtype": "m.text",
                "body": new_body,
                "format": "org.matrix.custom.html",
                "formatted_body": formatted
            })
        } else {
            json!({
                "msgtype": "m.text",
                "body": new_body
            })
        };

        json!({
            "msgtype": "m.text",
            "body": format!("* {}", new_body),
            "format": "org.matrix.custom.html",
            "formatted_body": format!("* {}", new_formatted_body.unwrap_or(new_body)),
            "m.new_content": new_content,
            "m.relates_to": {
                "rel_type": "m.replace",
                "event_id": original_event_id
            }
        })
    }

    pub fn extract_emoji_info(&self, content: &str) -> Vec<EmojiMention> {
        MessageUtils::extract_slack_emojis(content)
    }

    pub fn is_spoiler(&self, content: &str) -> bool {
        self.spoiler_regex.is_match(content)
    }

    pub fn has_code_block(&self, content: &str) -> bool {
        self.code_block_regex.is_match(content)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_converter() -> SlackToMatrixConverter {
        tokio_test::block_on(async {
            SlackToMatrixConverter::new(Arc::new(
                crate::slack::SlackClient::new(std::sync::Arc::new(crate::config::Config {
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
                }))
                .await
                .unwrap(),
            ))
            .with_domain("example.org".to_string())
        })
    }

    #[test]
    fn converts_user_mention_to_matrix() {
        let converter = make_converter();
        let result = converter.format_for_matrix("Hello <@U123456789>!");
        assert!(result.contains("@_slack_U123456789:example.org"));
    }

    #[test]
    fn converts_user_mention_with_nickname_to_matrix() {
        let converter = make_converter();
        let result = converter.format_for_matrix("Hello <@U123456789|john>!");
        assert!(result.contains("@_slack_U123456789:example.org"));
    }

    #[test]
    fn converts_channel_mention_to_matrix() {
        let converter = make_converter();
        let result = converter.format_for_matrix("Check out <#C987654321>!");
        assert!(result.contains("#_slack_C987654321:example.org"));
    }

    #[test]
    fn converts_custom_emoji_to_text() {
        let converter = make_converter();
        let result = converter.format_for_matrix("Nice! <:cool:12345>");
        assert_eq!(result, "Nice! :cool:");
    }

    #[test]
    fn converts_animated_emoji_to_text() {
        let converter = make_converter();
        let result = converter.format_for_matrix("Wow! <a:dance:67890>");
        assert_eq!(result, "Wow! :dance:");
    }

    #[test]
    fn converts_bold_to_html() {
        let converter = make_converter();
        let result = converter.format_as_html("*bold text*");
        assert!(result.contains("<strong>bold text</strong>"));
    }

    #[test]
    fn converts_links_to_html() {
        let converter = make_converter();
        let result = converter.format_as_html("Check out <https://example.com|Example>!");
        assert!(result.contains("<a href=\"https://example.com\">Example</a>"));
    }

    #[test]
    fn converts_lists_to_html() {
        let converter = make_converter();
        let result = converter.format_as_html("• Item 1\n• Item 2");
        assert!(result.contains("<ul>"));
        assert!(result.contains("<li>Item 1</li>"));
        assert!(result.contains("<li>Item 2</li>"));
        assert!(result.contains("</ul>"));
    }

    #[test]
    fn converts_strikethrough_to_html() {
        let converter = make_converter();
        let result = converter.format_as_html("~strikethrough~");
        assert!(result.contains("<del>strikethrough</del>"));
    }

    #[test]
    fn converts_code_block_to_html() {
        let converter = make_converter();
        let result = converter.format_as_html("```rust\nlet x = 1;\n```");
        assert!(result.contains("<pre>"));
        assert!(result.contains("language-rust"));
    }

    #[test]
    fn converts_inline_code_to_html() {
        let converter = make_converter();
        let result = converter.format_as_html("`inline code`");
        assert!(result.contains("<code>inline code</code>"));
    }

    #[test]
    fn detects_spoiler() {
        let converter = make_converter();
        assert!(converter.is_spoiler("This is ||spoiler|| text"));
        assert!(!converter.is_spoiler("Normal text"));
    }

    #[test]
    fn detects_code_block() {
        let converter = make_converter();
        assert!(converter.has_code_block("Here is code:\n```rust\ncode\n```"));
        assert!(!converter.has_code_block("No code here"));
    }

    #[test]
    fn creates_matrix_content_with_formatting() {
        let converter = make_converter();
        let msg = BridgeMessage {
            source_platform: "slack".to_string(),
            target_platform: "matrix".to_string(),
            source_id: "test".to_string(),
            target_id: "room".to_string(),
            content: "plain text".to_string(),
            formatted_content: Some("<b>formatted</b>".to_string()),
            timestamp: "now".to_string(),
            attachments: vec![],
        };
        let content = converter.to_matrix_content(&msg);
        assert_eq!(content["format"], "org.matrix.custom.html");
        assert_eq!(content["formatted_body"], "<b>formatted</b>");
    }

    #[test]
    fn creates_edit_content() {
        let converter = make_converter();
        let content = converter.create_edit_content("new message", "$event123", None);
        assert_eq!(content["m.relates_to"]["rel_type"], "m.replace");
        assert_eq!(content["m.relates_to"]["event_id"], "$event123");
        assert_eq!(content["m.new_content"]["body"], "new message");
    }
}
