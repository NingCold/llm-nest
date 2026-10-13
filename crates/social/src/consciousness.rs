use crate::{
    ActorId, ConversationId, Epoch, MemoryId, MessageId, RelationshipId, Revision, Sequence,
    SummaryId,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextScope {
    pub actor_id: ActorId,
    pub conversation_id: ConversationId,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MemoryScope {
    Actor { actor_id: ActorId },
    Relationship { relationship_id: RelationshipId },
    Conversation { conversation_id: ConversationId },
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SourceRef {
    Message {
        conversation_id: ConversationId,
        message_id: MessageId,
        revision: Revision,
    },
    Memory {
        memory_id: MemoryId,
        revision: Revision,
    },
    Summary {
        summary_id: SummaryId,
        revision: Revision,
    },
    Manual {
        author_id: ActorId,
        revision: Revision,
    },
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VisibilityDependency {
    pub conversation_id: ConversationId,
    pub actor_id: ActorId,
    pub membership_revision: Revision,
    pub context_epoch: Epoch,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextSnapshot {
    pub scope: ContextScope,
    pub profile_revision: Revision,
    pub binding_revision: Revision,
    pub relationship_revision: Option<Revision>,
    pub budget: ContextBudget,
    pub participation_epoch: Epoch,
    pub dependencies: Vec<VisibilityDependency>,
    pub sources: Vec<SourceRef>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextBudget {
    pub input_token_limit: crate::Counter,
    pub reserved_output_tokens: crate::Counter,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextReference {
    pub target: ContextScope,
    pub initiated_by: ActorId,
    pub attached_to: MessageId,
    pub sources: Vec<SourceRef>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MaterialStatus {
    Candidate,
    Confirmed,
    Stale,
    Forgotten,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryRecord {
    pub id: MemoryId,
    pub owner_actor_id: ActorId,
    pub conversation_id: ConversationId,
    pub scope: MemoryScope,
    pub kind: MemoryKind,
    pub text: String,
    pub status: MaterialStatus,
    pub revision: Revision,
    pub sources: Vec<SourceRef>,
    pub created_by: ActorId,
    pub updated_by: ActorId,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryKind {
    Preference,
    Fact,
    SharedExperience,
    OpenTopic,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SummaryBinding {
    pub provider_id: String,
    pub model_id: String,
    pub reasoning_effort: Option<String>,
    pub max_tokens: u32,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SummarySettings {
    pub id: ConversationId,
    pub enabled: bool,
    pub binding: Option<SummaryBinding>,
    pub revision: Revision,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SummaryStatus {
    Queued,
    Running,
    Ready,
    Failed,
    Interrupted,
    Stale,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationSummary {
    pub id: SummaryId,
    pub conversation_id: ConversationId,
    pub from_sequence: Sequence,
    pub through_sequence: Sequence,
    pub text: String,
    pub status: SummaryStatus,
    pub revision: Revision,
    pub sources: Vec<SourceRef>,
    pub binding: SummaryBinding,
    pub settings_revision: Revision,
    pub requested_by: ActorId,
    pub membership_revision: Revision,
    pub participation_epoch: Epoch,
    pub attempt: Revision,
    pub error: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}
