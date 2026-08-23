use thiserror::Error;

use crate::reasoning::ReasoningEffort;

#[derive(Debug, Error)]
pub enum AiError {
    #[error(transparent)]
    Reqwest(#[from] reqwest::Error),

    #[error(transparent)]
    Json(#[from] serde_json::Error),

    #[error("{message}")]
    ApiError {
        status: reqwest::StatusCode,
        message: String,
        kind: Option<String>,
        code: Option<String>,
    },

    #[error("Invalid provider response: {0}")]
    InvalidResponse(String),

    #[error("Stream error: {0}")]
    StreamError(String),

    #[error("Configuration error: {0}")]
    ConfigError(String),

    #[error("Provider not found: {0}; available providers: {1}")]
    ProviderNotFound(String, String),

    #[error("Model not found: {0}/{1}; available models: {2}")]
    ModelNotFound(String, String, String),

    #[error(
        "Reasoning effort `{effort:?}` is not supported by {provider}/{model}; supported: {supported}"
    )]
    ReasoningNotSupported {
        provider: String,
        model: String,
        effort: ReasoningEffort,
        supported: String,
    },
}

pub type Result<T> = std::result::Result<T, AiError>;
