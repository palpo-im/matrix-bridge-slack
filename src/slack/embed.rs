use chrono::{DateTime, Utc};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmbedAuthor {
    pub name: String,
    pub icon_url: Option<String>,
    pub url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmbedField {
    pub name: String,
    pub value: String,
    pub inline: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmbedFooter {
    pub text: String,
    pub icon_url: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SlackEmbed {
    pub title: Option<String>,
    pub description: Option<String>,
    pub url: Option<String>,
    pub timestamp: Option<String>,
    pub color: Option<u32>,
    pub footer: Option<EmbedFooter>,
    pub author: Option<EmbedAuthor>,
    pub fields: Vec<EmbedField>,
    pub image_url: Option<String>,
    pub thumbnail_url: Option<String>,
}

impl SlackEmbed {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    pub fn author(mut self, author: EmbedAuthor) -> Self {
        self.author = Some(author);
        self
    }

    pub fn footer(mut self, footer: EmbedFooter) -> Self {
        self.footer = Some(footer);
        self
    }

    pub fn field(
        mut self,
        name: impl Into<String>,
        value: impl Into<String>,
        inline: bool,
    ) -> Self {
        self.fields.push(EmbedField {
            name: name.into(),
            value: value.into(),
            inline,
        });
        self
    }

    pub fn color(mut self, color: u32) -> Self {
        self.color = Some(color);
        self
    }

    pub fn timestamp(mut self, timestamp: impl Into<String>) -> Self {
        self.timestamp = Some(timestamp.into());
        self
    }

    pub fn image(mut self, url: impl Into<String>) -> Self {
        self.image_url = Some(url.into());
        self
    }

    pub fn thumbnail(mut self, url: impl Into<String>) -> Self {
        self.thumbnail_url = Some(url.into());
        self
    }

    pub fn to_json(&self) -> Value {
        let mut embed = serde_json::Map::new();

        if let Some(ref title) = self.title {
            embed.insert("title".to_string(), Value::String(title.clone()));
        }

        if let Some(ref description) = self.description {
            embed.insert(
                "description".to_string(),
                Value::String(description.clone()),
            );
        }

        if let Some(ref url) = self.url {
            embed.insert("url".to_string(), Value::String(url.clone()));
        }

        if let Some(ref timestamp) = self.timestamp {
            embed.insert("timestamp".to_string(), Value::String(timestamp.clone()));
        }

        if let Some(color) = self.color {
            embed.insert("color".to_string(), Value::Number(color.into()));
        }

        if let Some(ref author) = self.author {
            let mut author_json = serde_json::Map::new();
            author_json.insert("name".to_string(), Value::String(author.name.clone()));
            if let Some(ref icon_url) = author.icon_url {
                author_json.insert("icon_url".to_string(), Value::String(icon_url.clone()));
            }
            if let Some(ref url) = author.url {
                author_json.insert("url".to_string(), Value::String(url.clone()));
            }
            embed.insert("author".to_string(), Value::Object(author_json));
        }

        if let Some(ref footer) = self.footer {
            let mut footer_json = serde_json::Map::new();
            footer_json.insert("text".to_string(), Value::String(footer.text.clone()));
            if let Some(ref icon_url) = footer.icon_url {
                footer_json.insert("icon_url".to_string(), Value::String(icon_url.clone()));
            }
            embed.insert("footer".to_string(), Value::Object(footer_json));
        }

        if !self.fields.is_empty() {
            let fields: Vec<Value> = self
                .fields
                .iter()
                .map(|f| {
                    let mut field = serde_json::Map::new();
                    field.insert("name".to_string(), Value::String(f.name.clone()));
                    field.insert("value".to_string(), Value::String(f.value.clone()));
                    field.insert("inline".to_string(), Value::Bool(f.inline));
                    Value::Object(field)
                })
                .collect();
            embed.insert("fields".to_string(), Value::Array(fields));
        }

        if let Some(ref image_url) = self.image_url {
            let mut image = serde_json::Map::new();
            image.insert("url".to_string(), Value::String(image_url.clone()));
            embed.insert("image".to_string(), Value::Object(image));
        }

        if let Some(ref thumbnail_url) = self.thumbnail_url {
            let mut thumbnail = serde_json::Map::new();
            thumbnail.insert("url".to_string(), Value::String(thumbnail_url.clone()));
            embed.insert("thumbnail".to_string(), Value::Object(thumbnail));
        }

        Value::Object(embed)
    }
}

pub fn build_matrix_message_embed(
    sender_displayname: &str,
    sender_avatar_url: Option<&str>,
    body: &str,
    reply_to: Option<(&str, &str)>,
) -> SlackEmbed {
    let mut embed = SlackEmbed::new().description(body);

    let author = EmbedAuthor {
        name: sender_displayname.to_string(),
        icon_url: sender_avatar_url.map(ToOwned::to_owned),
        url: None,
    };

    embed = embed.author(author);

    if let Some((reply_sender, reply_body)) = reply_to {
        let preview = if reply_body.len() > 100 {
            format!("{}...", &reply_body[..100])
        } else {
            reply_body.to_string()
        };

        embed = embed.field(format!("Replying to {}", reply_sender), preview, false);
    }

    embed
}

pub fn build_reply_embed(
    original_sender: &str,
    original_body: &str,
    original_event_id: Option<&str>,
) -> SlackEmbed {
    let preview = if original_body.len() > 100 {
        format!("{}...", &original_body[..100])
    } else {
        original_body.to_string()
    };

    let mut embed = SlackEmbed::new().description(preview);

    if let Some(_event_id) = original_event_id {
        embed = embed.footer(EmbedFooter {
            text: format!("Reply to {}", original_sender),
            icon_url: None,
        });
    }

    embed.color(0x2D2D2D)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embed_to_json_includes_description() {
        let embed = SlackEmbed::new().description("Hello world");

        let json = embed.to_json();
        assert_eq!(json["description"], "Hello world");
    }

    #[test]
    fn embed_to_json_includes_author() {
        let embed = SlackEmbed::new().author(EmbedAuthor {
            name: "Alice".to_string(),
            icon_url: Some("https://example.com/avatar.png".to_string()),
            url: None,
        });

        let json = embed.to_json();
        assert_eq!(json["author"]["name"], "Alice");
        assert_eq!(json["author"]["icon_url"], "https://example.com/avatar.png");
    }

    #[test]
    fn embed_to_json_includes_fields() {
        let embed = SlackEmbed::new().field("Replying to Bob", "Hello...", false);

        let json = embed.to_json();
        assert_eq!(json["fields"][0]["name"], "Replying to Bob");
        assert_eq!(json["fields"][0]["value"], "Hello...");
        assert_eq!(json["fields"][0]["inline"], false);
    }

    #[test]
    fn build_matrix_message_embed_creates_proper_embed() {
        let embed = build_matrix_message_embed(
            "Alice",
            Some("https://example.com/avatar.png"),
            "Hello world",
            Some(("Bob", "Hi there")),
        );

        assert!(embed.description.is_some());
        assert!(embed.author.is_some());
        assert_eq!(embed.fields.len(), 1);
    }
}
