use common::config::ProviderId;
use common::SessionId;
use provider::error::ProviderError;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum RuntimeError {
    #[error(transparent)]
    IoError(
        #[from] std::io::Error
    ),
    #[error("Provider error: {0}")]
    ProviderError(
        #[from] ProviderError
    ),
    #[error(transparent)]
    TOMLParseError(
        #[from] toml::de::Error
    ),
    #[error("Session not found: {0}")]
    SessionNotFound(
        SessionId
    ),
    #[error("Provider not found: {0}")]
    ProviderNotFound(
        ProviderId
    ),
    #[error("Config error: {0}")]
    ConfigError(
        String
    ),
    #[error("Request cancelled")]
    Cancelled,
}

pub type Result<T> = std::result::Result<T, RuntimeError>;