use super::message_flow::OutboundMatrixMessage;
use crate::db::{MessageMapping, RoomMapping};
use crate::slack::ModerationAction;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RedactionRequest {
    pub(crate) room_id: String,
    pub(crate) event_id: String,
    pub(crate) reason: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TypingRequest {
    pub(crate) room_id: String,
    pub(crate) slack_user_id: String,
    pub(crate) typing: bool,
    pub(crate) timeout_ms: Option<u64>,
}

pub(crate) const SLACK_TYPING_TIMEOUT_MS: u64 = 4000;
const MAX_PREVIEW_CHARS: usize = 120;

pub(crate) fn preview_text(value: &str) -> String {
    let mut chars = value.chars();
    let preview: String = chars.by_ref().take(MAX_PREVIEW_CHARS).collect();
    if chars.next().is_some() {
        format!("{preview}…")
    } else {
        preview
    }
}

pub(crate) fn apply_message_relation_mappings(
    outbound: &mut OutboundMatrixMessage,
    reply_mapping: Option<&MessageMapping>,
    edit_mapping: Option<&MessageMapping>,
) {
    if let Some(link) = reply_mapping {
        outbound.reply_to = Some(link.matrix_event_id.clone());
    }

    if let Some(link) = edit_mapping {
        outbound.edit_of = Some(link.matrix_event_id.clone());
    }
}

pub(crate) fn build_slack_delete_redaction_request(link: &MessageMapping) -> RedactionRequest {
    RedactionRequest {
        room_id: link.matrix_room_id.clone(),
        event_id: link.matrix_event_id.clone(),
        reason: "Deleted on Slack",
    }
}

pub(crate) fn slack_delete_redaction_request(
    link: Option<&MessageMapping>,
) -> Option<RedactionRequest> {
    link.map(build_slack_delete_redaction_request)
}

pub(crate) fn build_slack_typing_request(
    matrix_room_id: &str,
    slack_user_id: &str,
) -> TypingRequest {
    TypingRequest {
        room_id: matrix_room_id.to_string(),
        slack_user_id: slack_user_id.to_string(),
        typing: true,
        timeout_ms: Some(SLACK_TYPING_TIMEOUT_MS),
    }
}

pub(crate) fn should_forward_slack_typing(
    disable_typing_notifications: bool,
    room_mapping: Option<&RoomMapping>,
) -> bool {
    !disable_typing_notifications && room_mapping.is_some()
}

pub(crate) fn action_keyword(action: &ModerationAction) -> &'static str {
    match action {
        ModerationAction::Kick => "kick",
        ModerationAction::Ban => "ban",
        ModerationAction::Unban => "unban",
    }
}

#[cfg(test)]
mod tests {
    use chrono::Utc;

    use super::{
        OutboundMatrixMessage, action_keyword, apply_message_relation_mappings,
        build_slack_delete_redaction_request, build_slack_typing_request,
        slack_delete_redaction_request, preview_text, should_forward_slack_typing,
    };
    use crate::db::{MessageMapping, RoomMapping};
    use crate::slack::ModerationAction;

    fn mapping(slack_message_id: &str, matrix_event_id: &str) -> MessageMapping {
        MessageMapping {
            id: 0,
            slack_message_id: slack_message_id.to_string(),
            matrix_room_id: "!room:example.org".to_string(),
            matrix_event_id: matrix_event_id.to_string(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    #[test]
    fn apply_message_relation_mappings_replaces_ids_when_links_exist() {
        let mut outbound = OutboundMatrixMessage {
            body: "hello".to_string(),
            reply_to: Some("slack-reply-id".to_string()),
            edit_of: Some("slack-edit-id".to_string()),
            attachments: Vec::new(),
        };

        let reply = mapping("slack-reply-id", "$matrix-reply");
        let edit = mapping("slack-edit-id", "$matrix-edit");

        apply_message_relation_mappings(&mut outbound, Some(&reply), Some(&edit));

        assert_eq!(outbound.reply_to, Some("$matrix-reply".to_string()));
        assert_eq!(outbound.edit_of, Some("$matrix-edit".to_string()));
    }

    #[test]
    fn apply_message_relation_mappings_keeps_original_when_links_missing() {
        let mut outbound = OutboundMatrixMessage {
            body: "hello".to_string(),
            reply_to: Some("slack-reply-id".to_string()),
            edit_of: Some("slack-edit-id".to_string()),
            attachments: Vec::new(),
        };

        apply_message_relation_mappings(&mut outbound, None, None);

        assert_eq!(outbound.reply_to, Some("slack-reply-id".to_string()));
        assert_eq!(outbound.edit_of, Some("slack-edit-id".to_string()));
    }

    #[test]
    fn build_slack_delete_redaction_request_maps_fields() {
        let link = mapping("slack-msg-1", "$matrix-event-1");

        let request = build_slack_delete_redaction_request(&link);

        assert_eq!(request.room_id, "!room:example.org");
        assert_eq!(request.event_id, "$matrix-event-1");
        assert_eq!(request.reason, "Deleted on Slack");
    }

    #[test]
    fn slack_delete_redaction_request_returns_none_without_mapping() {
        let request = slack_delete_redaction_request(None);
        assert!(request.is_none());
    }

    #[test]
    fn slack_delete_redaction_request_returns_some_with_mapping() {
        let link = mapping("slack-msg-2", "$matrix-event-2");

        let request = slack_delete_redaction_request(Some(&link))
            .expect("request should be created when mapping exists");

        assert_eq!(request.room_id, "!room:example.org");
        assert_eq!(request.event_id, "$matrix-event-2");
        assert_eq!(request.reason, "Deleted on Slack");
    }

    #[test]
    fn build_slack_typing_request_maps_fields() {
        let request = build_slack_typing_request("!room:example.org", "slack-user-1");

        assert_eq!(request.room_id, "!room:example.org");
        assert_eq!(request.slack_user_id, "slack-user-1");
        assert!(request.typing);
        assert_eq!(request.timeout_ms, Some(4000));
    }

    #[test]
    fn build_slack_typing_request_uses_constant_timeout() {
        let request = build_slack_typing_request("!room:example.org", "slack-user-2");
        assert_eq!(request.timeout_ms, Some(super::SLACK_TYPING_TIMEOUT_MS));
    }

    fn room_mapping() -> RoomMapping {
        RoomMapping {
            id: 1,
            matrix_room_id: "!room:example.org".to_string(),
            slack_channel_id: "123".to_string(),
            slack_channel_name: "general".to_string(),
            slack_team_id: "456".to_string(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    #[test]
    fn should_forward_slack_typing_returns_false_when_disabled() {
        let mapping = room_mapping();
        let should_forward = should_forward_slack_typing(true, Some(&mapping));
        assert!(!should_forward);
    }

    #[test]
    fn should_forward_slack_typing_returns_false_without_mapping() {
        let should_forward = should_forward_slack_typing(false, None);
        assert!(!should_forward);
    }

    #[test]
    fn should_forward_slack_typing_returns_true_when_enabled_and_mapped() {
        let mapping = room_mapping();
        let should_forward = should_forward_slack_typing(false, Some(&mapping));
        assert!(should_forward);
    }

    #[test]
    fn preview_text_returns_original_when_short() {
        let text = "short message";
        assert_eq!(preview_text(text), text);
    }

    #[test]
    fn preview_text_truncates_and_appends_ellipsis_when_long() {
        let text = "x".repeat(130);
        let preview = preview_text(&text);
        assert_eq!(preview.chars().count(), 121);
        assert!(preview.ends_with('…'));
    }

    #[test]
    fn action_keyword_maps_all_moderation_actions() {
        assert_eq!(action_keyword(&ModerationAction::Kick), "kick");
        assert_eq!(action_keyword(&ModerationAction::Ban), "ban");
        assert_eq!(action_keyword(&ModerationAction::Unban), "unban");
    }
}

