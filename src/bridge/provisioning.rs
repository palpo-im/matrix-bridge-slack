use std::collections::HashMap;
use std::time::Duration;

use parking_lot::Mutex;
use tokio::sync::oneshot;
use tracing::warn;

use crate::slack::SlackClient;

const DEFAULT_PERMISSION_TIMEOUT: Duration = Duration::from_secs(300);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApprovalResponseStatus {
    Applied,
    Expired,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ProvisioningError {
    #[error("Timed out waiting for a response from the Slack owners.")]
    TimedOut,
    #[error("The bridge has been declined by the Slack guild.")]
    Declined,
    #[error("Could not send bridge permission request to Slack.")]
    DeliveryFailed,
    #[error("Bridge approval request was cancelled.")]
    Cancelled,
}

struct PendingRequest {
    decision_tx: oneshot::Sender<bool>,
}

pub struct ProvisioningCoordinator {
    timeout: Duration,
    pending: Mutex<HashMap<String, PendingRequest>>,
}

impl Default for ProvisioningCoordinator {
    fn default() -> Self {
        Self::new(DEFAULT_PERMISSION_TIMEOUT)
    }
}

impl ProvisioningCoordinator {
    pub fn new(timeout: Duration) -> Self {
        Self {
            timeout,
            pending: Mutex::new(HashMap::new()),
        }
    }

    pub async fn ask_bridge_permission(
        &self,
        slack_client: &SlackClient,
        channel_id: &str,
        requestor: &str,
    ) -> Result<(), ProvisioningError> {
        let (decision_tx, decision_rx) = oneshot::channel();
        self.pending
            .lock()
            .insert(channel_id.to_string(), PendingRequest { decision_tx });

        let timeout_minutes = self.timeout.as_secs().max(60).div_ceil(60);
        let prompt = format!(
            "{requestor} on matrix would like to bridge this channel. Someone with permission to manage webhooks please reply with `!matrix approve` or `!matrix deny` in the next {timeout_minutes} minutes."
        );

        if let Err(err) = slack_client.send_message(channel_id, &prompt).await {
            warn!(
                "failed to deliver bridge approval prompt to slack channel {}: {}",
                channel_id, err
            );
            self.pending.lock().remove(channel_id);
            return Err(ProvisioningError::DeliveryFailed);
        }

        match tokio::time::timeout(self.timeout, decision_rx).await {
            Ok(Ok(true)) => Ok(()),
            Ok(Ok(false)) => Err(ProvisioningError::Declined),
            Ok(Err(_)) => Err(ProvisioningError::Cancelled),
            Err(_) => {
                self.pending.lock().remove(channel_id);
                Err(ProvisioningError::TimedOut)
            }
        }
    }

    pub fn has_pending_request(&self, channel_id: &str) -> bool {
        self.pending.lock().contains_key(channel_id)
    }

    pub fn mark_approval(&self, channel_id: &str, allow: bool) -> ApprovalResponseStatus {
        let Some(pending) = self.pending.lock().remove(channel_id) else {
            return ApprovalResponseStatus::Expired;
        };
        let _ = pending.decision_tx.send(allow);
        ApprovalResponseStatus::Applied
    }
}
