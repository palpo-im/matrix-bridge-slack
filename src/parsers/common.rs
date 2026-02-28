use std::collections::HashSet;

use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParsedMessage {
    pub text: String,
    pub formatted: Option<String>,
    pub mentions: Vec<String>,
    pub channel_mentions: Vec<String>,
    pub role_mentions: Vec<String>,
    pub emoji_mentions: Vec<EmojiMention>,
    pub attachments: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmojiMention {
    pub name: String,
    pub id: String,
    pub animated: bool,
}

impl ParsedMessage {
    pub fn new(content: &str) -> Self {
        Self {
            text: content.to_string(),
            formatted: None,
            mentions: MessageUtils::extract_slack_user_mentions(content),
            channel_mentions: MessageUtils::extract_slack_channel_mentions(content),
            role_mentions: MessageUtils::extract_slack_role_mentions(content),
            emoji_mentions: MessageUtils::extract_slack_emojis(content),
            attachments: MessageUtils::extract_slack_attachments(content),
        }
    }

    pub fn formatted_or_text(&self) -> String {
        self.formatted.clone().unwrap_or_else(|| self.text.clone())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BridgeMessage {
    pub source_platform: String,
    pub target_platform: String,
    pub source_id: String,
    pub target_id: String,
    pub content: String,
    pub formatted_content: Option<String>,
    pub timestamp: String,
    pub attachments: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct HtmlToMarkdownConfig {
    pub preserve_links: bool,
    pub preserve_images: bool,
    pub code_block_style: String,
}

impl Default for HtmlToMarkdownConfig {
    fn default() -> Self {
        Self {
            preserve_links: true,
            preserve_images: true,
            code_block_style: "fenced".to_string(),
        }
    }
}

pub struct MessageUtils;

impl MessageUtils {
    pub fn sanitize_markdown(text: &str) -> String {
        let mut result = String::new();
        let chars: Vec<char> = text.chars().collect();
        let mut i = 0;

        while i < chars.len() {
            let c = chars[i];

            if c == '\\' && i + 1 < chars.len() {
                result.push(c);
                result.push(chars[i + 1]);
                i += 2;
                continue;
            }

            if c == '*' || c == '_' || c == '~' || c == '`' || c == '|' {
                result.push('\\');
            }
            result.push(c);
            i += 1;
        }

        result
    }

    pub fn extract_plain_text(content: &Value) -> String {
        match content {
            Value::String(s) => s.clone(),
            Value::Object(obj) => {
                if let Some(body) = obj.get("body").and_then(Value::as_str) {
                    body.to_string()
                } else if let Some(formatted) = obj.get("formatted_body").and_then(Value::as_str) {
                    strip_html_tags(formatted)
                } else {
                    String::new()
                }
            }
            _ => String::new(),
        }
    }

    pub fn extract_formatted_body(content: &Value) -> Option<String> {
        content
            .as_object()
            .and_then(|obj| obj.get("formatted_body"))
            .and_then(Value::as_str)
            .map(ToOwned::to_owned)
    }

    pub fn extract_slack_user_mentions(content: &str) -> Vec<String> {
        let re = Regex::new(r"<@!?(\d+)>").unwrap();
        re.captures_iter(content)
            .filter_map(|cap| cap.get(1).map(|m| m.as_str().to_string()))
            .collect()
    }

    pub fn extract_slack_channel_mentions(content: &str) -> Vec<String> {
        let re = Regex::new(r"<#(\d+)>").unwrap();
        re.captures_iter(content)
            .filter_map(|cap| cap.get(1).map(|m| m.as_str().to_string()))
            .collect()
    }

    pub fn extract_slack_role_mentions(content: &str) -> Vec<String> {
        let re = Regex::new(r"<@&(\d+)>").unwrap();
        re.captures_iter(content)
            .filter_map(|cap| cap.get(1).map(|m| m.as_str().to_string()))
            .collect()
    }

    pub fn extract_slack_emojis(content: &str) -> Vec<EmojiMention> {
        let re = Regex::new(r"<(a?):([a-zA-Z0-9_]+):(\d+)>").unwrap();
        re.captures_iter(content)
            .filter_map(|cap| {
                let animated = cap.get(1).map(|m| m.as_str() == "a").unwrap_or(false);
                let name = cap.get(2)?.as_str().to_string();
                let id = cap.get(3)?.as_str().to_string();
                Some(EmojiMention { name, id, animated })
            })
            .collect()
    }

    pub fn extract_slack_attachments(content: &str) -> Vec<String> {
        let re = Regex::new(r"https?://[^\s<>\[\](){}\x22\x27]+\.(?:png|jpg|jpeg|gif|webp|mp4|webm|mp3|ogg|wav|pdf|zip|txt)(?:\?[^\s<>\[\](){}\x22\x27]*)?").unwrap();
        re.find_iter(content)
            .map(|m| m.as_str().to_string())
            .collect()
    }

    pub fn convert_html_to_slack_markdown(html: &str) -> String {
        let mut result = html.to_string();

        result = Self::convert_html_links(&result);
        result = Self::convert_html_formatting(&result);
        result = Self::convert_html_code_blocks(&result);
        result = Self::convert_html_lists(&result);
        result = Self::convert_html_blockquotes(&result);
        result = Self::convert_html_headers(&result);
        result = strip_html_tags(&result);
        result = Self::cleanup_whitespace(&result);

        result
    }

    fn convert_html_links(html: &str) -> String {
        let re = Regex::new(r#"<a[^>]*href="([^"]*)"[^>]*>([^<]*)</a>"#).unwrap();
        re.replace_all(html, |caps: &regex::Captures| {
            let url = &caps[1];
            let text = &caps[2];
            if url == text {
                format!("<{}>", url)
            } else {
                format!("[{}]({})", text, url)
            }
        })
        .to_string()
    }

    fn convert_html_formatting(html: &str) -> String {
        let mut result = html.to_string();

        let strong_re = Regex::new(r"<strong>([^<]*)</strong>").unwrap();
        result = strong_re.replace_all(&result, "**$1**").to_string();

        let b_re = Regex::new(r"<b>([^<]*)</b>").unwrap();
        result = b_re.replace_all(&result, "**$1**").to_string();

        let em_re = Regex::new(r"<em>([^<]*)</em>").unwrap();
        result = em_re.replace_all(&result, "*$1*").to_string();

        let i_re = Regex::new(r"<i>([^<]*)</i>").unwrap();
        result = i_re.replace_all(&result, "*$1*").to_string();

        let del_re = Regex::new(r"<del>([^<]*)</del>").unwrap();
        result = del_re.replace_all(&result, "~~$1~~").to_string();

        let s_re = Regex::new(r"<s>([^<]*)</s>").unwrap();
        result = s_re.replace_all(&result, "~~$1~~").to_string();

        let u_re = Regex::new(r"<u>([^<]*)</u>").unwrap();
        result = u_re.replace_all(&result, "__$1__").to_string();

        let code_re = Regex::new(r"<code>([^<]*)</code>").unwrap();
        result = code_re.replace_all(&result, "`$1`").to_string();

        let span_re = Regex::new(r"<span[^>]*>([^<]*)</span>").unwrap();
        result = span_re.replace_all(&result, "$1").to_string();

        result
    }

    fn convert_html_code_blocks(html: &str) -> String {
        let re =
            Regex::new(r#"<pre[^>]*><code(?:\s+class="language-([^"]*)")?>([^<]*)</code></pre>"#)
                .unwrap();
        re.replace_all(html, |caps: &regex::Captures| {
            let lang = caps.get(1).map(|m| m.as_str()).unwrap_or("");
            let code = &caps[2];
            if lang.is_empty() {
                format!("```\n{}\n```", code)
            } else {
                format!("```{}\n{}\n```", lang, code)
            }
        })
        .to_string()
    }

    fn convert_html_lists(html: &str) -> String {
        let li_re = Regex::new(r"<li>([^<]*)</li>").unwrap();
        let mut result = li_re.replace_all(html, "- $1\n").to_string();

        let ul_re = Regex::new(r"</?ul>").unwrap();
        result = ul_re.replace_all(&result, "").to_string();

        let ol_re = Regex::new(r"</?ol>").unwrap();
        result = ol_re.replace_all(&result, "").to_string();

        result
    }

    fn convert_html_blockquotes(html: &str) -> String {
        let re = Regex::new(r"<blockquote>([^<]*)</blockquote>").unwrap();
        re.replace_all(html, |caps: &regex::Captures| {
            let text = &caps[1];
            text.lines()
                .map(|line| format!("> {}", line))
                .collect::<Vec<_>>()
                .join("\n")
        })
        .to_string()
    }

    fn convert_html_headers(html: &str) -> String {
        let mut result = html.to_string();

        for level in (1..=6).rev() {
            let re = Regex::new(&format!("<h{}[^>]*>([^<]*)</h{}>", level, level)).unwrap();
            let prefix = "#".repeat(level);
            result = re
                .replace_all(&result, format!("{} $1", prefix))
                .to_string();
        }

        result
    }

    fn cleanup_whitespace(text: &str) -> String {
        let re = Regex::new(r"\n{3,}").unwrap();
        re.replace_all(text, "\n\n").trim().to_string()
    }

    pub fn convert_matrix_reply_to_slack(
        body: &str,
        reply_content: &str,
        reply_author: &str,
    ) -> String {
        let quoted_reply: String = reply_content
            .lines()
            .map(|line| format!("> {}", line))
            .collect::<Vec<_>>()
            .join("\n");

        format!("**{}**:\n{}\n\n{}", reply_author, quoted_reply, body)
    }

    pub fn escape_slack_special_chars(text: &str) -> String {
        text.replace('\\', "\\\\")
    }

    pub fn extract_unique_values<T: std::hash::Hash + Eq + Clone>(items: Vec<T>) -> Vec<T> {
        let set: HashSet<T> = items.into_iter().collect();
        set.into_iter().collect()
    }
}

fn strip_html_tags(html: &str) -> String {
    let tag_re = Regex::new(r"<[^>]*>").unwrap();
    let result = tag_re.replace_all(html, "");

    let entity_re = Regex::new(r"&(?:nbsp|amp|lt|gt|quot|#39|#x27);").unwrap();
    entity_re
        .replace_all(&result, |caps: &regex::Captures| {
            match caps.get(0).map(|m| m.as_str()) {
                Some("&nbsp;") => " ",
                Some("&amp;") => "&",
                Some("&lt;") => "<",
                Some("&gt;") => ">",
                Some("&quot;") => "\"",
                Some("&#39;") | Some("&#x27;") => "'",
                _ => "",
            }
        })
        .to_string()
}

