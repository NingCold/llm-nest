use crate::{ActorId, ConversationId, MessageId, PlanId, Revision, RunId, Sequence};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanStatus {
    Queued,
    Running,
    Succeeded,
    Failed,
    Cancelled,
    Interrupted,
}

/// Durable authorization to respond to one human message, ordered within a space.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParticipationPlan {
    pub id: PlanId,
    pub conversation_id: ConversationId,
    pub actor_id: ActorId,
    pub trigger_id: MessageId,
    pub trigger_sequence: Sequence,
    pub ordinal: u32,
    pub membership_revision: Revision,
    pub status: PlanStatus,
    pub run_id: Option<RunId>,
    pub failure: Option<String>,
    pub revision: Revision,
    pub created_at: DateTime<Utc>,
}
