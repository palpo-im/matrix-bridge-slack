use std::sync::Arc;

use anyhow::Result;
use tracing::{debug, info, warn};

use crate::matrix::MatrixAppservice;

pub struct AdminNotifier {
    matrix_client: Arc<MatrixAppservice>,
    admin_mxid: String,
}

impl AdminNotifier {
    pub fn new(matrix_client: Arc<MatrixAppservice>, admin_mxid: String) -> Self {
        Self {
            matrix_client,
            admin_mxid,
        }
    }

    pub async fn notify(&self, message: &str) -> Result<()> {
        info!(
            "sending admin notification to {}: {}",
            self.admin_mxid, message
        );

        let room_id = self.ensure_dm_room().await?;

        self.matrix_client.send_text(&room_id, message).await?;

        debug!("admin notification sent to {}", self.admin_mxid);
        Ok(())
    }

    async fn ensure_dm_room(&self) -> Result<String> {
        let existing = self.find_dm_room().await?;
        if let Some(room_id) = existing {
            debug!(
                "found existing DM room with {}: {}",
                self.admin_mxid, room_id
            );
            return Ok(room_id);
        }

        let room_id = self.matrix_client.create_dm_room(&self.admin_mxid).await?;
        info!("created DM room {} with admin {}", room_id, self.admin_mxid);
        Ok(room_id)
    }

    async fn find_dm_room(&self) -> Result<Option<String>> {
        let rooms = self.matrix_client.get_joined_rooms().await?;

        for room_id in rooms {
            if let Ok(members) = self.matrix_client.get_room_members(&room_id).await {
                let is_dm = members.len() == 2 && members.iter().any(|m| m == &self.admin_mxid);
                if is_dm {
                    return Ok(Some(room_id));
                }
            }
        }

        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn admin_notifier_stores_admin_mxid() {
        // This is a basic test to ensure the struct can be created
        // Integration tests would require a mock MatrixAppservice
    }
}
