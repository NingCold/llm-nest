use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum SocialStorageError {
    #[error("SQLite error at '{path}': {source}")]
    Sqlite {
        path: PathBuf,
        #[source]
        source: rusqlite::Error,
    },
    #[error("I/O error at '{path}': {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("invalid stored data: {0}")]
    Json(#[from] serde_json::Error),
    #[error("invalid counter: {0}")]
    Counter(#[from] ::social::CounterError),
    #[error("not found: {0}")]
    NotFound(String),
    #[error("conflict: {0}")]
    Conflict(String),
    #[error("permission denied: {0}")]
    Forbidden(String),
    #[error("invalid operation: {0}")]
    Invalid(String),
    #[error("{message}")]
    ParticipationLimit {
        reason: ::social::ParticipationReason,
        message: &'static str,
    },
    #[error("unsupported or inconsistent schema at '{path}': {detail}")]
    Schema { path: PathBuf, detail: String },
    #[error("legacy source '{path}': {detail}")]
    Legacy { path: PathBuf, detail: String },
    #[error("storage worker unavailable")]
    WorkerClosed,
}
pub type SocialStorageResult<T> = Result<T, SocialStorageError>;
