use std::collections::HashMap;

use ai_client::ModelSelection;
use chrono::{DateTime, Utc};
use common::{Message, SessionId};
use serde::{Deserialize, Serialize};

/// Full persisted snapshot of one session.
///
/// The record is a dedicated storage DTO (not `runtime::Session`) so the
/// storage crate stays free of runtime types; `runtime` converts both ways.
/// Fields mirror `runtime::session::Session` 1:1.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionRecord {
    /// Storage format version. Always `VERSION` for new writes; `#[serde(default)]`
    /// lets older files without the field keep loading.
    #[serde(default = "SessionRecord::default_version")]
    pub version: u32,
    pub id: SessionId,
    pub title: Option<String>,
    pub messages: Vec<Message>,
    pub metadata: HashMap<String, String>,
    /// Model selection the session remembers; `None` means "use the global
    /// default at request time".
    pub model: Option<ModelSelection>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl SessionRecord {
    pub const VERSION: u32 = 1;

    fn default_version() -> u32 {
        Self::VERSION
    }
}
