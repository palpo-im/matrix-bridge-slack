use thiserror::Error;

#[derive(Error, Debug)]
pub enum AppError {
    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Database error: {0}")]
    Database(String),

    #[error("Matrix error: {0}")]
    Matrix(String),

    #[error("Slack error: {0}")]
    Slack(String),

    #[error("Bridge error: {0}")]
    Bridge(String),

    #[error("Web error: {0}")]
    Web(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}
