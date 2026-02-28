#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedCommand {
    pub command: String,
    pub args: Vec<String>,
}

pub fn parse_prefixed_command(prefix: &str, message: &str) -> Option<ParsedCommand> {
    let trimmed = message.trim();
    if !trimmed.starts_with(prefix) {
        return None;
    }

    let remainder = trimmed[prefix.len()..].trim();
    if remainder.is_empty() {
        return Some(ParsedCommand {
            command: "help".to_string(),
            args: Vec::new(),
        });
    }

    let mut segments = remainder.split_whitespace();
    let command = segments.next().unwrap_or("help").to_string();
    let args = segments.map(ToString::to_string).collect();

    Some(ParsedCommand { command, args })
}

pub fn parse_guild_and_channel(args: &[String]) -> Option<(String, String)> {
    let first = args.first()?;
    let (guild_id, remainder) = if let Some((guild, chan_from_guild)) = first.split_once('/') {
        (guild.to_string(), Some(chan_from_guild.to_string()))
    } else {
        (first.to_string(), None)
    };

    let channel_id = if let Some(explicit) = args.get(1) {
        explicit.to_string()
    } else {
        remainder?
    };

    if guild_id.is_empty() || channel_id.is_empty() {
        return None;
    }
    Some((guild_id, channel_id))
}

#[cfg(test)]
mod tests {
    use super::{ParsedCommand, parse_guild_and_channel, parse_prefixed_command};

    #[test]
    fn parse_prefixed_command_returns_none_for_other_prefix() {
        assert_eq!(parse_prefixed_command("!slack", "!matrix help"), None);
    }

    #[test]
    fn parse_prefixed_command_defaults_to_help() {
        assert_eq!(
            parse_prefixed_command("!slack", "!slack"),
            Some(ParsedCommand {
                command: "help".to_string(),
                args: vec![]
            })
        );
    }

    #[test]
    fn parse_prefixed_command_splits_command_and_args() {
        assert_eq!(
            parse_prefixed_command("!matrix", "!matrix ban @alice:example.org"),
            Some(ParsedCommand {
                command: "ban".to_string(),
                args: vec!["@alice:example.org".to_string()]
            })
        );
    }

    #[test]
    fn parse_guild_and_channel_supports_slash_format() {
        let args = vec!["123/456".to_string()];
        assert_eq!(
            parse_guild_and_channel(&args),
            Some(("123".to_string(), "456".to_string()))
        );
    }

    #[test]
    fn parse_guild_and_channel_prefers_explicit_channel() {
        let args = vec!["123/456".to_string(), "789".to_string()];
        assert_eq!(
            parse_guild_and_channel(&args),
            Some(("123".to_string(), "789".to_string()))
        );
    }
}
