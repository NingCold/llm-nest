//! Same-device chat contracts. Local execution and cloud history acceptance are distinct.
use crate::node::{
    MaterialScope, NodeAccount, NodeAttachment, NodeContextReference, NodeConversation,
    NodeMessage, NodeReference, NodeUsage, ReferenceSelection, ScopedSource, SourceRevision,
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
dto!(DirectReferenceRequest {
    grant: String,
    selection: ReferenceSelection
});
dto!(DirectAttachmentRequest {
    grant: String,
    conversation_id: String,
    message_id: String,
    index: usize,
    revision: String
});
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
    #[serde(default)] message_statuses: BTreeMap<String, String>, has_more: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")] references: Vec<NodeReference>
});
dto!(DirectMessage {
    id: String, sender_id: String, text: String, status: String, created_at: String,
    reply_to: Option<String>, run_id: Option<String>, usage: Option<NodeUsage>, error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    sealed: Option<crate::crypto::SealedEnvelope>,
    #[serde(default, skip_serializing_if = "Option::is_none")] audience_revision: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")] context_reference: Option<NodeContextReference>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")] mentions: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")] attachments: Vec<NodeAttachment>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    encrypted_attachments: Vec<crate::crypto::EncryptedAttachment>
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
    error: Option<String>, running: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    key_messages: Vec<serde_json::Value>
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
// One command is handled at a time; keep the wire DTO directly inspectable.
#[allow(clippy::large_enum_variant)]
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
        #[serde(default, skip_serializing_if = "Option::is_none")]
        sealed: Option<crate::crypto::SealedEnvelope>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        message_id: Option<String>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        key_messages: Vec<serde_json::Value>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        encrypted_attachments: Vec<crate::crypto::EncryptedAttachment>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reply_to: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reference: Option<ReferenceSelection>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        expected_conversation_revision: Option<String>,
    },
    PrepareReference {
        conversation_id: String,
        selection: ReferenceSelection,
    },
    Attachment {
        conversation_id: String,
        message_id: String,
        index: usize,
        revision: String,
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
// Matches the command DTO convention above; the payload is serialized at the IPC boundary.
#[allow(clippy::large_enum_variant)]
pub enum LocalReply {
    Status { bindings: Vec<DirectBinding> },
    View { view: Box<DirectView> },
    Reference { reference: NodeReference },
    Attachment { data: String },
    Error { error: DirectError },
}

pub mod ipc;
