//! Ciphertext and routing contracts. Cryptographic state belongs to endpoints.
use serde::{Deserialize, Serialize};

/// Opaque attachment upload. Original names, types and keys are sealed in the message.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EncryptedAttachment {
    pub index: usize,
    pub data: String,
}
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SealedEnvelope {
    pub version: u32,
    pub room_id: String,
    pub sender: String,
    pub device_id: String,
    pub event_id: String,
    pub content: Value,
    pub device_keys: Value,
    pub signatures: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationEncryption {
    pub version: u32,
    pub activation_position: String,
    #[serde(default)]
    pub history_complete: bool,
}

/// Attests who encrypted an existing record; it is not the original author's signature.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryMigration {
    pub id: String,
    pub actor_id: String,
    pub source_digest: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CryptoRequest {
    pub device_id: String,
    pub request_id: String,
    pub kind: String,
    pub body: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Resource {
    pub id: String,
    pub ai_id: String,
    pub conversation_id: Option<String>,
    pub kind: String,
    pub revision: String,
    pub sealed: Option<SealedEnvelope>,
    pub sources: Vec<crate::node::SourceRevision>,
    pub tombstone: Option<String>,
    pub applied: Option<ResourceApplied>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceApplied {
    pub binding_version: String,
    pub revision: String,
    pub status: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResourceList {
    pub items: Vec<Resource>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResourceParticipants {
    pub accounts: Vec<String>,
    pub revision: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveResource {
    pub request_id: String,
    pub device_id: String,
    pub ai_id: String,
    pub conversation_id: Option<String>,
    pub kind: String,
    pub expected_revision: String,
    pub expected_audience_revision: String,
    pub sealed: Option<SealedEnvelope>,
    pub sources: Vec<crate::node::SourceRevision>,
    pub tombstone: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyResource {
    pub device_id: String,
    pub revision: String,
    pub status: String,
}

pub fn user_id(account_id: &str) -> String {
    format!("@{account_id}:llmn")
}

pub fn room_id(conversation_id: &str) -> String {
    format!("!{conversation_id}:llmn")
}

pub fn resource_room(ai: &str, kind: &str, conversation: Option<&str>) -> String {
    format!("!{ai}.{kind}.{}:llmn", conversation.unwrap_or("global"))
}
