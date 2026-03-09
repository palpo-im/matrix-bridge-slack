use serde_json::Value;
use tracing::debug;

/// Renders Slack Block Kit blocks into Matrix-compatible HTML
pub fn render_blocks(blocks: &[Value]) -> Option<String> {
    if blocks.is_empty() {
        return None;
    }

    let mut parts = Vec::new();

    for block in blocks {
        let block_type = block.get("type").and_then(Value::as_str).unwrap_or("");
        match block_type {
            "rich_text" => {
                if let Some(html) = render_rich_text_block(block) {
                    parts.push(html);
                }
            }
            "header" => {
                if let Some(text) = block.get("text").and_then(|t| t.get("text")).and_then(Value::as_str) {
                    parts.push(format!("<h3>{}</h3>", escape_html(text)));
                }
            }
            "divider" => {
                parts.push("<hr/>".to_string());
            }
            "section" => {
                if let Some(html) = render_section_block(block) {
                    parts.push(html);
                }
            }
            "context" => {
                if let Some(html) = render_context_block(block) {
                    parts.push(html);
                }
            }
            "image" => {
                if let Some(html) = render_image_block(block) {
                    parts.push(html);
                }
            }
            _ => {
                debug!("unsupported block type: {}", block_type);
            }
        }
    }

    if parts.is_empty() {
        None
    } else {
        Some(parts.join("\n"))
    }
}

/// Renders plain text from blocks (for the body field)
pub fn render_blocks_plain(blocks: &[Value]) -> Option<String> {
    if blocks.is_empty() {
        return None;
    }

    let mut parts = Vec::new();

    for block in blocks {
        let block_type = block.get("type").and_then(Value::as_str).unwrap_or("");
        match block_type {
            "rich_text" => {
                if let Some(text) = render_rich_text_block_plain(block) {
                    parts.push(text);
                }
            }
            "header" => {
                if let Some(text) = block.get("text").and_then(|t| t.get("text")).and_then(Value::as_str) {
                    parts.push(text.to_string());
                }
            }
            "divider" => {
                parts.push("---".to_string());
            }
            "section" => {
                if let Some(text) = render_section_block_plain(block) {
                    parts.push(text);
                }
            }
            "context" => {
                if let Some(text) = render_context_block_plain(block) {
                    parts.push(text);
                }
            }
            "image" => {
                let alt = block.get("alt_text").and_then(Value::as_str).unwrap_or("image");
                let url = block.get("image_url").and_then(Value::as_str).unwrap_or("");
                parts.push(format!("[{}]({})", alt, url));
            }
            _ => {}
        }
    }

    if parts.is_empty() {
        None
    } else {
        Some(parts.join("\n"))
    }
}

fn render_rich_text_block(block: &Value) -> Option<String> {
    let elements = block.get("elements")?.as_array()?;
    let mut parts = Vec::new();

    for element in elements {
        let element_type = element.get("type").and_then(Value::as_str).unwrap_or("");
        match element_type {
            "rich_text_section" => {
                if let Some(html) = render_rich_text_section(element) {
                    parts.push(html);
                }
            }
            "rich_text_preformatted" => {
                if let Some(html) = render_rich_text_preformatted(element) {
                    parts.push(html);
                }
            }
            "rich_text_quote" => {
                if let Some(html) = render_rich_text_quote(element) {
                    parts.push(html);
                }
            }
            "rich_text_list" => {
                if let Some(html) = render_rich_text_list(element) {
                    parts.push(html);
                }
            }
            _ => {
                debug!("unsupported rich text element type: {}", element_type);
            }
        }
    }

    if parts.is_empty() {
        None
    } else {
        Some(parts.join(""))
    }
}

fn render_rich_text_block_plain(block: &Value) -> Option<String> {
    let elements = block.get("elements")?.as_array()?;
    let mut parts = Vec::new();

    for element in elements {
        let element_type = element.get("type").and_then(Value::as_str).unwrap_or("");
        match element_type {
            "rich_text_section" => {
                if let Some(text) = render_rich_text_section_plain(element) {
                    parts.push(text);
                }
            }
            "rich_text_preformatted" => {
                if let Some(inner) = element.get("elements").and_then(Value::as_array) {
                    let text: String = inner.iter()
                        .filter_map(|e| e.get("text").and_then(Value::as_str))
                        .collect::<Vec<_>>()
                        .join("");
                    if !text.is_empty() {
                        parts.push(format!("```\n{}\n```", text));
                    }
                }
            }
            "rich_text_quote" => {
                if let Some(inner) = element.get("elements").and_then(Value::as_array) {
                    let text: String = inner.iter()
                        .filter_map(|e| e.get("text").and_then(Value::as_str))
                        .collect::<Vec<_>>()
                        .join("");
                    if !text.is_empty() {
                        let quoted: String = text.lines()
                            .map(|line| format!("> {}", line))
                            .collect::<Vec<_>>()
                            .join("\n");
                        parts.push(quoted);
                    }
                }
            }
            "rich_text_list" => {
                if let Some(text) = render_rich_text_list_plain(element) {
                    parts.push(text);
                }
            }
            _ => {}
        }
    }

    if parts.is_empty() {
        None
    } else {
        Some(parts.join("\n"))
    }
}

fn render_rich_text_section(element: &Value) -> Option<String> {
    let inner_elements = element.get("elements")?.as_array()?;
    let mut html = String::new();

    for elem in inner_elements {
        html.push_str(&render_rich_text_element(elem));
    }

    if html.is_empty() {
        None
    } else {
        Some(html)
    }
}

fn render_rich_text_section_plain(element: &Value) -> Option<String> {
    let inner_elements = element.get("elements")?.as_array()?;
    let mut text = String::new();

    for elem in inner_elements {
        text.push_str(&render_rich_text_element_plain(elem));
    }

    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

fn render_rich_text_element(elem: &Value) -> String {
    let elem_type = elem.get("type").and_then(Value::as_str).unwrap_or("");
    match elem_type {
        "text" => {
            let text = elem.get("text").and_then(Value::as_str).unwrap_or("");
            let escaped = escape_html(text);
            apply_text_styles(elem, &escaped)
        }
        "user" => {
            let user_id = elem.get("user_id").and_then(Value::as_str).unwrap_or("");
            format!("<a href=\"https://matrix.to/#/@_slack_{}:\">@{}</a>", user_id, user_id)
        }
        "channel" => {
            let channel_id = elem.get("channel_id").and_then(Value::as_str).unwrap_or("");
            format!("<a href=\"https://matrix.to/#/#_slack_{}:\">#channel</a>", channel_id)
        }
        "link" => {
            let url = elem.get("url").and_then(Value::as_str).unwrap_or("");
            let text = elem.get("text").and_then(Value::as_str);
            let display = text.unwrap_or(url);
            let styled = apply_text_styles(elem, &escape_html(display));
            format!("<a href=\"{}\">{}</a>", escape_html(url), styled)
        }
        "emoji" => {
            let name = elem.get("name").and_then(Value::as_str).unwrap_or("");
            let unicode = elem.get("unicode").and_then(Value::as_str);
            if let Some(unicode) = unicode {
                decode_unicode_emoji(unicode)
            } else {
                format!(":{}:", name)
            }
        }
        "broadcast" => {
            let range = elem.get("range").and_then(Value::as_str).unwrap_or("everyone");
            format!("<font color=\"#FF0000\">@{}</font>", range)
        }
        "color" => {
            let value = elem.get("value").and_then(Value::as_str).unwrap_or("#000000");
            format!("<font color=\"{}\">■</font> {}", value, value)
        }
        "date" => {
            let timestamp = elem.get("timestamp").and_then(Value::as_i64).unwrap_or(0);
            let fallback = elem.get("fallback").and_then(Value::as_str).unwrap_or("");
            if fallback.is_empty() {
                format!("<time datetime=\"{}\">{}</time>", timestamp, timestamp)
            } else {
                escape_html(fallback)
            }
        }
        "usergroup" => {
            let usergroup_id = elem.get("usergroup_id").and_then(Value::as_str).unwrap_or("");
            format!("@group_{}", usergroup_id)
        }
        _ => {
            debug!("unsupported rich text element type: {}", elem_type);
            String::new()
        }
    }
}

fn render_rich_text_element_plain(elem: &Value) -> String {
    let elem_type = elem.get("type").and_then(Value::as_str).unwrap_or("");
    match elem_type {
        "text" => elem.get("text").and_then(Value::as_str).unwrap_or("").to_string(),
        "user" => {
            let user_id = elem.get("user_id").and_then(Value::as_str).unwrap_or("");
            format!("@{}", user_id)
        }
        "channel" => {
            let channel_id = elem.get("channel_id").and_then(Value::as_str).unwrap_or("");
            format!("#{}", channel_id)
        }
        "link" => {
            let url = elem.get("url").and_then(Value::as_str).unwrap_or("");
            let text = elem.get("text").and_then(Value::as_str);
            if let Some(text) = text {
                format!("{} ({})", text, url)
            } else {
                url.to_string()
            }
        }
        "emoji" => {
            let name = elem.get("name").and_then(Value::as_str).unwrap_or("");
            let unicode = elem.get("unicode").and_then(Value::as_str);
            if let Some(unicode) = unicode {
                decode_unicode_emoji(unicode)
            } else {
                format!(":{}:", name)
            }
        }
        "broadcast" => {
            let range = elem.get("range").and_then(Value::as_str).unwrap_or("everyone");
            format!("@{}", range)
        }
        _ => String::new(),
    }
}

fn apply_text_styles(elem: &Value, text: &str) -> String {
    let style = elem.get("style");
    let mut result = text.to_string();

    if let Some(style) = style.and_then(Value::as_object) {
        if style.get("code").and_then(Value::as_bool).unwrap_or(false) {
            result = format!("<code>{}</code>", result);
        }
        if style.get("bold").and_then(Value::as_bool).unwrap_or(false) {
            result = format!("<strong>{}</strong>", result);
        }
        if style.get("italic").and_then(Value::as_bool).unwrap_or(false) {
            result = format!("<em>{}</em>", result);
        }
        if style.get("strike").and_then(Value::as_bool).unwrap_or(false) {
            result = format!("<del>{}</del>", result);
        }
    }

    result
}

fn render_rich_text_preformatted(element: &Value) -> Option<String> {
    let inner = element.get("elements")?.as_array()?;
    let mut code = String::new();

    for elem in inner {
        if elem.get("type").and_then(Value::as_str) == Some("text") {
            if let Some(text) = elem.get("text").and_then(Value::as_str) {
                code.push_str(&escape_html(text));
            }
        }
    }

    if code.is_empty() {
        None
    } else {
        Some(format!("<pre><code>{}</code></pre>", code))
    }
}

fn render_rich_text_quote(element: &Value) -> Option<String> {
    let inner = element.get("elements")?.as_array()?;
    let mut content = String::new();

    for elem in inner {
        content.push_str(&render_rich_text_element(elem));
    }

    if content.is_empty() {
        None
    } else {
        Some(format!("<blockquote>{}</blockquote>", content))
    }
}

fn render_rich_text_list(element: &Value) -> Option<String> {
    let items = element.get("elements")?.as_array()?;
    let style = element.get("style").and_then(Value::as_str).unwrap_or("bullet");
    let offset = element.get("offset").and_then(Value::as_u64).unwrap_or(0);

    let tag = if style == "ordered" { "ol" } else { "ul" };
    let mut html = if style == "ordered" && offset > 0 {
        format!("<{} start=\"{}\">", tag, offset + 1)
    } else {
        format!("<{}>", tag)
    };

    for item in items {
        if let Some(section_html) = render_rich_text_section(item) {
            html.push_str(&format!("<li>{}</li>", section_html));
        }
    }

    html.push_str(&format!("</{}>", tag));
    Some(html)
}

fn render_rich_text_list_plain(element: &Value) -> Option<String> {
    let items = element.get("elements")?.as_array()?;
    let style = element.get("style").and_then(Value::as_str).unwrap_or("bullet");
    let offset = element.get("offset").and_then(Value::as_u64).unwrap_or(0);

    let mut lines = Vec::new();

    for (i, item) in items.iter().enumerate() {
        if let Some(text) = render_rich_text_section_plain(item) {
            if style == "ordered" {
                lines.push(format!("{}. {}", offset + i as u64 + 1, text));
            } else {
                lines.push(format!("• {}", text));
            }
        }
    }

    if lines.is_empty() {
        None
    } else {
        Some(lines.join("\n"))
    }
}

fn render_section_block(block: &Value) -> Option<String> {
    let mut parts = Vec::new();

    if let Some(text_obj) = block.get("text") {
        let text = text_obj.get("text").and_then(Value::as_str).unwrap_or("");
        if !text.is_empty() {
            parts.push(escape_html(text));
        }
    }

    if let Some(fields) = block.get("fields").and_then(Value::as_array) {
        let mut table = String::from("<table>");
        let mut row_open = false;
        for (i, field) in fields.iter().enumerate() {
            if i % 2 == 0 {
                if row_open {
                    table.push_str("</tr>");
                }
                table.push_str("<tr>");
                row_open = true;
            }
            let field_text = field.get("text").and_then(Value::as_str).unwrap_or("");
            table.push_str(&format!("<td>{}</td>", escape_html(field_text)));
        }
        if row_open {
            table.push_str("</tr>");
        }
        table.push_str("</table>");
        parts.push(table);
    }

    if parts.is_empty() {
        None
    } else {
        Some(parts.join("\n"))
    }
}

fn render_section_block_plain(block: &Value) -> Option<String> {
    let mut parts = Vec::new();

    if let Some(text_obj) = block.get("text") {
        let text = text_obj.get("text").and_then(Value::as_str).unwrap_or("");
        if !text.is_empty() {
            parts.push(text.to_string());
        }
    }

    if let Some(fields) = block.get("fields").and_then(Value::as_array) {
        for field in fields {
            let field_text = field.get("text").and_then(Value::as_str).unwrap_or("");
            if !field_text.is_empty() {
                parts.push(field_text.to_string());
            }
        }
    }

    if parts.is_empty() {
        None
    } else {
        Some(parts.join("\n"))
    }
}

fn render_context_block(block: &Value) -> Option<String> {
    let elements = block.get("elements")?.as_array()?;
    let mut parts = Vec::new();

    for elem in elements {
        let elem_type = elem.get("type").and_then(Value::as_str).unwrap_or("");
        match elem_type {
            "mrkdwn" | "plain_text" => {
                let text = elem.get("text").and_then(Value::as_str).unwrap_or("");
                if !text.is_empty() {
                    parts.push(format!("<em>{}</em>", escape_html(text)));
                }
            }
            "image" => {
                let url = elem.get("image_url").and_then(Value::as_str).unwrap_or("");
                let alt = elem.get("alt_text").and_then(Value::as_str).unwrap_or("");
                if !url.is_empty() {
                    parts.push(format!("<img src=\"{}\" alt=\"{}\" height=\"16\" width=\"16\" />",
                        escape_html(url), escape_html(alt)));
                }
            }
            _ => {}
        }
    }

    if parts.is_empty() {
        None
    } else {
        Some(format!("<p>{}</p>", parts.join(" | ")))
    }
}

fn render_context_block_plain(block: &Value) -> Option<String> {
    let elements = block.get("elements")?.as_array()?;
    let mut parts = Vec::new();

    for elem in elements {
        let text = elem.get("text").and_then(Value::as_str).unwrap_or("");
        if !text.is_empty() {
            parts.push(text.to_string());
        }
    }

    if parts.is_empty() {
        None
    } else {
        Some(parts.join(" | "))
    }
}

fn render_image_block(block: &Value) -> Option<String> {
    let url = block.get("image_url").and_then(Value::as_str)?;
    let alt = block.get("alt_text").and_then(Value::as_str).unwrap_or("image");
    let title = block.get("title")
        .and_then(|t| t.get("text"))
        .and_then(Value::as_str);

    let mut html = format!("<img src=\"{}\" alt=\"{}\" />", escape_html(url), escape_html(alt));
    if let Some(title) = title {
        html = format!("<figure>{}<figcaption>{}</figcaption></figure>", html, escape_html(title));
    }
    Some(html)
}

/// Renders Slack message attachments into Matrix-compatible HTML
pub fn render_attachments(attachments: &[Value]) -> Option<String> {
    if attachments.is_empty() {
        return None;
    }

    let mut parts = Vec::new();

    for attachment in attachments {
        if let Some(html) = render_single_attachment(attachment) {
            parts.push(html);
        }
    }

    if parts.is_empty() {
        None
    } else {
        Some(parts.join("\n"))
    }
}

fn render_single_attachment(attachment: &Value) -> Option<String> {
    let is_msg_unfurl = attachment.get("is_msg_unfurl").and_then(Value::as_bool).unwrap_or(false);
    if is_msg_unfurl {
        return None;
    }

    let mut parts = Vec::new();

    // Author
    if let Some(author_name) = attachment.get("author_name").and_then(Value::as_str) {
        let author_link = attachment.get("author_link").and_then(Value::as_str);
        if let Some(link) = author_link {
            parts.push(format!("<strong><a href=\"{}\">{}</a></strong>", escape_html(link), escape_html(author_name)));
        } else {
            parts.push(format!("<strong>{}</strong>", escape_html(author_name)));
        }
    }

    // Title
    if let Some(title) = attachment.get("title").and_then(Value::as_str) {
        let title_link = attachment.get("title_link").and_then(Value::as_str);
        if let Some(link) = title_link {
            parts.push(format!("<strong><a href=\"{}\">{}</a></strong>", escape_html(link), escape_html(title)));
        } else {
            parts.push(format!("<strong>{}</strong>", escape_html(title)));
        }
    }

    // Pretext
    if let Some(pretext) = attachment.get("pretext").and_then(Value::as_str) {
        if !pretext.is_empty() {
            parts.push(escape_html(pretext));
        }
    }

    // Text/Fallback
    if let Some(text) = attachment.get("text").and_then(Value::as_str) {
        if !text.is_empty() {
            parts.push(escape_html(text));
        }
    } else if let Some(fallback) = attachment.get("fallback").and_then(Value::as_str) {
        if !fallback.is_empty() {
            parts.push(escape_html(fallback));
        }
    }

    // Fields
    if let Some(fields) = attachment.get("fields").and_then(Value::as_array) {
        for field in fields {
            let title = field.get("title").and_then(Value::as_str).unwrap_or("");
            let value = field.get("value").and_then(Value::as_str).unwrap_or("");
            if !title.is_empty() || !value.is_empty() {
                parts.push(format!("<strong>{}</strong>: {}", escape_html(title), escape_html(value)));
            }
        }
    }

    // Image URL
    if let Some(image_url) = attachment.get("image_url").and_then(Value::as_str) {
        parts.push(format!("<img src=\"{}\" alt=\"attachment\" />", escape_html(image_url)));
    }

    // Footer
    if let Some(footer) = attachment.get("footer").and_then(Value::as_str) {
        if !footer.is_empty() {
            parts.push(format!("<em>{}</em>", escape_html(footer)));
        }
    }

    if parts.is_empty() {
        None
    } else {
        Some(format!("<blockquote>{}</blockquote>", parts.join("<br/>")))
    }
}

fn decode_unicode_emoji(unicode_str: &str) -> String {
    unicode_str
        .split('-')
        .filter_map(|code| {
            u32::from_str_radix(code, 16)
                .ok()
                .and_then(char::from_u32)
        })
        .collect()
}

fn escape_html(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_render_header_block() {
        let blocks = vec![json!({
            "type": "header",
            "text": {"type": "plain_text", "text": "Hello World"}
        })];
        let result = render_blocks(&blocks).unwrap();
        assert!(result.contains("<h3>Hello World</h3>"));
    }

    #[test]
    fn test_render_divider_block() {
        let blocks = vec![json!({"type": "divider"})];
        let result = render_blocks(&blocks).unwrap();
        assert!(result.contains("<hr/>"));
    }

    #[test]
    fn test_render_rich_text_with_styles() {
        let blocks = vec![json!({
            "type": "rich_text",
            "elements": [{
                "type": "rich_text_section",
                "elements": [{
                    "type": "text",
                    "text": "bold text",
                    "style": {"bold": true}
                }]
            }]
        })];
        let result = render_blocks(&blocks).unwrap();
        assert!(result.contains("<strong>bold text</strong>"));
    }

    #[test]
    fn test_render_rich_text_list() {
        let blocks = vec![json!({
            "type": "rich_text",
            "elements": [{
                "type": "rich_text_list",
                "style": "ordered",
                "elements": [
                    {"type": "rich_text_section", "elements": [{"type": "text", "text": "First"}]},
                    {"type": "rich_text_section", "elements": [{"type": "text", "text": "Second"}]}
                ]
            }]
        })];
        let result = render_blocks(&blocks).unwrap();
        assert!(result.contains("<ol>"));
        assert!(result.contains("<li>First</li>"));
        assert!(result.contains("<li>Second</li>"));
    }

    #[test]
    fn test_render_code_block() {
        let blocks = vec![json!({
            "type": "rich_text",
            "elements": [{
                "type": "rich_text_preformatted",
                "elements": [{"type": "text", "text": "let x = 1;"}]
            }]
        })];
        let result = render_blocks(&blocks).unwrap();
        assert!(result.contains("<pre><code>"));
        assert!(result.contains("let x = 1;"));
    }

    #[test]
    fn test_render_blockquote() {
        let blocks = vec![json!({
            "type": "rich_text",
            "elements": [{
                "type": "rich_text_quote",
                "elements": [{"type": "text", "text": "quoted text"}]
            }]
        })];
        let result = render_blocks(&blocks).unwrap();
        assert!(result.contains("<blockquote>quoted text</blockquote>"));
    }

    #[test]
    fn test_render_user_mention() {
        let blocks = vec![json!({
            "type": "rich_text",
            "elements": [{
                "type": "rich_text_section",
                "elements": [{"type": "user", "user_id": "U12345"}]
            }]
        })];
        let result = render_blocks(&blocks).unwrap();
        assert!(result.contains("@_slack_U12345"));
    }

    #[test]
    fn test_render_emoji() {
        let blocks = vec![json!({
            "type": "rich_text",
            "elements": [{
                "type": "rich_text_section",
                "elements": [{"type": "emoji", "name": "wave", "unicode": "1f44b"}]
            }]
        })];
        let result = render_blocks(&blocks).unwrap();
        assert!(result.contains("\u{1f44b}"));
    }

    #[test]
    fn test_render_attachment() {
        let attachments = vec![json!({
            "author_name": "Bot",
            "title": "Alert",
            "text": "Something happened",
            "footer": "v1.0"
        })];
        let result = render_attachments(&attachments).unwrap();
        assert!(result.contains("Bot"));
        assert!(result.contains("Alert"));
        assert!(result.contains("Something happened"));
    }

    #[test]
    fn test_decode_unicode_emoji() {
        assert_eq!(decode_unicode_emoji("1f44b"), "\u{1f44b}");
        assert_eq!(decode_unicode_emoji("1f1fa-1f1f8"), "\u{1f1fa}\u{1f1f8}");
    }
}
