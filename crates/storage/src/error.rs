use std::path::PathBuf;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum StorageError {
    #[error("storage I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("session serialization error: {0}")]
    Serialize(#[from] serde_json::Error),

    #[error("corrupt session file '{path}': {source}")]
    CorruptFile {
        path: PathBuf,
        source: serde_json::Error,
    },
}

pub type Result<T> = std::result::Result<T, StorageError>;
