//! SQLite social persistence, directory ownership, and explicit historical import.

pub mod data_dir;
pub mod error;
pub mod guard;
pub mod legacy;
pub mod social;

pub use data_dir::{default_data_dir, legacy_data_dir, prepare_data_dir, prepare_default_data_dir};
pub use error::{Result, StorageError};
pub use guard::DataDirectoryGuard;
