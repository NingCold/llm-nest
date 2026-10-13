use std::collections::HashMap;

use ai_client::ModelSelection;
use chrono::{DateTime, Utc};
use common::{Message, SessionId};
use serde::{Deserialize, Serialize};

/// Read-only DTO for importing historical Harness JSON. Never used as live state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionRecord {
    #[serde(default)]
    pub run: Option<common::RunCheckpoint>,
    /// Historical storage format version; records predating the field use version 1.
    #[serde(default = "SessionRecord::default_version")]
    pub version: u32,
    pub id: SessionId,
    pub title: Option<String>,
    pub messages: Vec<Message>,
    pub metadata: HashMap<String, String>,
    /// Historical model selection, retained as import metadata only.
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
