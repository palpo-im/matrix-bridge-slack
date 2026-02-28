use std::sync::Arc;

use anyhow::Result;
use serde_json::json;
use tracing::{debug, info, warn};

use crate::config::Config;
use crate::db::DatabaseManager;
use crate::matrix::MatrixAppservice;

#[derive(Debug, Clone)]
pub struct AdminConfig {
    pub admin_mxid: Option<String>,
    pub notify_on_token_invalid: bool,
    pub notify_on_errors: bool,
    pub error_threshold: u32,
}

impl Default for AdminConfig {
    fn default() -> Self {
        Self {
            admin_mxid: None,
            notify_on_token_invalid: true,
            notify_on_errors: true,
            error_threshold: 5,
        }
    }
}

pub struct AdminNotifier {
    config: AdminConfig,
    matrix: Arc<MatrixAppservice>,
    db: Arc<DatabaseManager>,
    error_count: std::sync::atomic::AtomicU32,
}

impl AdminNotifier {
    pub fn new(
        config: AdminConfig,
        matrix: Arc<MatrixAppservice>,
        db: Arc<DatabaseManager>,
    ) -> Self {
        Self {
            config,
            matrix,
            db,
            error_count: std::sync::atomic::AtomicU32::new(0),
        }
    }

    pub fn from_config(
        config: Arc<Config>,
        matrix: Arc<MatrixAppservice>,
        db: Arc<DatabaseManager>,
    ) -> Self {
        let admin_config = AdminConfig {
            admin_mxid: config.bridge.admin_mxid.clone(),
            notify_on_token_invalid: true,
            notify_on_errors: true,
            error_threshold: 5,
        };
        Self::new(admin_config, matrix, db)
    }

    pub async fn notify_token_invalid(&self) -> Result<()> {
        if !self.config.notify_on_token_invalid {
            return Ok(());
        }

        let Some(ref admin_mxid) = self.config.admin_mxid else {
            warn!("Token invalid but no admin_mxid configured");
            return Ok(());
        };

        info!("Sending token invalid notification to {}", admin_mxid);

        let message = "⚠️ **Slack Bot Token Invalid**\n\nYour Slack bot token appears to be invalid. The bridge cannot function properly.\n\nPlease update your bot token in the configuration and restart the bridge.";

        self.send_admin_message(admin_mxid, message).await
    }

    pub async fn notify_error(&self, error: &str, context: Option<&str>) -> Result<()> {
        if !self.config.notify_on_errors {
            return Ok(());
        }

        let count = self
            .error_count
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        if count < self.config.error_threshold {
            return Ok(());
        }

        let Some(ref admin_mxid) = self.config.admin_mxid else {
            return Ok(());
        };

        debug!(
            "Sending error notification to {} (count: {})",
            admin_mxid, count
        );

        let message = if let Some(ctx) = context {
            format!(
                "⚠️ **Bridge Error**\n\nError: {}\nContext: {}\n\nThis is error #{} since the last notification.",
                error, ctx, count
            )
        } else {
            format!(
                "⚠️ **Bridge Error**\n\nError: {}\n\nThis is error #{} since the last notification.",
                error, count
            )
        };

        self.send_admin_message(admin_mxid, &message).await?;

        self.error_count
            .store(0, std::sync::atomic::Ordering::Relaxed);

        Ok(())
    }

    pub async fn notify_bridge_paused(&self, reason: &str) -> Result<()> {
        let Some(ref admin_mxid) = self.config.admin_mxid else {
            return Ok(());
        };

        let message = format!(
            "⏸️ **Bridge Paused**\n\nThe bridge has been paused.\n\nReason: {}",
            reason
        );

        self.send_admin_message(admin_mxid, &message).await
    }

    pub async fn notify_bridge_resumed(&self) -> Result<()> {
        let Some(ref admin_mxid) = self.config.admin_mxid else {
            return Ok(());
        };

        let message = "▶️ **Bridge Resumed**\n\nThe bridge has been resumed and is now active.";

        self.send_admin_message(admin_mxid, message).await
    }

    pub async fn notify_user_limit_reached(&self, current: u64, limit: u32) -> Result<()> {
        let Some(ref admin_mxid) = self.config.admin_mxid else {
            return Ok(());
        };

        let message = format!(
            "📊 **User Limit Reached**\n\nThe bridge has reached its user limit.\n\nCurrent users: {}\nLimit: {}",
            current, limit
        );

        self.send_admin_message(admin_mxid, &message).await
    }

    async fn send_admin_message(&self, admin_mxid: &str, message: &str) -> Result<()> {
        match self.matrix.create_dm_room(admin_mxid).await {
            Ok(room_id) => {
                self.matrix.send_notice(&room_id, message).await?;
                info!("Sent admin notification to {}", admin_mxid);
            }
            Err(e) => {
                warn!("Failed to create DM room for admin {}: {}", admin_mxid, e);
                return Err(e);
            }
        }
        Ok(())
    }

    pub fn reset_error_count(&self) {
        self.error_count
            .store(0, std::sync::atomic::Ordering::Relaxed);
    }

    pub fn has_admin_configured(&self) -> bool {
        self.config.admin_mxid.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn admin_config_defaults() {
        let config = AdminConfig::default();
        assert!(config.admin_mxid.is_none());
        assert!(config.notify_on_token_invalid);
        assert!(config.notify_on_errors);
    }
}
