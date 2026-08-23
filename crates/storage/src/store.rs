use common::SessionId;

use crate::error::Result;
use crate::record::SessionRecord;

/// Persistent session store.
///
/// Synchronous by design: the runtime persists sessions write-through on
/// every mutation (create / push / rename / model / delete), and each write
/// is a small JSON file — blocking the caller for microseconds is fine.
pub trait SessionStore: Send + Sync {
    /// Persist `record` (create or full update).
    fn save_session(&self, record: &SessionRecord) -> Result<()>;

    /// Load every persisted session.
    fn load_sessions(&self) -> Result<Vec<SessionRecord>>;

    /// Remove one persisted session. Deleting a session that is not stored
    /// (anymore) is not an error.
    fn delete_session(&self, id: &SessionId) -> Result<()>;
}
