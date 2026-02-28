use crate::parsers::{parse_guild_and_channel, parse_prefixed_command};

const DEFAULT_PROVISIONING_POWER_LEVEL: i64 = 50;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MatrixCommandPermission {
    pub required_level: i64,
    pub category: &'static str,
    pub subcategory: &'static str,
    pub self_service: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MatrixCommandOutcome {
    Ignored,
    Reply(String),
    BridgeRequested {
        guild_id: String,
        channel_id: String,
    },
    UnbridgeRequested,
}

#[derive(Debug, Clone)]
pub struct MatrixCommandHandler {
    prefix: &'static str,
    self_service_enabled: bool,
    provisioning_power_level: i64,
}

impl Default for MatrixCommandHandler {
    fn default() -> Self {
        Self {
            prefix: "!slack",
            self_service_enabled: true,
            provisioning_power_level: DEFAULT_PROVISIONING_POWER_LEVEL,
        }
    }
}

impl MatrixCommandHandler {
    pub fn new(self_service_enabled: bool, provisioning_power_level: Option<i64>) -> Self {
        Self {
            self_service_enabled,
            provisioning_power_level: provisioning_power_level
                .unwrap_or(DEFAULT_PROVISIONING_POWER_LEVEL),
            ..Self::default()
        }
    }

    pub fn is_command(&self, message: &str) -> bool {
        message.trim_start().starts_with(self.prefix)
    }

    pub fn handle<P>(
        &self,
        message: &str,
        room_is_bridged: bool,
        permission_check: P,
    ) -> MatrixCommandOutcome
    where
        P: Fn(MatrixCommandPermission) -> Result<bool, String>,
    {
        let parsed = match parse_prefixed_command(self.prefix, message) {
            Some(parsed) => parsed,
            None => return MatrixCommandOutcome::Ignored,
        };

        match parsed.command.as_str() {
            "help" => MatrixCommandOutcome::Reply(
                self.render_help(parsed.args.first().map(String::as_str)),
            ),
            "bridge" => {
                if let Err(reply) = self.ensure_permission(&permission_check) {
                    return MatrixCommandOutcome::Reply(reply);
                }
                if room_is_bridged {
                    return MatrixCommandOutcome::Reply(
                        "This room is already bridged to a Slack guild.".to_string(),
                    );
                }
                let Some((guild_id, channel_id)) = parse_guild_and_channel(&parsed.args) else {
                    return MatrixCommandOutcome::Reply(
                        "Invalid syntax. For more information try `!slack help bridge`"
                            .to_string(),
                    );
                };
                MatrixCommandOutcome::BridgeRequested {
                    guild_id,
                    channel_id,
                }
            }
            "unbridge" => {
                if let Err(reply) = self.ensure_permission(&permission_check) {
                    return MatrixCommandOutcome::Reply(reply);
                }
                if !room_is_bridged {
                    return MatrixCommandOutcome::Reply("This room is not bridged.".to_string());
                }
                MatrixCommandOutcome::UnbridgeRequested
            }
            _ => MatrixCommandOutcome::Reply(
                "**ERROR:** unknown command. Try `!slack help` to see all commands".to_string(),
            ),
        }
    }

    fn ensure_permission<P>(&self, permission_check: &P) -> Result<(), String>
    where
        P: Fn(MatrixCommandPermission) -> Result<bool, String>,
    {
        let permission = MatrixCommandPermission {
            required_level: self.provisioning_power_level,
            category: "events",
            subcategory: "m.room.power_levels",
            self_service: true,
        };

        if permission.self_service && !self.self_service_enabled {
            return Err(
                "The owner of this bridge does not permit self-service bridging.".to_string(),
            );
        }
        let granted = permission_check(permission).map_err(|err| format!("**ERROR:** {err}"))?;
        if granted {
            Ok(())
        } else {
            Err("**ERROR:** insufficient permissions to use this command! Try `!slack help` to see all available commands".to_string())
        }
    }

    fn render_help(&self, command: Option<&str>) -> String {
        match command {
            Some("bridge") => "`!slack bridge <guildId> <channelId>`: Bridges this room to a Slack channel\nUse `guild/channel` or `guild channel`.".to_string(),
            Some("unbridge") => {
                "`!slack unbridge`: Unbridges a Slack channel from this room".to_string()
            }
            Some(_) => "**ERROR:** unknown command! Try `!slack help` to see all commands"
                .to_string(),
            None => {
                "Available Commands:\n - `!slack bridge <guildId> <channelId>`: Bridges this room to a Slack channel\n - `!slack unbridge`: Unbridges a Slack channel from this room".to_string()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{MatrixCommandHandler, MatrixCommandOutcome, MatrixCommandPermission};

    #[test]
    fn bridge_command_supports_slash_syntax() {
        let handler = MatrixCommandHandler::default();
        let outcome = handler.handle("!slack bridge 1/2", false, |_| Ok(true));
        assert_eq!(
            outcome,
            MatrixCommandOutcome::BridgeRequested {
                guild_id: "1".to_string(),
                channel_id: "2".to_string()
            }
        );
    }

    #[test]
    fn bridge_command_rejects_when_permission_denied() {
        let handler = MatrixCommandHandler::default();
        let outcome = handler.handle("!slack bridge 1 2", false, |_| Ok(false));
        assert_eq!(
            outcome,
            MatrixCommandOutcome::Reply("**ERROR:** insufficient permissions to use this command! Try `!slack help` to see all available commands".to_string())
        );
    }

    #[test]
    fn unbridge_requires_existing_link() {
        let handler = MatrixCommandHandler::default();
        let outcome = handler.handle("!slack unbridge", false, |_| Ok(true));
        assert_eq!(
            outcome,
            MatrixCommandOutcome::Reply("This room is not bridged.".to_string())
        );
    }

    #[test]
    fn self_service_flag_blocks_command() {
        let handler = MatrixCommandHandler::new(false, Some(50));
        let outcome = handler.handle("!slack bridge 1 2", false, |permission| {
            assert_eq!(
                permission,
                MatrixCommandPermission {
                    required_level: 50,
                    category: "events",
                    subcategory: "m.room.power_levels",
                    self_service: true,
                }
            );
            Ok(true)
        });
        assert_eq!(
            outcome,
            MatrixCommandOutcome::Reply(
                "The owner of this bridge does not permit self-service bridging.".to_string()
            )
        );
    }
}
