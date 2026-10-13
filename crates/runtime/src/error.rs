use thiserror::Error;

#[derive(Debug, Error)]
pub enum RuntimeError {
    #[error(transparent)]
    IoError(#[from] std::io::Error),

    #[error("AI client error: {0}")]
    AiError(#[from] ai_client::error::AiError),

    #[error(transparent)]
    TOMLParseError(#[from] toml::de::Error),

    #[error("Config error: {0}")]
    ConfigError(String),

    #[error("Storage error: {0}")]
    Storage(#[from] storage::StorageError),

    #[error("Social storage error: {0}")]
    SocialStorage(#[from] storage::social::SocialStorageError),

    #[error("File watcher error: {0}")]
    WatcherError(#[from] notify::Error),
}

pub type Result<T> = std::result::Result<T, RuntimeError>;
