use std::path::PathBuf;

use clap::{Parser, Subcommand};
use serde_json::json;

#[derive(Parser, Debug)]
#[command(name = "matrix-slack-bridge")]
#[command(about = "Matrix-Slack Bridge", long_about = None)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,

    #[arg(short, long, env = "CONFIG_PATH", default_value = "config.yaml")]
    pub config: PathBuf,

    #[arg(short, long, env = "REGISTRATION_PATH")]
    pub registration: Option<PathBuf>,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    #[command(about = "Generate a registration file for the Matrix homeserver")]
    GenerateRegistration {
        #[arg(short, long, default_value = "slack-registration.yaml")]
        output: PathBuf,

        #[arg(long, default_value = "slack")]
        id: String,

        #[arg(long, default_value = "http://localhost:8008")]
        homeserver_url: String,

        #[arg(long, default_value = "example.org")]
        domain: String,
    },

    #[command(about = "Grant admin privileges to a Matrix user")]
    Adminme {
        #[arg(short, long)]
        user: String,

        #[arg(short, long)]
        room: Option<String>,

        #[arg(short, long, default_value = "100")]
        power_level: i64,
    },

    #[command(about = "Migrate data between database backends")]
    Migrate {
        #[arg(long, help = "Source database URL")]
        from: String,

        #[arg(long, help = "Destination database URL")]
        to: String,

        #[arg(short, long, help = "Dry run without making changes")]
        dry_run: bool,
    },

    #[command(about = "List all bridged rooms")]
    ListRooms {
        #[arg(short, long, help = "Filter by guild ID")]
        guild: Option<String>,

        #[arg(short, long, default_value = "100")]
        limit: i64,
    },

    #[command(about = "Unbridge a room")]
    Unbridge {
        #[arg(short, long, help = "Matrix room ID")]
        room: String,

        #[arg(short, long, help = "Also leave the Matrix room")]
        leave: bool,
    },

    #[command(about = "Validate the configuration file")]
    ValidateConfig,

    #[command(about = "Show bridge status")]
    Status,
}

pub fn generate_registration(id: &str, homeserver_url: &str, domain: &str) -> String {
    let as_token = generate_token();
    let hs_token = generate_token();

    let registration = json!({
        "id": id,
        "url": homeserver_url,
        "as_token": as_token,
        "hs_token": hs_token,
        "sender_localpart": "_slack_",
        "rate_limited": false,
        "protocols": ["slack"],
        "namespaces": {
            "users": [{
                "exclusive": true,
                "regex": format!("@_slack_.*:{}", domain)
            }],
            "aliases": [{
                "exclusive": true,
                "regex": format!("#_slack_.*:{}", domain)
            }],
            "rooms": []
        }
    });

    serde_yaml::to_string(&registration).unwrap_or_default()
}

fn generate_token() -> String {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};

    let state = RandomState::new();
    let mut hasher = state.build_hasher();
    hasher.write_u64(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64,
    );
    format!("{:x}", hasher.finish())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_registration_produces_valid_yaml() {
        let yaml = generate_registration("test", "http://localhost:8008", "example.org");
        assert!(yaml.contains("id: test"));
        assert!(yaml.contains("as_token:"));
        assert!(yaml.contains("hs_token:"));
        assert!(yaml.contains("protocols:"));
    }
}
