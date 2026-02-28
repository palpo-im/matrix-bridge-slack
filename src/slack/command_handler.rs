use std::collections::HashSet;

use crate::parsers::parse_prefixed_command;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModerationAction {
    Kick,
    Ban,
    Unban,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SlackCommandOutcome {
    Ignored,
    Reply(String),
    ApproveRequested,
    DenyRequested,
    ModerationRequested {
        action: ModerationAction,
        matrix_user: String,
    },
    UnbridgeRequested,
    BridgeRequested {
        guild_id: String,
        channel_id: String,
    },
}

#[derive(Debug, Clone)]
pub struct SlackCommandHandler {
    prefix: &'static str,
}

impl Default for SlackCommandHandler {
    fn default() -> Self {
        Self { prefix: "!matrix" }
    }
}

impl SlackCommandHandler {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_command(&self, message: &str) -> bool {
        message.trim_start().starts_with(self.prefix)
    }

    pub fn handle(
        &self,
        message: &str,
        is_channel_bridged: bool,
        granted_permissions: &HashSet<String>,
    ) -> SlackCommandOutcome {
        let parsed = match parse_prefixed_command(self.prefix, message) {
            Some(parsed) => parsed,
            None => return SlackCommandOutcome::Ignored,
        };

        match parsed.command.as_str() {
            "help" => SlackCommandOutcome::Reply(
                self.render_help(parsed.args.first().map(String::as_str)),
            ),
            "approve" => {
                if !has_all_permissions(granted_permissions, &["MANAGE_WEBHOOKS"]) {
                    return permission_denied();
                }
                SlackCommandOutcome::ApproveRequested
            }
            "deny" => {
                if !has_all_permissions(granted_permissions, &["MANAGE_WEBHOOKS"]) {
                    return permission_denied();
                }
                SlackCommandOutcome::DenyRequested
            }
            "bridge" => self.handle_bridge(parsed.args, granted_permissions, is_channel_bridged),
            "unbridge" => {
                if !has_all_permissions(
                    granted_permissions,
                    &["MANAGE_WEBHOOKS", "MANAGE_CHANNELS"],
                ) {
                    return permission_denied();
                }
                if !is_channel_bridged {
                    return SlackCommandOutcome::Reply(
                        "This channel is not bridged to a plumbed matrix room".to_string(),
                    );
                }
                SlackCommandOutcome::UnbridgeRequested
            }
            "kick" => self.handle_moderation(
                parsed.args,
                granted_permissions,
                "KICK_MEMBERS",
                ModerationAction::Kick,
            ),
            "ban" => self.handle_moderation(
                parsed.args,
                granted_permissions,
                "BAN_MEMBERS",
                ModerationAction::Ban,
            ),
            "unban" => self.handle_moderation(
                parsed.args,
                granted_permissions,
                "BAN_MEMBERS",
                ModerationAction::Unban,
            ),
            _ => SlackCommandOutcome::Reply(
                "**ERROR:** unknown command. Try `!matrix help` to see all commands".to_string(),
            ),
        }
    }

    fn handle_bridge(
        &self,
        args: Vec<String>,
        granted_permissions: &HashSet<String>,
        is_channel_bridged: bool,
    ) -> SlackCommandOutcome {
        if !has_all_permissions(granted_permissions, &["MANAGE_WEBHOOKS", "MANAGE_CHANNELS"]) {
            return permission_denied();
        }

        if is_channel_bridged {
            return SlackCommandOutcome::Reply(
                "This channel is already bridged. Use `!matrix unbridge` to remove the bridge first.".to_string(),
            );
        }

        if args.len() < 2 {
            return SlackCommandOutcome::Reply(
                "**ERROR:** Invalid syntax. Usage: `!matrix bridge <guild_id> <channel_id>`"
                    .to_string(),
            );
        }

        let guild_id = args[0].clone();
        let channel_id = args[1].clone();

        SlackCommandOutcome::BridgeRequested {
            guild_id,
            channel_id,
        }
    }

    fn handle_moderation(
        &self,
        args: Vec<String>,
        granted_permissions: &HashSet<String>,
        needed_permission: &str,
        action: ModerationAction,
    ) -> SlackCommandOutcome {
        if !has_all_permissions(granted_permissions, &[needed_permission]) {
            return permission_denied();
        }
        let matrix_user = args.join(" ").trim().to_string();
        if matrix_user.is_empty() {
            return SlackCommandOutcome::Reply(format!(
                "Invalid syntax. For more information try `!matrix help {}`",
                action_keyword(&action),
            ));
        }
        SlackCommandOutcome::ModerationRequested {
            action,
            matrix_user,
        }
    }

    fn render_help(&self, command: Option<&str>) -> String {
        match command {
            Some("approve") => "`!matrix approve`: Approve a pending bridge request".to_string(),
            Some("deny") => "`!matrix deny`: Deny a pending bridge request".to_string(),
            Some("bridge") => "`!matrix bridge <guild_id> <channel_id>`: Bridge this channel to a Matrix room".to_string(),
            Some("kick") => "`!matrix kick <name>`: Kicks a user on the Matrix side".to_string(),
            Some("ban") => "`!matrix ban <name>`: Bans a user on the Matrix side".to_string(),
            Some("unban") => "`!matrix unban <name>`: Unbans a user on the Matrix side".to_string(),
            Some("unbridge") => "`!matrix unbridge`: Unbridge Matrix rooms from this channel".to_string(),
            Some(_) => "**ERROR:** unknown command! Try `!matrix help` to see all commands"
                .to_string(),
            None => {
                "Available Commands:\n - `!matrix approve`: Approve a pending bridge request\n - `!matrix deny`: Deny a pending bridge request\n - `!matrix bridge <guild_id> <channel_id>`: Bridge this channel to a Matrix room\n - `!matrix kick <name>`: Kicks a user on the Matrix side\n - `!matrix ban <name>`: Bans a user on the Matrix side\n - `!matrix unban <name>`: Unbans a user on the Matrix side\n - `!matrix unbridge`: Unbridge Matrix rooms from this channel".to_string()
            }
        }
    }
}

fn action_keyword(action: &ModerationAction) -> &'static str {
    match action {
        ModerationAction::Kick => "kick",
        ModerationAction::Ban => "ban",
        ModerationAction::Unban => "unban",
    }
}

fn has_all_permissions(granted: &HashSet<String>, required: &[&str]) -> bool {
    required.iter().all(|perm| granted.contains(*perm))
}

fn permission_denied() -> SlackCommandOutcome {
    SlackCommandOutcome::Reply(
        "**ERROR:** insufficient permissions to use this command! Try `!matrix help` to see all available commands".to_string(),
    )
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::{SlackCommandHandler, SlackCommandOutcome, ModerationAction};

    #[test]
    fn ban_requires_permission() {
        let handler = SlackCommandHandler::new();
        let permissions = HashSet::new();
        let outcome = handler.handle("!matrix ban @alice:example.org", true, &permissions);
        assert_eq!(
            outcome,
            SlackCommandOutcome::Reply("**ERROR:** insufficient permissions to use this command! Try `!matrix help` to see all available commands".to_string()),
        );
    }

    #[test]
    fn ban_command_returns_target() {
        let handler = SlackCommandHandler::new();
        let permissions = HashSet::from(["BAN_MEMBERS".to_string()]);
        let outcome = handler.handle("!matrix ban @alice:example.org", true, &permissions);
        assert_eq!(
            outcome,
            SlackCommandOutcome::ModerationRequested {
                action: ModerationAction::Ban,
                matrix_user: "@alice:example.org".to_string(),
            }
        );
    }

    #[test]
    fn unbridge_requires_both_permissions() {
        let handler = SlackCommandHandler::new();
        let permissions = HashSet::from(["MANAGE_WEBHOOKS".to_string()]);
        let outcome = handler.handle("!matrix unbridge", true, &permissions);
        assert_eq!(
            outcome,
            SlackCommandOutcome::Reply("**ERROR:** insufficient permissions to use this command! Try `!matrix help` to see all available commands".to_string()),
        );
    }

    #[test]
    fn unbridge_rejects_when_not_bridged() {
        let handler = SlackCommandHandler::new();
        let permissions =
            HashSet::from(["MANAGE_WEBHOOKS".to_string(), "MANAGE_CHANNELS".to_string()]);
        let outcome = handler.handle("!matrix unbridge", false, &permissions);
        assert_eq!(
            outcome,
            SlackCommandOutcome::Reply(
                "This channel is not bridged to a plumbed matrix room".to_string()
            )
        );
    }

    #[test]
    fn bridge_command_requires_permissions() {
        let handler = SlackCommandHandler::new();
        let permissions = HashSet::new();
        let outcome = handler.handle("!matrix bridge 123 456", false, &permissions);
        assert_eq!(
            outcome,
            SlackCommandOutcome::Reply("**ERROR:** insufficient permissions to use this command! Try `!matrix help` to see all available commands".to_string()),
        );
    }

    #[test]
    fn bridge_command_returns_guild_and_channel() {
        let handler = SlackCommandHandler::new();
        let permissions =
            HashSet::from(["MANAGE_WEBHOOKS".to_string(), "MANAGE_CHANNELS".to_string()]);
        let outcome = handler.handle("!matrix bridge 123456 789012", false, &permissions);
        assert_eq!(
            outcome,
            SlackCommandOutcome::BridgeRequested {
                guild_id: "123456".to_string(),
                channel_id: "789012".to_string(),
            }
        );
    }
}
