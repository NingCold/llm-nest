use serde::{Deserialize, Serialize};
use social::{ActorId, ConversationId, MessageId, RunId};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SocialFailure {
    pub code: String,
    pub message: String,
}

/// Transient fragments. These have no durable cursor and are not replayed.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all_fields = "camelCase")]
pub enum SocialGenerationEvent {
    #[serde(rename = "generation.delta")]
    Delta {
        run_id: RunId,
        message_id: MessageId,
        conversation_id: ConversationId,
        actor_id: ActorId,
        delta_seq: u32,
        text: String,
    },
    #[serde(rename = "generation.reasoning_delta")]
    ReasoningDelta {
        run_id: RunId,
        message_id: MessageId,
        conversation_id: ConversationId,
        actor_id: ActorId,
        delta_seq: u32,
        text: String,
    },
    #[serde(rename = "execution.notice")]
    Notice {
        conversation_id: ConversationId,
        error: SocialFailure,
    },
}
