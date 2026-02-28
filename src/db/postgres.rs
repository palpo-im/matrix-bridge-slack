use async_trait::async_trait;
use chrono::{DateTime, Utc};
use diesel::pg::PgConnection;
use diesel::prelude::*;

use super::DatabaseError;
use super::models::{
    EmojiMapping, MessageMapping, RemoteRoomInfo, RemoteUserInfo, RoomMapping, UserMapping,
};
use crate::db::manager::Pool;
use crate::db::schema::{message_mappings, room_mappings, user_mappings};

#[derive(Debug, Clone, Queryable, Selectable)]
#[diesel(table_name = room_mappings)]
struct DbRoomMapping {
    id: i64,
    matrix_room_id: String,
    slack_channel_id: String,
    slack_channel_name: String,
    slack_team_id: String,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl From<DbRoomMapping> for RoomMapping {
    fn from(value: DbRoomMapping) -> Self {
        Self {
            id: value.id,
            matrix_room_id: value.matrix_room_id,
            slack_channel_id: value.slack_channel_id,
            slack_channel_name: value.slack_channel_name,
            slack_team_id: value.slack_team_id,
            created_at: value.created_at,
            updated_at: value.updated_at,
        }
    }
}

#[derive(Insertable)]
#[diesel(table_name = room_mappings)]
struct NewRoomMapping<'a> {
    matrix_room_id: &'a str,
    slack_channel_id: &'a str,
    slack_channel_name: &'a str,
    slack_team_id: &'a str,
    created_at: &'a DateTime<Utc>,
    updated_at: &'a DateTime<Utc>,
}

#[derive(AsChangeset)]
#[diesel(table_name = room_mappings)]
struct UpdateRoomMapping<'a> {
    matrix_room_id: &'a str,
    slack_channel_id: &'a str,
    slack_channel_name: &'a str,
    slack_team_id: &'a str,
    updated_at: &'a DateTime<Utc>,
}

#[derive(Debug, Clone, Queryable, Selectable)]
#[diesel(table_name = user_mappings)]
struct DbUserMapping {
    id: i64,
    matrix_user_id: String,
    slack_user_id: String,
    slack_username: String,
    slack_discriminator: String,
    slack_avatar: Option<String>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl From<DbUserMapping> for UserMapping {
    fn from(value: DbUserMapping) -> Self {
        Self {
            id: value.id,
            matrix_user_id: value.matrix_user_id,
            slack_user_id: value.slack_user_id,
            slack_username: value.slack_username,
            slack_discriminator: value.slack_discriminator,
            slack_avatar: value.slack_avatar,
            created_at: value.created_at,
            updated_at: value.updated_at,
        }
    }
}

#[derive(Insertable)]
#[diesel(table_name = user_mappings)]
struct NewUserMapping<'a> {
    matrix_user_id: &'a str,
    slack_user_id: &'a str,
    slack_username: &'a str,
    slack_discriminator: &'a str,
    slack_avatar: Option<&'a str>,
    created_at: &'a DateTime<Utc>,
    updated_at: &'a DateTime<Utc>,
}

#[derive(AsChangeset)]
#[diesel(table_name = user_mappings)]
struct UpdateUserMapping<'a> {
    slack_username: &'a str,
    slack_discriminator: &'a str,
    slack_avatar: Option<&'a str>,
    updated_at: &'a DateTime<Utc>,
}

#[derive(Debug, Clone, Queryable, Selectable)]
#[diesel(table_name = message_mappings)]
struct DbMessageMapping {
    id: i64,
    slack_message_id: String,
    matrix_room_id: String,
    matrix_event_id: String,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl From<DbMessageMapping> for MessageMapping {
    fn from(value: DbMessageMapping) -> Self {
        Self {
            id: value.id,
            slack_message_id: value.slack_message_id,
            matrix_room_id: value.matrix_room_id,
            matrix_event_id: value.matrix_event_id,
            created_at: value.created_at,
            updated_at: value.updated_at,
        }
    }
}

#[derive(Insertable)]
#[diesel(table_name = message_mappings)]
struct NewMessageMapping<'a> {
    slack_message_id: &'a str,
    matrix_room_id: &'a str,
    matrix_event_id: &'a str,
    created_at: &'a DateTime<Utc>,
    updated_at: &'a DateTime<Utc>,
}

#[derive(AsChangeset)]
#[diesel(table_name = message_mappings)]
struct UpdateMessageMapping<'a> {
    matrix_room_id: &'a str,
    matrix_event_id: &'a str,
    updated_at: &'a DateTime<Utc>,
}

async fn with_connection<T, F>(pool: Pool, operation: F) -> Result<T, DatabaseError>
where
    T: Send + 'static,
    F: FnOnce(&mut PgConnection) -> Result<T, DatabaseError> + Send + 'static,
{
    tokio::task::spawn_blocking(move || {
        let mut conn = pool
            .get()
            .map_err(|e| DatabaseError::Connection(e.to_string()))?;
        operation(&mut conn)
    })
    .await
    .map_err(|e| DatabaseError::Query(format!("database task failed: {e}")))?
}

pub struct PostgresRoomStore {
    pool: Pool,
}

impl PostgresRoomStore {
    pub fn new(pool: Pool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl super::RoomStore for PostgresRoomStore {
    async fn get_room_by_slack_channel(
        &self,
        channel_id: &str,
    ) -> Result<Option<RoomMapping>, DatabaseError> {
        let pool = self.pool.clone();
        let channel_id = channel_id.to_string();
        with_connection(pool, move |conn| {
            use crate::db::schema::room_mappings::dsl::*;
            room_mappings
                .filter(slack_channel_id.eq(channel_id))
                .select(DbRoomMapping::as_select())
                .first::<DbRoomMapping>(conn)
                .optional()
                .map(|value| value.map(Into::into))
                .map_err(|e| DatabaseError::Query(e.to_string()))
        })
        .await
    }

    async fn get_room_by_matrix_room(
        &self,
        room_id: &str,
    ) -> Result<Option<RoomMapping>, DatabaseError> {
        let pool = self.pool.clone();
        let room_id = room_id.to_string();
        with_connection(pool, move |conn| {
            use crate::db::schema::room_mappings::dsl::*;
            room_mappings
                .filter(matrix_room_id.eq(room_id))
                .select(DbRoomMapping::as_select())
                .first::<DbRoomMapping>(conn)
                .optional()
                .map(|value| value.map(Into::into))
                .map_err(|e| DatabaseError::Query(e.to_string()))
        })
        .await
    }

    async fn get_room_by_id(&self, mapping_id: i64) -> Result<Option<RoomMapping>, DatabaseError> {
        let pool = self.pool.clone();
        with_connection(pool, move |conn| {
            use crate::db::schema::room_mappings::dsl::*;
            room_mappings
                .filter(id.eq(mapping_id))
                .select(DbRoomMapping::as_select())
                .first::<DbRoomMapping>(conn)
                .optional()
                .map(|value| value.map(Into::into))
                .map_err(|e| DatabaseError::Query(e.to_string()))
        })
        .await
    }

    async fn count_rooms(&self) -> Result<i64, DatabaseError> {
        let pool = self.pool.clone();
        with_connection(pool, move |conn| {
            use crate::db::schema::room_mappings::dsl::*;
            room_mappings
                .count()
                .get_result(conn)
                .map_err(|e| DatabaseError::Query(e.to_string()))
        })
        .await
    }

    async fn list_room_mappings(
        &self,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<RoomMapping>, DatabaseError> {
        let pool = self.pool.clone();
        with_connection(pool, move |conn| {
            use crate::db::schema::room_mappings::dsl::*;
            room_mappings
                .order(id.desc())
                .limit(limit)
                .offset(offset)
                .select(DbRoomMapping::as_select())
                .load::<DbRoomMapping>(conn)
                .map(|rows| rows.into_iter().map(Into::into).collect())
                .map_err(|e| DatabaseError::Query(e.to_string()))
        })
        .await
    }

    async fn create_room_mapping(&self, mapping: &RoomMapping) -> Result<(), DatabaseError> {
        let pool = self.pool.clone();
        let mapping = mapping.clone();
        with_connection(pool, move |conn| {
            let new_mapping = NewRoomMapping {
                matrix_room_id: &mapping.matrix_room_id,
                slack_channel_id: &mapping.slack_channel_id,
                slack_channel_name: &mapping.slack_channel_name,
                slack_team_id: &mapping.slack_team_id,
                created_at: &mapping.created_at,
                updated_at: &mapping.updated_at,
            };

            diesel::insert_into(room_mappings::table)
                .values(&new_mapping)
                .execute(conn)
                .map(|_| ())
                .map_err(|e| DatabaseError::Query(e.to_string()))
        })
        .await
    }

    async fn update_room_mapping(&self, mapping: &RoomMapping) -> Result<(), DatabaseError> {
        let pool = self.pool.clone();
        let mapping = mapping.clone();
        with_connection(pool, move |conn| {
            let changes = UpdateRoomMapping {
                matrix_room_id: &mapping.matrix_room_id,
                slack_channel_id: &mapping.slack_channel_id,
                slack_channel_name: &mapping.slack_channel_name,
                slack_team_id: &mapping.slack_team_id,
                updated_at: &mapping.updated_at,
            };

            diesel::update(room_mappings::table.filter(room_mappings::id.eq(mapping.id)))
                .set(changes)
                .execute(conn)
                .map(|_| ())
                .map_err(|e| DatabaseError::Query(e.to_string()))
        })
        .await
    }

    async fn delete_room_mapping(&self, id: i64) -> Result<(), DatabaseError> {
        let pool = self.pool.clone();
        with_connection(pool, move |conn| {
            diesel::delete(room_mappings::table.filter(room_mappings::id.eq(id)))
                .execute(conn)
                .map(|_| ())
                .map_err(|e| DatabaseError::Query(e.to_string()))
        })
        .await
    }

    async fn get_rooms_by_guild(&self, guild_id: &str) -> Result<Vec<RoomMapping>, DatabaseError> {
        let pool = self.pool.clone();
        let guild_id = guild_id.to_string();
        with_connection(pool, move |conn| {
            use crate::db::schema::room_mappings::dsl::*;
            room_mappings
                .filter(slack_team_id.eq(guild_id))
                .select(DbRoomMapping::as_select())
                .load::<DbRoomMapping>(conn)
                .map(|rows| rows.into_iter().map(Into::into).collect())
                .map_err(|e| DatabaseError::Query(e.to_string()))
        })
        .await
    }

    async fn get_remote_room_info(
        &self,
        _matrix_room_id: &str,
    ) -> Result<Option<RemoteRoomInfo>, DatabaseError> {
        Ok(None)
    }

    async fn update_remote_room_info(
        &self,
        _matrix_room_id: &str,
        _info: &RemoteRoomInfo,
    ) -> Result<(), DatabaseError> {
        Ok(())
    }
}

pub struct PostgresUserStore {
    pool: Pool,
}

impl PostgresUserStore {
    pub fn new(pool: Pool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl super::UserStore for PostgresUserStore {
    async fn get_user_by_slack_id(
        &self,
        slack_id: &str,
    ) -> Result<Option<UserMapping>, DatabaseError> {
        let pool = self.pool.clone();
        let slack_id = slack_id.to_string();
        with_connection(pool, move |conn| {
            use crate::db::schema::user_mappings::dsl::*;
            user_mappings
                .filter(slack_user_id.eq(slack_id))
                .select(DbUserMapping::as_select())
                .first::<DbUserMapping>(conn)
                .optional()
                .map(|value| value.map(Into::into))
                .map_err(|e| DatabaseError::Query(e.to_string()))
        })
        .await
    }

    async fn create_user_mapping(&self, mapping: &UserMapping) -> Result<(), DatabaseError> {
        let pool = self.pool.clone();
        let mapping = mapping.clone();
        with_connection(pool, move |conn| {
            let new_mapping = NewUserMapping {
                matrix_user_id: &mapping.matrix_user_id,
                slack_user_id: &mapping.slack_user_id,
                slack_username: &mapping.slack_username,
                slack_discriminator: &mapping.slack_discriminator,
                slack_avatar: mapping.slack_avatar.as_deref(),
                created_at: &mapping.created_at,
                updated_at: &mapping.updated_at,
            };

            diesel::insert_into(user_mappings::table)
                .values(new_mapping)
                .execute(conn)
                .map(|_| ())
                .map_err(|e| DatabaseError::Query(e.to_string()))
        })
        .await
    }

    async fn update_user_mapping(&self, mapping: &UserMapping) -> Result<(), DatabaseError> {
        let pool = self.pool.clone();
        let mapping = mapping.clone();
        with_connection(pool, move |conn| {
            let changes = UpdateUserMapping {
                slack_username: &mapping.slack_username,
                slack_discriminator: &mapping.slack_discriminator,
                slack_avatar: mapping.slack_avatar.as_deref(),
                updated_at: &mapping.updated_at,
            };

            diesel::update(user_mappings::table.filter(user_mappings::id.eq(mapping.id)))
                .set(changes)
                .execute(conn)
                .map(|_| ())
                .map_err(|e| DatabaseError::Query(e.to_string()))
        })
        .await
    }

    async fn delete_user_mapping(&self, id: i64) -> Result<(), DatabaseError> {
        let pool = self.pool.clone();
        with_connection(pool, move |conn| {
            diesel::delete(user_mappings::table.filter(user_mappings::id.eq(id)))
                .execute(conn)
                .map(|_| ())
                .map_err(|e| DatabaseError::Query(e.to_string()))
        })
        .await
    }

    async fn get_user_by_matrix_id(
        &self,
        matrix_id: &str,
    ) -> Result<Option<UserMapping>, DatabaseError> {
        let pool = self.pool.clone();
        let matrix_id = matrix_id.to_string();
        with_connection(pool, move |conn| {
            use crate::db::schema::user_mappings::dsl::*;
            user_mappings
                .filter(matrix_user_id.eq(matrix_id))
                .select(DbUserMapping::as_select())
                .first::<DbUserMapping>(conn)
                .optional()
                .map(|value| value.map(Into::into))
                .map_err(|e| DatabaseError::Query(e.to_string()))
        })
        .await
    }

    async fn get_remote_user_info(
        &self,
        _slack_user_id: &str,
    ) -> Result<Option<RemoteUserInfo>, DatabaseError> {
        Ok(None)
    }

    async fn update_remote_user_info(
        &self,
        _slack_user_id: &str,
        _info: &RemoteUserInfo,
    ) -> Result<(), DatabaseError> {
        Ok(())
    }

    async fn get_all_user_ids(&self) -> Result<Vec<String>, DatabaseError> {
        let pool = self.pool.clone();
        with_connection(pool, move |conn| {
            use crate::db::schema::user_mappings::dsl::*;
            user_mappings
                .select(matrix_user_id)
                .load::<String>(conn)
                .map_err(|e| DatabaseError::Query(e.to_string()))
        })
        .await
    }
}

pub struct PostgresMessageStore {
    pool: Pool,
}

impl PostgresMessageStore {
    pub fn new(pool: Pool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl super::MessageStore for PostgresMessageStore {
    async fn get_by_slack_message_id(
        &self,
        slack_message_id_param: &str,
    ) -> Result<Option<MessageMapping>, DatabaseError> {
        let pool = self.pool.clone();
        let slack_message_id_param = slack_message_id_param.to_string();
        with_connection(pool, move |conn| {
            use crate::db::schema::message_mappings::dsl::*;
            message_mappings
                .filter(slack_message_id.eq(slack_message_id_param))
                .select(DbMessageMapping::as_select())
                .first::<DbMessageMapping>(conn)
                .optional()
                .map(|value| value.map(Into::into))
                .map_err(|e| DatabaseError::Query(e.to_string()))
        })
        .await
    }

    async fn get_by_matrix_event_id(
        &self,
        matrix_event_id_param: &str,
    ) -> Result<Option<MessageMapping>, DatabaseError> {
        let pool = self.pool.clone();
        let matrix_event_id_param = matrix_event_id_param.to_string();
        with_connection(pool, move |conn| {
            use crate::db::schema::message_mappings::dsl::*;
            message_mappings
                .filter(matrix_event_id.eq(matrix_event_id_param))
                .select(DbMessageMapping::as_select())
                .first::<DbMessageMapping>(conn)
                .optional()
                .map(|value| value.map(Into::into))
                .map_err(|e| DatabaseError::Query(e.to_string()))
        })
        .await
    }

    async fn upsert_message_mapping(&self, mapping: &MessageMapping) -> Result<(), DatabaseError> {
        let pool = self.pool.clone();
        let mapping = mapping.clone();
        with_connection(pool, move |conn| {
            use crate::db::schema::message_mappings::dsl::*;

            let existing = message_mappings
                .filter(slack_message_id.eq(&mapping.slack_message_id))
                .select(DbMessageMapping::as_select())
                .first::<DbMessageMapping>(conn)
                .optional()
                .map_err(|e| DatabaseError::Query(e.to_string()))?;

            if let Some(existing) = existing {
                let changes = UpdateMessageMapping {
                    matrix_room_id: &mapping.matrix_room_id,
                    matrix_event_id: &mapping.matrix_event_id,
                    updated_at: &mapping.updated_at,
                };
                diesel::update(message_mappings.filter(id.eq(existing.id)))
                    .set(changes)
                    .execute(conn)
                    .map(|_| ())
                    .map_err(|e| DatabaseError::Query(e.to_string()))
            } else {
                let new_mapping = NewMessageMapping {
                    slack_message_id: &mapping.slack_message_id,
                    matrix_room_id: &mapping.matrix_room_id,
                    matrix_event_id: &mapping.matrix_event_id,
                    created_at: &mapping.created_at,
                    updated_at: &mapping.updated_at,
                };
                diesel::insert_into(message_mappings)
                    .values(new_mapping)
                    .execute(conn)
                    .map(|_| ())
                    .map_err(|e| DatabaseError::Query(e.to_string()))
            }
        })
        .await
    }

    async fn delete_by_slack_message_id(
        &self,
        slack_message_id_param: &str,
    ) -> Result<(), DatabaseError> {
        let pool = self.pool.clone();
        let slack_message_id_param = slack_message_id_param.to_string();
        with_connection(pool, move |conn| {
            use crate::db::schema::message_mappings::dsl::*;
            diesel::delete(message_mappings.filter(slack_message_id.eq(slack_message_id_param)))
                .execute(conn)
                .map(|_| ())
                .map_err(|e| DatabaseError::Query(e.to_string()))
        })
        .await
    }

    async fn delete_by_matrix_event_id(
        &self,
        matrix_event_id_param: &str,
    ) -> Result<(), DatabaseError> {
        let pool = self.pool.clone();
        let matrix_event_id_param = matrix_event_id_param.to_string();
        with_connection(pool, move |conn| {
            use crate::db::schema::message_mappings::dsl::*;
            diesel::delete(message_mappings.filter(matrix_event_id.eq(matrix_event_id_param)))
                .execute(conn)
                .map(|_| ())
                .map_err(|e| DatabaseError::Query(e.to_string()))
        })
        .await
    }
}

pub struct PostgresEmojiStore {
    pool: Pool,
}

impl PostgresEmojiStore {
    pub fn new(pool: Pool) -> Self {
        Self { pool }
    }
}

#[derive(Debug, Clone, QueryableByName)]
#[diesel(table_name = crate::db::schema::emoji_mappings)]
struct DbEmojiMapping {
    id: i64,
    slack_emoji_id: String,
    emoji_name: String,
    animated: bool,
    mxc_url: String,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl From<DbEmojiMapping> for EmojiMapping {
    fn from(value: DbEmojiMapping) -> Self {
        Self {
            id: value.id,
            slack_emoji_id: value.slack_emoji_id,
            emoji_name: value.emoji_name,
            animated: value.animated,
            mxc_url: value.mxc_url,
            created_at: value.created_at,
            updated_at: value.updated_at,
        }
    }
}

#[derive(Insertable)]
#[diesel(table_name = crate::db::schema::emoji_mappings)]
struct NewEmojiMapping<'a> {
    slack_emoji_id: &'a str,
    emoji_name: &'a str,
    animated: bool,
    mxc_url: &'a str,
    created_at: &'a DateTime<Utc>,
    updated_at: &'a DateTime<Utc>,
}

#[derive(AsChangeset)]
#[diesel(table_name = crate::db::schema::emoji_mappings)]
struct UpdateEmojiMapping<'a> {
    emoji_name: &'a str,
    animated: bool,
    mxc_url: &'a str,
    updated_at: &'a DateTime<Utc>,
}

#[async_trait]
impl super::EmojiStore for PostgresEmojiStore {
    async fn get_emoji_by_slack_id(
        &self,
        slack_emoji_id: &str,
    ) -> Result<Option<EmojiMapping>, DatabaseError> {
        let pool = self.pool.clone();
        let slack_emoji_id = slack_emoji_id.to_string();
        with_connection(pool, move |conn| {
            diesel::sql_query(
                "SELECT id, slack_emoji_id, emoji_name, animated, mxc_url, created_at, updated_at FROM emoji_mappings WHERE slack_emoji_id = $1"
            )
            .bind::<diesel::sql_types::Text, _>(&slack_emoji_id)
            .get_result::<DbEmojiMapping>(conn)
            .optional()
            .map(|value| value.map(Into::into))
            .map_err(|e| DatabaseError::Query(e.to_string()))
        })
        .await
    }

    async fn get_emoji_by_mxc(&self, mxc_url: &str) -> Result<Option<EmojiMapping>, DatabaseError> {
        let pool = self.pool.clone();
        let mxc_url = mxc_url.to_string();
        with_connection(pool, move |conn| {
            diesel::sql_query(
                "SELECT id, slack_emoji_id, emoji_name, animated, mxc_url, created_at, updated_at FROM emoji_mappings WHERE mxc_url = $1"
            )
            .bind::<diesel::sql_types::Text, _>(&mxc_url)
            .get_result::<DbEmojiMapping>(conn)
            .optional()
            .map(|value| value.map(Into::into))
            .map_err(|e| DatabaseError::Query(e.to_string()))
        })
        .await
    }

    async fn create_emoji(&self, emoji: &EmojiMapping) -> Result<(), DatabaseError> {
        let pool = self.pool.clone();
        let emoji = emoji.clone();
        with_connection(pool, move |conn| {
            let new_emoji = NewEmojiMapping {
                slack_emoji_id: &emoji.slack_emoji_id,
                emoji_name: &emoji.emoji_name,
                animated: emoji.animated,
                mxc_url: &emoji.mxc_url,
                created_at: &emoji.created_at,
                updated_at: &emoji.updated_at,
            };
            diesel::insert_into(crate::db::schema::emoji_mappings::table)
                .values(&new_emoji)
                .execute(conn)
                .map(|_| ())
                .map_err(|e| DatabaseError::Query(e.to_string()))
        })
        .await
    }

    async fn update_emoji(&self, emoji: &EmojiMapping) -> Result<(), DatabaseError> {
        let pool = self.pool.clone();
        let emoji = emoji.clone();
        with_connection(pool, move |conn| {
            let changes = UpdateEmojiMapping {
                emoji_name: &emoji.emoji_name,
                animated: emoji.animated,
                mxc_url: &emoji.mxc_url,
                updated_at: &emoji.updated_at,
            };
            diesel::update(crate::db::schema::emoji_mappings::table.filter(
                crate::db::schema::emoji_mappings::slack_emoji_id.eq(&emoji.slack_emoji_id),
            ))
            .set(changes)
            .execute(conn)
            .map(|_| ())
            .map_err(|e| DatabaseError::Query(e.to_string()))
        })
        .await
    }

    async fn delete_emoji(&self, slack_emoji_id: &str) -> Result<(), DatabaseError> {
        let pool = self.pool.clone();
        let slack_emoji_id = slack_emoji_id.to_string();
        with_connection(pool, move |conn| {
            diesel::delete(
                crate::db::schema::emoji_mappings::table.filter(
                    crate::db::schema::emoji_mappings::slack_emoji_id.eq(slack_emoji_id),
                ),
            )
            .execute(conn)
            .map(|_| ())
            .map_err(|e| DatabaseError::Query(e.to_string()))
        })
        .await
    }
}

