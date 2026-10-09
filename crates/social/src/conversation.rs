use crate::{ActorId, ConversationId, EntityStatus, Epoch, RelationshipId, Revision, Sequence};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConversationKind {
    Direct,
    Group,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Conversation {
    pub id: ConversationId,
    pub kind: ConversationKind,
    pub relationship_id: Option<RelationshipId>,
    pub title: String,
    pub status: EntityStatus,
    pub paused: bool,
    pub revision: Revision,
    pub context_epoch: Epoch,
    pub participation_epoch: Epoch,
    pub last_sequence: Sequence,
    pub created_at: DateTime<Utc>,
    pub legacy: bool,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MembershipStatus {
    Active,
    Left,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Membership {
    pub conversation_id: ConversationId,
    pub actor_id: ActorId,
    pub status: MembershipStatus,
    pub revision: Revision,
}
