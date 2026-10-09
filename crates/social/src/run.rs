use crate::{BoutId, ContextSnapshot, ConversationId, MessageId, Revision, RunId};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Running,
    Succeeded,
    Failed,
    Cancelled,
    Interrupted,
}
impl RunStatus {
    pub fn is_terminal(self) -> bool {
        self != Self::Running
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BoutStatus {
    Open,
    Ended,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Bout {
    pub id: BoutId,
    pub conversation_id: ConversationId,
    pub status: BoutStatus,
    pub revision: Revision,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AiRun {
    pub id: RunId,
    pub message_id: MessageId,
    pub bout_id: BoutId,
    pub status: RunStatus,
    pub revision: Revision,
    pub snapshot: ContextSnapshot,
}
