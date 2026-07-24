use thiserror::Error;

#[derive(Debug, Error)]
pub enum LlmError {
    #[error("Provider error: {0}")]
    ProviderError(String),

    #[error("Invalid response: {0}")]
    InvalidResponse(String),

    #[error("Serialization error: {0}")]
    Serialization(String),
}

pub type Result<T> = std::result::Result<T, LlmError>;