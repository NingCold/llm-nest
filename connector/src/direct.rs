//! Same-device chat contracts. Local execution and cloud history acceptance are distinct.
use crate::node::{
    MaterialScope, NodeAccount, NodeConversation, NodeMessage, NodeUsage, ScopedSource,
    SourceRevision,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

macro_rules! dto { ($name:ident { $( $(#[$field_meta:meta])* $field:ident : $ty:ty),* $(,)? }) => {
    #[derive(Clone, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct $name { $( $(#[$field_meta])* pub $field: $ty),* }
}; }
dto!(DirectGrantRequest {
    node_id: String,
    binding_version: String
});
// Grant and IPC credentials deliberately do not implement Debug.
dto!(DirectGrant { token: String });
dto!(DirectPrepare { grant: String });
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DirectHead {
    pub message_id: String,
    pub revision: String,
    pub position: String,
}
dto!(DirectBootstrap {
    ai: NodeAccount, human: NodeAccount, conversation: NodeConversation,
    binding_version: String, base: Option<DirectHead>, messages: Vec<NodeMessage>,
    #[serde(default)] message_statuses: BTreeMap<String, String>, has_more: bool
});
dto!(DirectMessage {
    id: String, sender_id: String, text: String, status: String, created_at: String,
    reply_to: Option<String>, run_id: Option<String>, usage: Option<NodeUsage>, error: Option<String>
});
dto!(DirectSyncRequest {
    request_id: String, grant: String, conversation_revision: String, base: Option<DirectHead>,
    sources: Vec<SourceRevision>, derived_sources: Vec<ScopedSource>, material_scopes: Vec<MaterialScope>,
    configuration_revision: String, messages: Vec<DirectMessage>
});
dto!(DirectSyncedMessage {
    message: NodeMessage,
    status: String
});
dto!(DirectSyncReceipt { messages: Vec<DirectSyncedMessage>, head: Option<DirectHead> });
dto!(DirectMessageView {
    message: NodeMessage,
    status: String,
    local: bool,
    synced: bool
});
dto!(DirectView {
    ai: NodeAccount, human: NodeAccount, conversation: NodeConversation,
    messages: Vec<DirectMessageView>, has_more: bool, sync_state: String,
    error: Option<String>, running: bool
});
dto!(DirectBinding {
    ai_id: String,
    node_id: String,
    binding_version: String
});
#[derive(Clone, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum LocalOperation {
    Status,
    Prepare {
        ai_id: String,
        grant: String,
    },
    View {
        conversation_id: String,
    },
    Send {
        conversation_id: String,
        request_id: String,
        text: String,
    },
    Stop {
        conversation_id: String,
    },
    Sync {
        conversation_id: String,
    },
}
dto!(LocalRequest {
    owner_id: String,
    operation: LocalOperation
});
dto!(DirectError {
    code: String,
    message: String
});
#[derive(Clone, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum LocalReply {
    Status { bindings: Vec<DirectBinding> },
    View { view: Box<DirectView> },
    Error { error: DirectError },
}

pub mod ipc;
