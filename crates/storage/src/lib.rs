//! Session persistence: a storage-format-agnostic [`SessionStore`] trait, a
//! JSON-file backed implementation ([`FileSessionStore`]), and the persisted
//! session DTO ([`SessionRecord`]).

pub mod data_dir;
pub mod error;
pub mod file;
pub mod record;
pub mod store;

pub use data_dir::default_data_dir;
pub use error::{Result, StorageError};
pub use file::FileSessionStore;
pub use record::SessionRecord;
pub use store::SessionStore;
