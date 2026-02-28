use std::sync::Arc;

use anyhow::Result;
use tracing::{debug, info, warn};

use crate::db::{DatabaseManager, EmojiMapping};
use crate::media::MediaHandler;

pub struct EmojiHandler {
    db: Arc<DatabaseManager>,
    media_handler: Arc<MediaHandler>,
    homeserver_url: String,
}

impl EmojiHandler {
    pub fn new(
        db: Arc<DatabaseManager>,
        media_handler: Arc<MediaHandler>,
        homeserver_url: String,
    ) -> Self {
        Self {
            db,
            media_handler,
            homeserver_url,
        }
    }

    pub async fn get_or_upload_emoji(
        &self,
        emoji_id: &str,
        emoji_name: &str,
        animated: bool,
    ) -> Result<String> {
        if !emoji_id.chars().all(|c| c.is_ascii_digit()) {
            return Err(anyhow::anyhow!("Non-numerical emoji ID: {}", emoji_id));
        }

        if let Some(cached) = self
            .db
            .emoji_store()
            .get_emoji_by_slack_id(emoji_id)
            .await?
        {
            debug!("Emoji cache hit for {} ({})", emoji_name, emoji_id);
            return Ok(cached.mxc_url);
        }

        let ext = if animated { "gif" } else { "png" };
        let url = format!("https://cdn.slackapp.com/emojis/{}.{}", emoji_id, ext);

        info!("Downloading emoji {} from {}", emoji_name, url);

        let media = self.media_handler.download_from_url(&url).await?;

        let content_type = if animated { "image/gif" } else { "image/png" };

        let mxc_url = self
            .upload_to_matrix(&media.data, content_type, &media.filename)
            .await?;

        let emoji = EmojiMapping::new(
            emoji_id.to_string(),
            emoji_name.to_string(),
            animated,
            mxc_url.clone(),
        );

        self.db.emoji_store().create_emoji(&emoji).await?;
        info!("Cached emoji {} ({}) as {}", emoji_name, emoji_id, mxc_url);

        Ok(mxc_url)
    }

    async fn upload_to_matrix(
        &self,
        data: &[u8],
        content_type: &str,
        filename: &str,
    ) -> Result<String> {
        let upload_url = format!(
            "{}/_matrix/media/v3/upload?filename={}",
            self.homeserver_url.trim_end_matches('/'),
            urlencoding::encode(filename)
        );

        let client = reqwest::Client::new();
        let response = client
            .post(&upload_url)
            .header("Content-Type", content_type)
            .body(data.to_vec())
            .send()
            .await?;

        if !response.status().is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!("Failed to upload emoji: {}", body));
        }

        let json: serde_json::Value = response.json().await?;
        let content_uri = json
            .get("content_uri")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("No content_uri in response"))?
            .to_string();

        Ok(content_uri)
    }

    pub async fn get_emoji_mxc(&self, emoji_id: &str) -> Result<Option<String>> {
        Ok(self
            .db
            .emoji_store()
            .get_emoji_by_slack_id(emoji_id)
            .await?
            .map(|e| e.mxc_url))
    }

    pub async fn delete_emoji(&self, emoji_id: &str) -> Result<()> {
        self.db.emoji_store().delete_emoji(emoji_id).await?;
        info!("Deleted emoji cache for {}", emoji_id);
        Ok(())
    }

    pub fn emoji_to_matrix_html(&self, mxc_url: &str, emoji_name: &str) -> String {
        format!(
            r#"<img data-mx-emoticon src="{}" alt=":{}:" title=":{}:" height="32" width="32" />"#,
            mxc_url, emoji_name, emoji_name
        )
    }

    pub fn emoji_to_matrix_plain(&self, emoji_name: &str) -> String {
        format!(":{}:", emoji_name)
    }
}

mod urlencoding {
    pub fn encode(s: &str) -> String {
        url::form_urlencoded::byte_serialize(s.as_bytes()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emoji_to_matrix_html_creates_correct_format() {
        let handler = EmojiHandler::new(
            Arc::new(crate::db::DatabaseManager::new_in_memory().unwrap()),
            Arc::new(crate::media::MediaHandler::new("http://localhost:8008")),
            "http://localhost:8008".to_string(),
        );

        let html = handler.emoji_to_matrix_html("mxc://example.org/abc123", "smile");
        assert!(html.contains("mxc://example.org/abc123"));
        assert!(html.contains(":smile:"));
        assert!(html.contains("data-mx-emoticon"));
    }

    #[test]
    fn emoji_to_matrix_plain_creates_correct_format() {
        let handler = EmojiHandler::new(
            Arc::new(crate::db::DatabaseManager::new_in_memory().unwrap()),
            Arc::new(crate::media::MediaHandler::new("http://localhost:8008")),
            "http://localhost:8008".to_string(),
        );

        let plain = handler.emoji_to_matrix_plain("smile");
        assert_eq!(plain, ":smile:");
    }
}

