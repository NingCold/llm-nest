//! Versioned local-to-cloud migration boundary. No execution configuration or credentials.
use serde::{Deserialize, Serialize};

macro_rules! dto { ($name:ident { $($field:ident : $ty:ty),* $(,)? }) => {
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct $name { $(pub $field: $ty),* }
}; }
dto!(MigrationSnapshot {
    version: u32, source_id: String, source_human_id: String,
    actors: Vec<MigrationActor>, contacts: Vec<MigrationContact>,
    conversations: Vec<MigrationConversation>, messages: Vec<MigrationMessage>,
    list_state: Vec<MigrationListState>, deferred: Vec<MigrationDeferred>
});
dto!(MigrationActor {
    id: String,
    kind: String,
    name: String,
    bio: String,
    status: String,
    revision: String
});
dto!(MigrationContact {
    owner_id: String,
    target_id: String,
    listed: bool,
    blocked: bool
});
dto!(MigrationConversation {
    id: String, kind: String, name: String, revision: String,
    archived: bool, legacy: bool, members: Vec<MigrationMember>
});
dto!(MigrationMember {
    actor_id: String,
    role: String,
    left: bool
});
dto!(MigrationMessage {
    id: String, conversation_id: String, sender_id: String, sender_name: String,
    sequence: String, revision: String, status: String, created_at: String,
    text: String, attachments: Vec<MigrationAttachment>, reply_to: Option<String>,
    mentions: Vec<String>, reference: Option<MigrationReference>
});
dto!(MigrationAttachment { source_index: usize, kind: String, name: Option<String>, mime: String, data: String });
dto!(MigrationReference { conversation_id: String, sources: Vec<MigrationSource>, stale: bool });
dto!(MigrationSource {
    message_id: String,
    revision: String
});
// Read positions are exported through message_deliveries, never messages.sequence.
dto!(MigrationListState { owner_id: String, conversation_id: String, pinned: bool, manual_unread: bool, read_message_ids: Vec<String> });
// Preview-only old participation settings and future intents; never scheduled by import.
dto!(MigrationDeferred {
    kind: String,
    conversation_id: String,
    description: String
});
dto!(MigrationImport {
    snapshot: MigrationSnapshot,
    preview_digest: String
});
dto!(MigrationPreview { snapshot_digest: String, preview_digest: String, target_human_id: String, new_ai_ids: Vec<String>, warnings: Vec<String>, already_imported: bool });
dto!(MigrationMapping { import_id: String, source_id: String, snapshot_digest: String, actors: Vec<MigrationEntityMapping>, conversations: Vec<MigrationEntityMapping>, messages: Vec<MigrationMessageMapping> });
dto!(MigrationEntityMapping {
    source_id: String,
    target_id: String,
    source_revision: String,
    target_revision: String
});
dto!(MigrationMessageMapping { source_id: String, target_id: String, source_revision: String, target_revision: String, target_position: String, attachments: Vec<MigrationAttachmentMapping> });
dto!(MigrationAttachmentMapping {
    source_index: usize,
    target_index: usize
});
dto!(ImportProvenance {
    import_id: String,
    source_message_id: String,
    source_revision: String
});
