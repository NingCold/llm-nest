use ai_client::config::ProviderId;
use common::SessionId;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum RuntimeError {
    #[error(transparent)]
    IoError(#[from] std::io::Error),

    #[error("AI client error: {0}")]
    AiError(#[from] ai_client::error::AiError),

    #[error(transparent)]
    TOMLParseError(#[from] toml::de::Error),

    #[error("Session not found: {0}")]
    SessionNotFound(SessionId),

    #[error("Provider not found: {0}")]
    ProviderNotFound(ProviderId),

    #[error("Config error: {0}")]
    ConfigError(String),

    #[error("Storage error: {0}")]
    Storage(#[from] storage::StorageError),

    #[error("Feature not found: {0}")]
    FeatureNotFound(String),

    #[error("Request cancelled")]
    Cancelled,

    #[error("File watcher error: {0}")]
    WatcherError(#[from] notify::Error),
}

pub type Result<T> = std::result::Result<T, RuntimeError>;
