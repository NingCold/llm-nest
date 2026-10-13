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

/// Durable authorization for a response to committed input, ordered within a space.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParticipationPlan {
    #[serde(default)]
    pub automatic: bool,
    #[serde(default)]
    pub input_ids: Vec<MessageId>,
    #[serde(default)]
    pub not_before: Option<DateTime<Utc>>,
    #[serde(default)]
    pub expires_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub policy_revision: Option<Revision>,
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

/// Window-level consent and deterministic limits; independent of model settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParticipationPolicy {
    pub revision: Revision,
    pub merge_delay_ms: u32,
    pub idle_seconds: u32,
    pub limited_auto: bool,
    pub max_consecutive: u32,
    pub max_auto_per_bout: u32,
    pub max_auto_per_day: u32,
    pub daily_token_budget: u32,
}
impl Default for ParticipationPolicy {
    fn default() -> Self {
        Self {
            revision: Revision::ONE,
            merge_delay_ms: 800,
            idle_seconds: 300,
            limited_auto: false,
            max_consecutive: 3,
            max_auto_per_bout: 12,
            max_auto_per_day: 48,
            daily_token_budget: 262_144,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ParticipationReason {
    Waiting,
    InputPending,
    TurnLimit,
    DailyLimit,
    TokenLimit,
    NoCandidate,
    Expired,
    Idle,
    UserEnded,
    PolicyChanged,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParticipationState {
    pub last_activity: Option<DateTime<Utc>>,
    pub consecutive_ai: u32,
    pub bout_auto_turns: u32,
    pub reason: Option<ParticipationReason>,
}
