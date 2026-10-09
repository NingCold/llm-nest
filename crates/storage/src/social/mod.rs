//! M0 foundation, deliberately not activated by Runtime or GUI yet.
mod error;
mod legacy;
mod schema;
mod sqlite;
mod store;
pub use error::{SocialStorageError, SocialStorageResult};
pub use legacy::{LegacyImportResult, LegacyPreview, LegacySessionPreview};
pub use sqlite::SqliteSocialStore;
pub use store::*;
