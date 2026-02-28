use std::path::{Path, PathBuf};

use serde::{Deserialize, Deserializer, Serialize};

use super::ConfigError;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Config {
    pub bridge: BridgeConfig,
    #[serde(default)]
    pub registration: RegistrationConfig,
    pub auth: AuthConfig,
    pub logging: LoggingConfig,
    pub database: DatabaseConfig,
    pub room: RoomConfig,
    pub channel: ChannelConfig,
    #[serde(default)]
    pub limits: LimitsConfig,
    pub ghosts: GhostsConfig,
    #[serde(default)]
    pub metrics: MetricsConfig,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct BridgeConfig {
    pub domain: String,
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default = "default_bind_address")]
    pub bind_address: String,
    #[serde(default)]
    pub homeserver_url: String,
    #[serde(default = "default_presence_interval")]
    pub presence_interval: u64,
    #[serde(default)]
    pub disable_presence: bool,
    #[serde(default)]
    pub disable_typing_notifications: bool,
    #[serde(default)]
    pub disable_slack_mentions: bool,
    #[serde(default)]
    pub disable_deletion_forwarding: bool,
    #[serde(default)]
    pub enable_self_service_bridging: bool,
    #[serde(default)]
    pub disable_portal_bridging: bool,
    #[serde(default)]
    pub disable_read_receipts: bool,
    #[serde(default)]
    pub disable_everyone_mention: bool,
    #[serde(default)]
    pub disable_here_mention: bool,
    #[serde(default)]
    pub disable_join_leave_notifications: bool,
    #[serde(default)]
    pub disable_invite_notifications: bool,
    #[serde(default)]
    pub disable_room_topic_notifications: bool,
    #[serde(default)]
    pub determine_code_language: bool,
    #[serde(default)]
    pub user_limit: Option<u32>,
    #[serde(default)]
    pub admin_mxid: Option<String>,
    #[serde(default = "default_invalid_token_message")]
    pub invalid_token_message: String,
    #[serde(default)]
    pub user_activity: Option<UserActivityConfig>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RegistrationConfig {
    #[serde(alias = "id")]
    pub bridge_id: String,
    #[serde(default, alias = "as_token")]
    pub appservice_token: String,
    #[serde(default, alias = "hs_token")]
    pub homeserver_token: String,
    #[serde(default = "default_sender_localpart")]
    pub sender_localpart: String,
    #[serde(default)]
    pub namespaces: RegistrationNamespaces,
    #[serde(default)]
    pub rate_limited: bool,
    #[serde(
        default = "default_registration_protocols",
        alias = "protocol",
        deserialize_with = "deserialize_registration_protocols"
    )]
    pub protocols: Vec<String>,
}

impl Default for RegistrationConfig {
    fn default() -> Self {
        Self {
            bridge_id: String::new(),
            appservice_token: String::new(),
            homeserver_token: String::new(),
            sender_localpart: default_sender_localpart(),
            namespaces: RegistrationNamespaces::default(),
            rate_limited: false,
            protocols: default_registration_protocols(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, Default, PartialEq, Eq)]
pub struct RegistrationNamespaces {
    #[serde(default)]
    pub users: Vec<RegistrationNamespaceEntry>,
    #[serde(default)]
    pub aliases: Vec<RegistrationNamespaceEntry>,
    #[serde(default)]
    pub rooms: Vec<RegistrationNamespaceEntry>,
}

impl RegistrationNamespaces {
    fn is_empty(&self) -> bool {
        self.users.is_empty() && self.aliases.is_empty() && self.rooms.is_empty()
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, Default, PartialEq, Eq)]
pub struct RegistrationNamespaceEntry {
    #[serde(default)]
    pub exclusive: bool,
    #[serde(default)]
    pub regex: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct UserActivityConfig {
    #[serde(default)]
    pub min_user_active_days: u64,
    #[serde(default)]
    pub inactive_after_days: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AuthConfig {
    pub bot_token: String,
    #[serde(default)]
    pub app_token: Option<String>,
    #[serde(default)]
    pub client_id: Option<String>,
    #[serde(default)]
    pub client_secret: Option<String>,
    #[serde(default = "default_use_privileged_intents")]
    pub use_privileged_intents: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LoggingConfig {
    #[serde(alias = "console", default = "default_log_level")]
    pub level: String,
    #[serde(default = "default_line_date_format")]
    pub line_date_format: String,
    #[serde(default = "default_log_format")]
    pub format: String,
    #[serde(default)]
    pub file: Option<String>,
    #[serde(default)]
    pub files: Vec<LoggingFileConfig>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LoggingFileConfig {
    pub file: String,
    #[serde(default = "default_log_file_level")]
    pub level: String,
    #[serde(default = "default_log_max_files")]
    pub max_files: String,
    #[serde(default = "default_log_max_size")]
    pub max_size: String,
    #[serde(default = "default_log_date_pattern")]
    pub date_pattern: String,
    #[serde(default)]
    pub enabled: Vec<String>,
    #[serde(default)]
    pub disabled: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DatabaseConfig {
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub conn_string: Option<String>,
    #[serde(default)]
    pub filename: Option<String>,
    #[serde(default)]
    pub user_store_path: Option<String>,
    #[serde(default)]
    pub room_store_path: Option<String>,
    #[serde(default)]
    pub max_connections: Option<u32>,
    #[serde(default)]
    pub min_connections: Option<u32>,
}

impl DatabaseConfig {
    pub fn db_type(&self) -> DbType {
        let url = self.connection_string();
        if url.starts_with("sqlite://") {
            DbType::Sqlite
        } else if url.starts_with("mysql://") || url.starts_with("mariadb://") {
            DbType::Mysql
        } else {
            DbType::Postgres
        }
    }

    pub fn connection_string(&self) -> String {
        if let Some(ref url) = self.url {
            url.clone()
        } else if let Some(ref conn) = self.conn_string {
            conn.clone()
        } else if let Some(ref file) = self.filename {
            format!("sqlite://{}", file)
        } else {
            String::new()
        }
    }

    pub fn sqlite_path(&self) -> Option<String> {
        if let DbType::Sqlite = self.db_type() {
            let url = self.connection_string();
            Some(url.strip_prefix("sqlite://").unwrap_or(&url).to_string())
        } else {
            None
        }
    }

    pub fn max_connections(&self) -> Option<u32> {
        match self.db_type() {
            DbType::Postgres | DbType::Mysql => self.max_connections,
            DbType::Sqlite => Some(1),
        }
    }

    pub fn min_connections(&self) -> Option<u32> {
        match self.db_type() {
            DbType::Postgres | DbType::Mysql => self.min_connections,
            DbType::Sqlite => Some(1),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DbType {
    Postgres,
    Sqlite,
    Mysql,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RoomConfig {
    #[serde(default)]
    pub default_visibility: String,
    #[serde(default)]
    pub room_alias_prefix: String,
    #[serde(default)]
    pub enable_room_creation: bool,
    #[serde(default = "default_kick_for")]
    pub kick_for: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ChannelConfig {
    #[serde(default = "default_channel_name_pattern")]
    pub name_pattern: String,
    #[serde(default)]
    pub enable_channel_creation: bool,
    #[serde(default)]
    pub channel_name_format: String,
    #[serde(default)]
    pub topic_format: String,
    #[serde(default)]
    pub delete_options: ChannelDeleteOptionsConfig,
    #[serde(default = "default_enable_webhook")]
    pub enable_webhook: bool,
    #[serde(default = "default_webhook_name")]
    pub webhook_name: String,
    #[serde(default = "default_webhook_avatar")]
    pub webhook_avatar: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ChannelDeleteOptionsConfig {
    #[serde(default)]
    pub name_prefix: Option<String>,
    #[serde(default)]
    pub topic_prefix: Option<String>,
    #[serde(default)]
    pub disable_messaging: bool,
    #[serde(default = "default_unset_room_alias")]
    pub unset_room_alias: bool,
    #[serde(default = "default_unlist_from_directory")]
    pub unlist_from_directory: bool,
    #[serde(default = "default_set_invite_only")]
    pub set_invite_only: bool,
    #[serde(default = "default_ghosts_leave")]
    pub ghosts_leave: bool,
}

impl Default for ChannelDeleteOptionsConfig {
    fn default() -> Self {
        Self {
            name_prefix: None,
            topic_prefix: None,
            disable_messaging: false,
            unset_room_alias: true,
            unlist_from_directory: true,
            set_invite_only: true,
            ghosts_leave: true,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LimitsConfig {
    #[serde(default = "default_room_ghost_join_delay")]
    pub room_ghost_join_delay: u64,
    #[serde(default = "default_slack_send_delay", alias = "slack_send_delay")]
    pub slack_send_delay: u64,
    #[serde(default = "default_room_count")]
    pub room_count: i32,
    #[serde(default = "default_matrix_event_age_limit_ms")]
    pub matrix_event_age_limit_ms: u64,
}

impl Default for LimitsConfig {
    fn default() -> Self {
        Self {
            room_ghost_join_delay: 6000,
            slack_send_delay: 1500,
            room_count: -1,
            matrix_event_age_limit_ms: 900_000,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct GhostsConfig {
    #[serde(default = "default_nick_pattern")]
    pub nick_pattern: String,
    #[serde(default = "default_username_pattern")]
    pub username_pattern: String,
    #[serde(default)]
    pub username_template: String,
    #[serde(default)]
    pub displayname_template: String,
    #[serde(default)]
    pub avatar_url_template: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct MetricsConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_metrics_port")]
    pub port: u16,
    #[serde(default = "default_metrics_bind_address")]
    pub bind_address: String,
}

impl Config {
    pub fn load() -> Result<Self, ConfigError> {
        let config_path = std::env::var("CONFIG_PATH")
            .ok()
            .or_else(|| Some("config.yaml".to_string()))
            .unwrap();

        Self::load_from_file(&config_path)
    }

    pub fn load_from_file<P: AsRef<Path>>(path: P) -> Result<Self, ConfigError> {
        let content = std::fs::read_to_string(&path)?;
        let mut config: Config = serde_yaml::from_str(&content)?;
        config.apply_env_overrides();
        config.normalize();
        config.load_registration(path.as_ref())?;
        config.validate()?;
        Ok(config)
    }

    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.bridge.domain.is_empty() {
            return Err(ConfigError::InvalidConfig(
                "bridge.domain cannot be empty".to_string(),
            ));
        }

        if self.registration.bridge_id.is_empty() {
            return Err(ConfigError::InvalidConfig(
                "registration id cannot be empty (set registration.id or provide slack-registration.yaml)"
                    .to_string(),
            ));
        }

        if self.registration.appservice_token.is_empty() {
            return Err(ConfigError::InvalidConfig(
                "registration as_token cannot be empty (set registration.as_token or provide slack-registration.yaml)"
                    .to_string(),
            ));
        }

        if self.registration.homeserver_token.is_empty() {
            return Err(ConfigError::InvalidConfig(
                "registration hs_token cannot be empty (set registration.hs_token or provide slack-registration.yaml)"
                    .to_string(),
            ));
        }

        if self.auth.bot_token.is_empty() {
            return Err(ConfigError::InvalidConfig(
                "auth.bot_token cannot be empty".to_string(),
            ));
        }
        if looks_like_placeholder_bot_token(&self.auth.bot_token) {
            return Err(ConfigError::InvalidConfig(
                "auth.bot_token is still using a placeholder value; set a real Slack bot token"
                    .to_string(),
            ));
        }
        if self
            .auth
            .app_token
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .is_none()
            && self
                .auth
                .client_secret
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .is_none()
        {
            return Err(ConfigError::InvalidConfig(
                "auth.app_token cannot be empty for Slack Socket Mode (xapp- token)".to_string(),
            ));
        }

        if self.database.connection_string().is_empty() {
            return Err(ConfigError::InvalidConfig(
                "database connection string cannot be empty".to_string(),
            ));
        }

        if self.bridge.port == 0 {
            return Err(ConfigError::InvalidConfig(
                "bridge.port must be between 1 and 65535".to_string(),
            ));
        }

        Ok(())
    }

    fn normalize(&mut self) {
        self.auth.bot_token = sanitize_bot_token(&self.auth.bot_token);
    }

    fn apply_env_overrides(&mut self) {
        if let Ok(value) = std::env::var("APPSERVICE_SLACK_AUTH_BOT_TOKEN")
            .or_else(|_| std::env::var("APPSERVICE_slack_AUTH_BOT_TOKEN"))
        {
            self.auth.bot_token = sanitize_bot_token(&value);
        }
        if let Ok(value) = std::env::var("APPSERVICE_SLACK_AUTH_APP_TOKEN") {
            self.auth.app_token = Some(value);
        }
        if let Ok(value) = std::env::var("APPSERVICE_SLACK_AUTH_CLIENT_ID")
            .or_else(|_| std::env::var("APPSERVICE_slack_AUTH_CLIENT_ID"))
        {
            self.auth.client_id = Some(value);
        }
        if let Ok(value) = std::env::var("APPSERVICE_SLACK_AUTH_CLIENT_SECRET")
            .or_else(|_| std::env::var("APPSERVICE_slack_AUTH_CLIENT_SECRET"))
        {
            self.auth.client_secret = Some(value);
        }
        if let Ok(value) = std::env::var("APPSERVICE_SLACK_REGISTRATION_ID")
            .or_else(|_| std::env::var("APPSERVICE_slack_REGISTRATION_ID"))
        {
            self.registration.bridge_id = value;
        }
        if let Ok(value) = std::env::var("APPSERVICE_SLACK_REGISTRATION_AS_TOKEN")
            .or_else(|_| std::env::var("APPSERVICE_slack_REGISTRATION_AS_TOKEN"))
        {
            self.registration.appservice_token = value;
        }
        if let Ok(value) = std::env::var("APPSERVICE_SLACK_REGISTRATION_HS_TOKEN")
            .or_else(|_| std::env::var("APPSERVICE_slack_REGISTRATION_HS_TOKEN"))
        {
            self.registration.homeserver_token = value;
        }
        if let Ok(value) = std::env::var("APPSERVICE_SLACK_REGISTRATION_SENDER_LOCALPART")
            .or_else(|_| std::env::var("APPSERVICE_slack_REGISTRATION_SENDER_LOCALPART"))
        {
            self.registration.sender_localpart = value;
        }
    }

    fn load_registration(&mut self, config_path: &Path) -> Result<(), ConfigError> {
        let registration_path = std::env::var("REGISTRATION_PATH")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(default_registration_file);
        let registration_path = resolve_registration_path(config_path, &registration_path);

        if !registration_path.exists() {
            return Ok(());
        }

        let content = std::fs::read_to_string(registration_path)?;
        let registration: RegistrationConfig = serde_yaml::from_str(&content)?;

        if self.registration.bridge_id.is_empty() {
            self.registration.bridge_id = registration.bridge_id;
        }
        if self.registration.appservice_token.is_empty() {
            self.registration.appservice_token = registration.appservice_token;
        }
        if self.registration.homeserver_token.is_empty() {
            self.registration.homeserver_token = registration.homeserver_token;
        }
        if self.registration.sender_localpart == default_sender_localpart()
            && registration.sender_localpart != default_sender_localpart()
        {
            self.registration.sender_localpart = registration.sender_localpart;
        }
        if self.registration.namespaces.is_empty() && !registration.namespaces.is_empty() {
            self.registration.namespaces = registration.namespaces;
        }
        if !self.registration.rate_limited && registration.rate_limited {
            self.registration.rate_limited = true;
        }
        if self.registration.protocols == default_registration_protocols()
            && registration.protocols != default_registration_protocols()
        {
            self.registration.protocols = registration.protocols;
        }

        Ok(())
    }
}

fn default_port() -> u16 {
    9005
}

fn default_bind_address() -> String {
    "0.0.0.0".to_string()
}

fn default_registration_file() -> String {
    "slack-registration.yaml".to_string()
}

fn resolve_registration_path(config_path: &Path, registration_path: &str) -> PathBuf {
    let registration_path = Path::new(registration_path);
    if registration_path.is_absolute() {
        registration_path.to_path_buf()
    } else if let Some(parent) = config_path.parent() {
        parent.join(registration_path)
    } else {
        registration_path.to_path_buf()
    }
}

fn default_sender_localpart() -> String {
    "_slack_".to_string()
}

fn default_registration_protocols() -> Vec<String> {
    vec!["slack".to_string()]
}

fn deserialize_registration_protocols<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum ProtocolValue {
        Single(String),
        Multiple(Vec<String>),
    }

    match ProtocolValue::deserialize(deserializer)? {
        ProtocolValue::Single(protocol) => Ok(vec![protocol]),
        ProtocolValue::Multiple(protocols) => Ok(protocols),
    }
}

fn default_presence_interval() -> u64 {
    500
}

fn default_invalid_token_message() -> String {
    "Your Slack bot token seems to be invalid, and the bridge cannot function. Please update it in your bridge settings and restart the bridge".to_string()
}

fn default_use_privileged_intents() -> bool {
    false
}

fn default_log_level() -> String {
    "info".to_string()
}

fn default_line_date_format() -> String {
    "MMM-D HH:mm:ss.SSS".to_string()
}

fn default_log_format() -> String {
    "pretty".to_string()
}

fn default_log_file_level() -> String {
    "info".to_string()
}

fn default_log_max_files() -> String {
    "14d".to_string()
}

fn default_log_max_size() -> String {
    "50m".to_string()
}

fn default_log_date_pattern() -> String {
    "YYYY-MM-DD".to_string()
}

fn default_kick_for() -> u64 {
    30000
}

fn default_channel_name_pattern() -> String {
    "[Slack] :guild :name".to_string()
}

fn default_unset_room_alias() -> bool {
    true
}

fn default_unlist_from_directory() -> bool {
    true
}

fn default_set_invite_only() -> bool {
    true
}

fn default_ghosts_leave() -> bool {
    true
}

fn default_room_ghost_join_delay() -> u64 {
    6000
}

fn default_slack_send_delay() -> u64 {
    1500
}

fn default_room_count() -> i32 {
    -1
}

fn default_matrix_event_age_limit_ms() -> u64 {
    900_000
}

fn default_nick_pattern() -> String {
    ":nick".to_string()
}

fn default_username_pattern() -> String {
    ":username#:tag".to_string()
}

fn default_metrics_port() -> u16 {
    9001
}

fn default_metrics_bind_address() -> String {
    "127.0.0.1".to_string()
}

fn default_enable_webhook() -> bool {
    true
}

fn default_webhook_name() -> String {
    "_matrix".to_string()
}

fn default_webhook_avatar() -> String {
    "https://matrix.org/_matrix/media/r0/download/matrix.org/mlxoESwIsTbJrfXyAAogrNxA".to_string()
}

fn sanitize_bot_token(token: &str) -> String {
    let trimmed = token.trim();
    let without_prefix = trimmed
        .strip_prefix("Bot ")
        .or_else(|| trimmed.strip_prefix("bot "))
        .unwrap_or(trimmed);
    without_prefix.trim().to_string()
}

fn looks_like_placeholder_bot_token(token: &str) -> bool {
    let lower = token.trim().to_ascii_lowercase();
    lower == "your_slack_bot_token"
        || lower == "your_bot_token_here"
        || lower == "your_bot_token"
        || lower == "your-token-here"
        || lower == "changeme"
}

#[cfg(test)]
mod tests {
    use super::{looks_like_placeholder_bot_token, sanitize_bot_token};

    #[test]
    fn sanitize_bot_token_removes_prefix_and_whitespace() {
        let sanitized = sanitize_bot_token("  Bot abc123  ");
        assert_eq!(sanitized, "abc123");
    }

    #[test]
    fn placeholder_detection_matches_common_values() {
        assert!(looks_like_placeholder_bot_token("YOUR_SLACK_BOT_TOKEN"));
        assert!(looks_like_placeholder_bot_token("your_bot_token_here"));
        assert!(!looks_like_placeholder_bot_token("mfa.x.y"));
    }
}
