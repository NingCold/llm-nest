//! Social persistence, independent of runtime and frontend transports.
mod controls;
mod error;
mod legacy;
mod participation;
mod reads;
mod schema;
mod sqlite;
mod store;
pub use error::{SocialStorageError, SocialStorageResult};
pub use legacy::{LegacyImportResult, LegacyPreview, LegacySessionPreview};
pub use sqlite::SqliteSocialStore;
pub use store::*;
