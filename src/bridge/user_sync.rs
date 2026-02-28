use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use chrono::Utc;
use tracing::{debug, info, warn};

use crate::config::Config;
use crate::db::{DatabaseManager, UserMapping};
use crate::matrix::MatrixAppservice;

const CACHE_TTL_SECS: i64 = 300;

struct DisplaynameCacheEntry {
    displayname: String,
    timestamp: i64,
}

struct AvatarCacheEntry {
    mxc_url: String,
    timestamp: i64,
}

pub struct UserSyncHandler {
    db: Arc<DatabaseManager>,
    matrix: Arc<MatrixAppservice>,
    config: Arc<Config>,
    avatar_cache: HashMap<String, AvatarCacheEntry>,
    displayname_cache: HashMap<String, DisplaynameCacheEntry>,
}

impl UserSyncHandler {
    pub fn new(
        db: Arc<DatabaseManager>,
        matrix: Arc<MatrixAppservice>,
        config: Arc<Config>,
    ) -> Self {
        Self {
            db,
            matrix,
            config,
            avatar_cache: HashMap::new(),
            displayname_cache: HashMap::new(),
        }
    }

    pub async fn on_user_update(
        &self,
        slack_user_id: &str,
        slack_username: Option<&str>,
        slack_avatar_url: Option<&str>,
        is_webhook: bool,
    ) -> Result<()> {
        let user_state = self
            .get_user_update_state(
                slack_user_id,
                slack_username,
                slack_avatar_url,
                is_webhook,
            )
            .await?;

        if user_state.create_user {
            info!(
                "Creating new ghost user for Slack user {}",
                slack_user_id
            );
        }

        self.apply_state_to_profile(&user_state).await?;

        if user_state.displayname.is_some() || user_state.avatar_url.is_some() {
            self.update_state_for_guilds(&user_state).await?;
        }

        Ok(())
    }

    async fn get_user_update_state(
        &self,
        slack_user_id: &str,
        slack_username: Option<&str>,
        slack_avatar_url: Option<&str>,
        is_webhook: bool,
    ) -> Result<UserState> {
        let mxid_extra = if is_webhook {
            format!(
                "_{}",
                self.sanitize_for_mxid(slack_username.unwrap_or("unknown"))
            )
        } else {
            String::new()
        };

        let displayname = slack_username.map(|name| {
            crate::utils::formatting::apply_pattern_string(
                &self.config.ghosts.username_pattern,
                &[("id", slack_user_id), ("tag", "0000"), ("username", name)],
            )
        });

        let existing = self
            .db
            .user_store()
            .get_user_by_slack_id(&format!("{}{}", slack_user_id, mxid_extra))
            .await?;

        let user_state = match existing {
            None => UserState {
                id: format!("{}{}", slack_user_id, mxid_extra),
                mx_user_id: format!(
                    "@_slack_{}{}:{}",
                    slack_user_id, mxid_extra, self.config.bridge.domain
                ),
                create_user: true,
                displayname: displayname.clone(),
                avatar_url: slack_avatar_url.map(ToOwned::to_owned),
                remove_avatar: false,
                roles: Vec::new(),
            },
            Some(existing) => UserState {
                id: format!("{}{}", slack_user_id, mxid_extra),
                mx_user_id: existing.matrix_user_id.clone(),
                create_user: false,
                displayname: if displayname.as_ref() != Some(&existing.slack_username) {
                    displayname.clone()
                } else {
                    None
                },
                avatar_url: if slack_avatar_url != existing.slack_avatar.as_deref() {
                    slack_avatar_url.map(ToOwned::to_owned)
                } else {
                    None
                },
                remove_avatar: slack_avatar_url.is_none() && existing.slack_avatar.is_some(),
                roles: Vec::new(),
            },
        };

        Ok(user_state)
    }

    async fn apply_state_to_profile(&self, state: &UserState) -> Result<()> {
        let mapping = UserMapping {
            id: 0,
            matrix_user_id: state.mx_user_id.clone(),
            slack_user_id: state.id.clone(),
            slack_username: state.displayname.clone().unwrap_or_default(),
            slack_discriminator: "0000".to_string(),
            slack_avatar: state.avatar_url.clone(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };

        if state.create_user {
            self.db.user_store().create_user_mapping(&mapping).await?;
            info!("Created user mapping for Slack user {}", state.id);
        } else {
            self.db.user_store().update_user_mapping(&mapping).await?;
            debug!("Updated user mapping for Slack user {}", state.id);
        }

        self.matrix
            .ensure_ghost_user_registered(&state.id, state.displayname.as_deref())
            .await?;

        if let Some(displayname) = &state.displayname {
            debug!(
                "Setting displayname for {} to {}",
                state.mx_user_id, displayname
            );
            if let Err(e) = self
                .matrix
                .set_ghost_displayname(&state.id, displayname)
                .await
            {
                warn!("Failed to set displayname for {}: {}", state.mx_user_id, e);
            }
        }

        if let Some(avatar_url) = &state.avatar_url {
            debug!(
                "Setting avatar for {} from {}",
                state.mx_user_id, avatar_url
            );
            match self.upload_avatar_to_matrix(&state.id, avatar_url).await {
                Ok(mxc_url) => {
                    if let Err(e) = self.matrix.set_ghost_avatar(&state.id, &mxc_url).await {
                        warn!("Failed to set avatar for {}: {}", state.mx_user_id, e);
                    }
                }
                Err(e) => {
                    warn!("Failed to upload avatar for {}: {}", state.mx_user_id, e);
                }
            }
        }

        if state.remove_avatar {
            debug!("Removing avatar for {}", state.mx_user_id);
            if let Err(e) = self.matrix.set_ghost_avatar(&state.id, "").await {
                warn!("Failed to remove avatar for {}: {}", state.mx_user_id, e);
            }
        }

        Ok(())
    }

    async fn upload_avatar_to_matrix(
        &self,
        slack_user_id: &str,
        avatar_url: &str,
    ) -> Result<String> {
        use crate::media::MediaHandler;

        let media_handler = MediaHandler::new(&self.config.bridge.homeserver_url);
        let media = media_handler.download_from_url(avatar_url).await?;

        let content_type = media.content_type.clone();
        let filename = media.filename.clone();

        let mxc_url = self
            .matrix
            .upload_media_for_ghost(slack_user_id, &media.data, &content_type, &filename)
            .await?;

        info!("Uploaded avatar for {} to {}", slack_user_id, mxc_url);
        Ok(mxc_url)
    }

    async fn update_state_for_guilds(&self, state: &UserState) -> Result<()> {
        let room_mappings = self.db.room_store().list_room_mappings(i64::MAX, 0).await?;

        if room_mappings.is_empty() {
            debug!("No rooms to update user state for");
            return Ok(());
        }

        let mut guild_rooms: HashMap<String, Vec<String>> = HashMap::new();
        for mapping in room_mappings {
            guild_rooms
                .entry(mapping.slack_team_id.clone())
                .or_default()
                .push(mapping.matrix_room_id);
        }

        for (_guild_id, room_ids) in guild_rooms {
            for room_id in room_ids {
                if let Err(e) = self.apply_state_to_room(state, &room_id).await {
                    warn!("Failed to update user state in room {}: {}", room_id, e);
                }
            }
        }

        Ok(())
    }

    async fn apply_state_to_room(&self, state: &UserState, room_id: &str) -> Result<()> {
        debug!(
            "Applying member state for {} in room {}",
            state.mx_user_id, room_id
        );

        if let Some(displayname) = &state.displayname {
            self.matrix
                .set_ghost_room_displayname(&state.id, room_id, displayname)
                .await?;
        }

        if let Some(avatar_mxc) = &state.avatar_url {
            self.matrix
                .set_ghost_room_avatar(&state.id, room_id, avatar_mxc)
                .await?;
        }

        if !state.roles.is_empty() {
            self.matrix
                .set_ghost_room_roles(&state.id, room_id, &state.roles)
                .await?;
        }

        Ok(())
    }

    pub async fn ensure_user_in_room(&self, slack_user_id: &str, room_id: &str) -> Result<()> {
        self.matrix
            .ensure_ghost_user_registered(slack_user_id, None)
            .await?;
        self.matrix
            .invite_ghost_to_room(slack_user_id, room_id)
            .await?;

        Ok(())
    }

    pub async fn remove_user_from_room(&self, slack_user_id: &str, room_id: &str) -> Result<()> {
        self.matrix
            .kick_ghost_from_room(slack_user_id, room_id)
            .await?;

        Ok(())
    }

    pub async fn sync_user_roles(
        &self,
        slack_user_id: &str,
        guild_id: &str,
        roles: &[String],
    ) -> Result<()> {
        let room_mappings = self.db.room_store().get_rooms_by_guild(guild_id).await?;

        for mapping in room_mappings {
            if let Err(e) = self
                .matrix
                .set_ghost_room_roles(slack_user_id, &mapping.matrix_room_id, roles)
                .await
            {
                warn!(
                    "Failed to sync roles for {} in room {}: {}",
                    slack_user_id, mapping.matrix_room_id, e
                );
            }
        }

        Ok(())
    }

    fn sanitize_for_mxid(&self, input: &str) -> String {
        let mut result = String::new();
        for c in input.chars() {
            match c {
                'a'..='z' | 'A'..='Z' | '0'..='9' | '_' | '-' | '.' | '=' | '/' | '+' => {
                    result.push(c);
                }
                _ => {
                    result.push('_');
                }
            }
        }
        result
    }
}

#[derive(Debug)]
struct UserState {
    id: String,
    mx_user_id: String,
    create_user: bool,
    displayname: Option<String>,
    avatar_url: Option<String>,
    remove_avatar: bool,
    roles: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sanitize_test(input: &str) -> String {
        let mut result = String::new();
        for c in input.chars() {
            match c {
                'a'..='z' | 'A'..='Z' | '0'..='9' | '_' | '-' | '.' | '=' | '/' | '+' => {
                    result.push(c);
                }
                _ => {
                    result.push('_');
                }
            }
        }
        result
    }

    #[test]
    fn sanitize_for_mxid_replaces_special_chars() {
        let result = sanitize_test("hello world!");
        assert_eq!(result, "hello_world_");
    }

    #[test]
    fn sanitize_for_mxid_keeps_alphanumeric() {
        let result = sanitize_test("User123");
        assert_eq!(result, "User123");
    }

    #[test]
    fn sanitize_for_mxid_keeps_special_allowed() {
        let result = sanitize_test("user-name.test_123");
        assert_eq!(result, "user-name.test_123");
    }
}

