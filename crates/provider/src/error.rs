use thiserror::Error;

#[derive(Debug, Error)]
pub enum ProviderError {
    #[error(transparent)]
    Reqwest(
        #[from] reqwest::Error
    ),

    #[error(transparent)]
    Json(
        #[from] serde_json::Error
    ),
    
    #[error("{message}")]
    ApiError {
        status: reqwest::StatusCode,
        message: String,
        kind: Option<String>,
        code: Option<String>,
    },

    #[error("Invalid provider response: {0}")]
    InvalidResponse(String),
}

pub type Result<T> = std::result::Result<T, ProviderError>;