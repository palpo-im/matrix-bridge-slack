use thiserror::Error;

#[derive(Error, Debug)]
pub enum ConfigError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("YAML parsing error: {0}")]
    Yaml(#[from] serde_yaml::Error),

    #[error("Invalid configuration: {0}")]
    InvalidConfig(String),

    #[error("Environment variable error: {0}")]
    EnvVar(#[from] std::env::VarError),
}
