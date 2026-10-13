//! Social persistence, independent of runtime and frontend transports.
mod account_control;
mod accounts;
mod controls;
mod error;
mod legacy;
mod memory;
mod participation;
mod reads;
mod reference;
mod rhythm;
mod schema;
mod sqlite;
mod store;
mod summary;
pub use error::{SocialStorageError, SocialStorageResult};
pub use legacy::{LegacyImportResult, LegacyPreview, LegacySessionPreview};
pub use sqlite::SqliteSocialStore;
pub use store::*;
