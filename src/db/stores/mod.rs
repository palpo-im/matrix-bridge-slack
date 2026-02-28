use async_trait::async_trait;

use super::DatabaseError;
use super::models::{
    EmojiMapping, MessageMapping, RemoteRoomInfo, RemoteUserInfo, RoomMapping, UserMapping,
};

#[async_trait]
pub trait RoomStore: Send + Sync {
    async fn get_room_by_slack_channel(
        &self,
        channel_id: &str,
    ) -> Result<Option<RoomMapping>, DatabaseError>;
    async fn get_room_by_matrix_room(
        &self,
        room_id: &str,
    ) -> Result<Option<RoomMapping>, DatabaseError>;
    async fn get_room_by_id(&self, id: i64) -> Result<Option<RoomMapping>, DatabaseError>;
    async fn count_rooms(&self) -> Result<i64, DatabaseError>;
    async fn list_room_mappings(
        &self,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<RoomMapping>, DatabaseError>;
    async fn create_room_mapping(&self, mapping: &RoomMapping) -> Result<(), DatabaseError>;
    async fn update_room_mapping(&self, mapping: &RoomMapping) -> Result<(), DatabaseError>;
    async fn delete_room_mapping(&self, id: i64) -> Result<(), DatabaseError>;
    async fn get_rooms_by_guild(&self, guild_id: &str) -> Result<Vec<RoomMapping>, DatabaseError>;
    async fn get_remote_room_info(
        &self,
        matrix_room_id: &str,
    ) -> Result<Option<RemoteRoomInfo>, DatabaseError>;
    async fn update_remote_room_info(
        &self,
        matrix_room_id: &str,
        info: &RemoteRoomInfo,
    ) -> Result<(), DatabaseError>;
}

#[async_trait]
pub trait UserStore: Send + Sync {
    async fn get_user_by_slack_id(
        &self,
        slack_id: &str,
    ) -> Result<Option<UserMapping>, DatabaseError>;
    async fn get_user_by_matrix_id(
        &self,
        matrix_id: &str,
    ) -> Result<Option<UserMapping>, DatabaseError>;
    async fn create_user_mapping(&self, mapping: &UserMapping) -> Result<(), DatabaseError>;
    async fn update_user_mapping(&self, mapping: &UserMapping) -> Result<(), DatabaseError>;
    async fn delete_user_mapping(&self, id: i64) -> Result<(), DatabaseError>;
    async fn get_remote_user_info(
        &self,
        slack_user_id: &str,
    ) -> Result<Option<RemoteUserInfo>, DatabaseError>;
    async fn update_remote_user_info(
        &self,
        slack_user_id: &str,
        info: &RemoteUserInfo,
    ) -> Result<(), DatabaseError>;
    async fn get_all_user_ids(&self) -> Result<Vec<String>, DatabaseError>;
}

#[async_trait]
pub trait MessageStore: Send + Sync {
    async fn get_by_slack_message_id(
        &self,
        slack_message_id: &str,
    ) -> Result<Option<MessageMapping>, DatabaseError>;
    async fn get_by_matrix_event_id(
        &self,
        matrix_event_id: &str,
    ) -> Result<Option<MessageMapping>, DatabaseError>;
    async fn upsert_message_mapping(&self, mapping: &MessageMapping) -> Result<(), DatabaseError>;
    async fn delete_by_slack_message_id(
        &self,
        slack_message_id: &str,
    ) -> Result<(), DatabaseError>;
    async fn delete_by_matrix_event_id(&self, matrix_event_id: &str) -> Result<(), DatabaseError>;
}

#[async_trait]
pub trait EmojiStore: Send + Sync {
    async fn get_emoji_by_slack_id(
        &self,
        slack_emoji_id: &str,
    ) -> Result<Option<EmojiMapping>, DatabaseError>;
    async fn get_emoji_by_mxc(&self, mxc_url: &str) -> Result<Option<EmojiMapping>, DatabaseError>;
    async fn create_emoji(&self, emoji: &EmojiMapping) -> Result<(), DatabaseError>;
    async fn update_emoji(&self, emoji: &EmojiMapping) -> Result<(), DatabaseError>;
    async fn delete_emoji(&self, slack_emoji_id: &str) -> Result<(), DatabaseError>;
}

