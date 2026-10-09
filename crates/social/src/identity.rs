use crate::{ActorId, BindingId, RelationshipId, Revision};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActorKind {
    Human,
    Ai,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntityStatus {
    Active,
    Archived,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Actor {
    pub id: ActorId,
    pub kind: ActorKind,
    pub display_name: String,
    pub avatar_ref: Option<String>,
    pub status: EntityStatus,
    pub revision: Revision,
    pub created_at: DateTime<Utc>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AiProfile {
    pub actor_id: ActorId,
    pub persona: String,
    pub revision: Revision,
}
/// Configuration references only. Runtime must validate against its frozen
/// catalog before execution; presence of a stored candidate does not mean ready.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecutionBinding {
    pub id: BindingId,
    pub actor_id: ActorId,
    pub provider_id: String,
    pub model_id: String,
    pub reasoning_effort: Option<String>,
    pub revision: Revision,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Relationship {
    pub id: RelationshipId,
    pub participant_a: ActorId,
    pub participant_b: ActorId,
    pub revision: Revision,
}
