use std::collections::VecDeque;

use anyhow::Result;
use async_trait::async_trait;
use parking_lot::Mutex;
use tracing::warn;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SlackPresenceState {
    Online,
    Dnd,
    Idle,
    Offline,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlackActivity {
    pub kind: String,
    pub name: String,
    pub url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlackPresence {
    pub user_id: String,
    pub username: Option<String>,
    pub state: SlackPresenceState,
    pub activities: Vec<SlackActivity>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MatrixPresenceState {
    Online,
    Offline,
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PresenceDecision {
    pub presence: MatrixPresenceState,
    pub status_message: String,
    pub should_drop: bool,
}

#[async_trait]
pub trait MatrixPresenceTarget: Send + Sync {
    async fn set_presence(
        &self,
        slack_user_id: &str,
        presence: MatrixPresenceState,
        status_message: &str,
    ) -> Result<()>;

    async fn ensure_user_registered(
        &self,
        slack_user_id: &str,
        username: Option<&str>,
    ) -> Result<()>;
}

pub struct PresenceHandler {
    bot_slack_user_id: Option<String>,
    queue: Mutex<VecDeque<SlackPresence>>,
}

impl PresenceHandler {
    pub fn new(bot_slack_user_id: Option<String>) -> Self {
        Self {
            bot_slack_user_id,
            queue: Mutex::new(VecDeque::new()),
        }
    }

    pub fn queue_count(&self) -> usize {
        self.queue.lock().len()
    }

    pub fn enqueue_user(&self, presence: SlackPresence) {
        if self
            .bot_slack_user_id
            .as_ref()
            .is_some_and(|bot_id| bot_id == &presence.user_id)
        {
            return;
        }

        let mut queue = self.queue.lock();
        queue.retain(|item| item.user_id != presence.user_id);
        queue.push_back(presence);
    }

    pub fn dequeue_user(&self, user_id: &str) -> bool {
        let mut queue = self.queue.lock();
        let before = queue.len();
        queue.retain(|item| item.user_id != user_id);
        before != queue.len()
    }

    pub async fn process_next<T>(&self, target: &T) -> Result<bool>
    where
        T: MatrixPresenceTarget,
    {
        let Some(presence) = self.queue.lock().pop_front() else {
            return Ok(false);
        };

        let decision = Self::map_presence(&presence);
        if let Err(err) = target
            .set_presence(
                &presence.user_id,
                decision.presence.clone(),
                &decision.status_message,
            )
            .await
        {
            if err.to_string().contains("M_FORBIDDEN") {
                if let Err(register_err) = target
                    .ensure_user_registered(&presence.user_id, presence.username.as_deref())
                    .await
                {
                    warn!(
                        "Could not register Matrix ghost user for slack user {}: {}",
                        presence.user_id, register_err
                    );
                }
            } else {
                warn!(
                    "Could not update Matrix presence for slack user {}: {}",
                    presence.user_id, err
                );
            }
        }

        if !decision.should_drop {
            self.enqueue_user(presence);
        }
        Ok(true)
    }

    pub fn map_presence(presence: &SlackPresence) -> PresenceDecision {
        let mut status_message = String::new();

        if let Some(activity) = presence.activities.first() {
            let mut chars = activity.kind.chars();
            if let Some(first) = chars.next() {
                status_message = format!(
                    "{}{} {}",
                    first.to_uppercase(),
                    chars.as_str().to_lowercase(),
                    activity.name
                );
            } else {
                status_message = activity.name.clone();
            }
            if let Some(url) = &activity.url {
                status_message.push_str(" | ");
                status_message.push_str(url);
            }
        }

        match presence.state {
            SlackPresenceState::Online => PresenceDecision {
                presence: MatrixPresenceState::Online,
                status_message,
                should_drop: false,
            },
            SlackPresenceState::Dnd => {
                let status_message = if status_message.is_empty() {
                    "Do not disturb".to_string()
                } else {
                    format!("Do not disturb | {status_message}")
                };
                PresenceDecision {
                    presence: MatrixPresenceState::Online,
                    status_message,
                    should_drop: false,
                }
            }
            SlackPresenceState::Offline => PresenceDecision {
                presence: MatrixPresenceState::Offline,
                status_message,
                should_drop: true,
            },
            SlackPresenceState::Idle => PresenceDecision {
                presence: MatrixPresenceState::Unavailable,
                status_message,
                should_drop: false,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use parking_lot::Mutex;

    use super::{
        SlackActivity, SlackPresence, SlackPresenceState, MatrixPresenceState,
        MatrixPresenceTarget, PresenceHandler,
    };

    #[derive(Default, Clone)]
    struct MockPresenceTarget {
        calls: Arc<Mutex<Vec<(String, MatrixPresenceState, String)>>>,
        forbid_updates: bool,
    }

    #[async_trait::async_trait]
    impl MatrixPresenceTarget for MockPresenceTarget {
        async fn set_presence(
            &self,
            slack_user_id: &str,
            presence: MatrixPresenceState,
            status_message: &str,
        ) -> anyhow::Result<()> {
            if self.forbid_updates {
                anyhow::bail!("M_FORBIDDEN");
            }
            self.calls.lock().push((
                slack_user_id.to_string(),
                presence,
                status_message.to_string(),
            ));
            Ok(())
        }

        async fn ensure_user_registered(
            &self,
            _slack_user_id: &str,
            _username: Option<&str>,
        ) -> anyhow::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn enqueue_replaces_stale_presence() {
        let handler = PresenceHandler::new(None);
        let first = SlackPresence {
            user_id: "1".to_string(),
            username: Some("alice".to_string()),
            state: SlackPresenceState::Online,
            activities: vec![],
        };
        let second = SlackPresence {
            state: SlackPresenceState::Idle,
            ..first.clone()
        };

        handler.enqueue_user(first);
        handler.enqueue_user(second);
        assert_eq!(handler.queue_count(), 1);
    }

    #[test]
    fn dnd_maps_to_online_with_prefix() {
        let presence = SlackPresence {
            user_id: "1".to_string(),
            username: None,
            state: SlackPresenceState::Dnd,
            activities: vec![SlackActivity {
                kind: "STREAMING".to_string(),
                name: "Rust".to_string(),
                url: Some("https://example.org".to_string()),
            }],
        };

        let decision = PresenceHandler::map_presence(&presence);
        assert_eq!(decision.presence, MatrixPresenceState::Online);
        assert_eq!(
            decision.status_message,
            "Do not disturb | Streaming Rust | https://example.org"
        );
    }

    #[tokio::test]
    async fn offline_presence_is_dropped_after_processing() {
        let handler = PresenceHandler::new(None);
        let target = MockPresenceTarget::default();
        handler.enqueue_user(SlackPresence {
            user_id: "1".to_string(),
            username: Some("alice".to_string()),
            state: SlackPresenceState::Offline,
            activities: vec![],
        });

        let processed = handler.process_next(&target).await.expect("process_next");
        assert!(processed);
        assert_eq!(handler.queue_count(), 0);
    }

    #[tokio::test]
    async fn non_offline_presence_requeues() {
        let handler = PresenceHandler::new(None);
        let target = MockPresenceTarget::default();
        handler.enqueue_user(SlackPresence {
            user_id: "1".to_string(),
            username: Some("alice".to_string()),
            state: SlackPresenceState::Online,
            activities: vec![],
        });

        handler.process_next(&target).await.expect("process_next");
        assert_eq!(handler.queue_count(), 1);
    }
}

