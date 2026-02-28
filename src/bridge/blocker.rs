use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Result;
use tokio::sync::RwLock;
use tracing::{info, warn};

use crate::config::Config;
use crate::db::DatabaseManager;

const DEFAULT_USER_LIMIT: u32 = 100;
const DEFAULT_CHECK_INTERVAL_SECS: u64 = 60;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BridgeState {
    Active,
    Paused,
    Limited,
}

#[derive(Debug, Clone)]
pub struct BridgeStatus {
    pub state: BridgeState,
    pub active_users: u64,
    pub total_rooms: u64,
    pub messages_sent: u64,
    pub messages_received: u64,
    pub last_check: Instant,
    pub pause_reason: Option<String>,
}

pub struct BridgeBlocker {
    db: Arc<DatabaseManager>,
    config: Arc<Config>,
    state: Arc<RwLock<BridgeStatus>>,
    user_limit: u32,
}

impl BridgeBlocker {
    pub fn new(db: Arc<DatabaseManager>, config: Arc<Config>) -> Self {
        let user_limit = config.bridge.user_limit.unwrap_or(DEFAULT_USER_LIMIT);

        Self {
            db,
            config,
            state: Arc::new(RwLock::new(BridgeStatus {
                state: BridgeState::Active,
                active_users: 0,
                total_rooms: 0,
                messages_sent: 0,
                messages_received: 0,
                last_check: Instant::now(),
                pause_reason: None,
            })),
            user_limit,
        }
    }

    pub async fn check_and_update(&self) -> Result<BridgeState> {
        let active_users = self.count_active_users().await?;
        let total_rooms = self.count_rooms().await?;

        let mut status = self.state.write().await;
        status.active_users = active_users;
        status.total_rooms = total_rooms;
        status.last_check = Instant::now();

        if active_users > self.user_limit as u64 {
            status.state = BridgeState::Limited;
            status.pause_reason = Some(format!(
                "User limit exceeded: {} > {}",
                active_users, self.user_limit
            ));
            warn!("Bridge limited: {}", status.pause_reason.as_ref().unwrap());
        } else if status.state == BridgeState::Limited && active_users <= self.user_limit as u64 {
            status.state = BridgeState::Active;
            status.pause_reason = None;
            info!("Bridge restored to active state");
        }

        Ok(status.state.clone())
    }

    pub async fn pause(&self, reason: &str) -> Result<()> {
        let mut status = self.state.write().await;
        status.state = BridgeState::Paused;
        status.pause_reason = Some(reason.to_string());
        info!("Bridge paused: {}", reason);
        Ok(())
    }

    pub async fn resume(&self) -> Result<()> {
        let mut status = self.state.write().await;
        status.state = BridgeState::Active;
        status.pause_reason = None;
        info!("Bridge resumed");
        Ok(())
    }

    pub async fn is_blocked(&self) -> bool {
        let status = self.state.read().await;
        status.state != BridgeState::Active
    }

    pub async fn get_status(&self) -> BridgeStatus {
        self.state.read().await.clone()
    }

    pub async fn record_message_sent(&self) {
        let mut status = self.state.write().await;
        status.messages_sent += 1;
    }

    pub async fn record_message_received(&self) {
        let mut status = self.state.write().await;
        status.messages_received += 1;
    }

    async fn count_active_users(&self) -> Result<u64> {
        let user_ids = self.db.user_store().get_all_user_ids().await?;
        Ok(user_ids.len() as u64)
    }

    async fn count_rooms(&self) -> Result<u64> {
        let count = self.db.room_store().count_rooms().await?;
        Ok(count as u64)
    }

    pub fn user_limit(&self) -> u32 {
        self.user_limit
    }

    pub async fn should_accept_new_user(&self) -> bool {
        let status = self.state.read().await;
        if status.state != BridgeState::Active {
            return false;
        }
        status.active_users < self.user_limit as u64
    }
}

#[cfg(test)]
mod tests {
    use super::{BridgeBlocker, BridgeState};

    #[test]
    fn bridge_status_defaults_to_active() {
        let status = super::BridgeStatus {
            state: BridgeState::Active,
            active_users: 0,
            total_rooms: 0,
            messages_sent: 0,
            messages_received: 0,
            last_check: std::time::Instant::now(),
            pause_reason: None,
        };
        assert_eq!(status.state, BridgeState::Active);
    }
}
