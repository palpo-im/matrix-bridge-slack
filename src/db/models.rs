use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoomMapping {
    pub id: i64,
    pub matrix_room_id: String,
    pub slack_channel_id: String,
    pub slack_channel_name: String,
    pub slack_team_id: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserMapping {
    pub id: i64,
    pub matrix_user_id: String,
    pub slack_user_id: String,
    pub slack_username: String,
    pub slack_discriminator: String,
    pub slack_avatar: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessedEvent {
    pub id: i64,
    pub event_id: String,
    pub event_type: String,
    pub source: String,
    pub processed_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageMapping {
    pub id: i64,
    pub slack_message_id: String,
    pub matrix_room_id: String,
    pub matrix_event_id: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmojiMapping {
    pub id: i64,
    pub slack_emoji_id: String,
    pub emoji_name: String,
    pub animated: bool,
    pub mxc_url: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl EmojiMapping {
    pub fn new(
        slack_emoji_id: String,
        emoji_name: String,
        animated: bool,
        mxc_url: String,
    ) -> Self {
        let now = Utc::now();
        Self {
            id: 0,
            slack_emoji_id,
            emoji_name,
            animated,
            mxc_url,
            created_at: now,
            updated_at: now,
        }
    }

    pub fn slack_url(&self) -> String {
        let ext = if self.animated { "gif" } else { "png" };
        format!(
            "https://cdn.slackapp.com/emojis/{}.{}",
            self.slack_emoji_id, ext
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteRoomInfo {
    pub slack_team_id: String,
    pub slack_channel_id: String,
    pub slack_name: Option<String>,
    pub slack_topic: Option<String>,
    pub slack_icon_url: Option<String>,
    pub plumbed: bool,
    pub update_name: bool,
    pub update_topic: bool,
    pub update_icon: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteUserInfo {
    pub slack_user_id: String,
    pub displayname: Option<String>,
    pub avatar_url: Option<String>,
    pub avatar_mxc: Option<String>,
    pub guild_nicks: std::collections::HashMap<String, String>,
}
