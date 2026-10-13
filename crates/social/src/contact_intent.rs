use crate::{ActorId, CommandId, ConversationId, IntentId, PlanId, Revision};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QuietAction {
    Defer,
    Skip,
}

/// Explicit consent for this window, separate from reactive group participation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContactSettings {
    pub revision: Revision,
    pub enabled: bool,
    pub quiet_until: Option<DateTime<Utc>>,
    pub quiet_action: QuietAction,
}
impl Default for ContactSettings {
    fn default() -> Self {
        Self {
            revision: Revision::ONE,
            enabled: false,
            quiet_until: None,
            quiet_action: QuietAction::Defer,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContactIntentStatus {
    Pending,
    Claimed,
    Executing,
    Delivered,
    Failed,
    Cancelled,
    Expired,
    Skipped,
    Uncertain,
}
impl ContactIntentStatus {
    pub fn terminal(self) -> bool {
        !matches!(self, Self::Pending | Self::Claimed | Self::Executing)
    }
}

/// Immutable one-shot instruction. Cancellation changes its state, never its authorization.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContactIntent {
    pub id: IntentId,
    pub conversation_id: ConversationId,
    pub actor_id: ActorId,
    pub created_by: ActorId,
    pub authorization_id: CommandId,
    pub text: String,
    pub due_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    /// Fixed offset of the explicitly selected date; not a recurring time-zone rule.
    pub utc_offset_minutes: i32,
    pub status: ContactIntentStatus,
    pub plan_id: Option<PlanId>,
    pub revision: Revision,
    pub reason: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
