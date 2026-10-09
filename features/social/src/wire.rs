//! Shared business DTOs. HTTP and IPC only select their transport.
use serde::{Deserialize, Serialize};
use social::*;
use storage::social::{BindingCandidate, ContactRecord, ConversationRecord, RunFailure};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Binding {
    pub provider_id: String,
    pub model_id: String,
    pub reasoning_effort: Option<String>,
}
impl From<Binding> for BindingCandidate {
    fn from(b: Binding) -> Self {
        Self {
            provider_id: b.provider_id,
            model_id: b.model_id,
            reasoning_effort: b.reasoning_effort,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Contact {
    pub id: ActorId,
    pub name: String,
    pub persona: String,
    pub revision: Revision,
    pub binding: Option<Binding>,
}
impl From<ContactRecord> for Contact {
    fn from(c: ContactRecord) -> Self {
        Self {
            id: c.actor.id,
            name: c.actor.display_name,
            persona: c.profile.persona,
            revision: c.actor.revision,
            binding: c.binding.map(|b| Binding {
                provider_id: b.provider_id,
                model_id: b.model_id,
                reasoning_effort: b.reasoning_effort,
            }),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Message {
    pub id: MessageId,
    pub conversation_id: ConversationId,
    pub sender_id: ActorId,
    pub sender_name: String,
    pub sequence: Sequence,
    pub text: String,
    pub status: MessageStatus,
    pub revision: Revision,
    pub mentions: Vec<ActorId>,
    pub reply_to: Option<MessageId>,
    pub run_id: Option<RunId>,
    pub created_at: String,
}
impl From<SocialMessage> for Message {
    fn from(m: SocialMessage) -> Self {
        Self {
            id: m.id,
            conversation_id: m.conversation_id,
            sender_id: m.sender_id,
            sender_name: m.sender_name,
            sequence: m.sequence,
            text: m
                .body
                .0
                .into_iter()
                .filter_map(|p| match p {
                    SocialContent::Text { text } => Some(text),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("\n"),
            status: m.status,
            revision: m.revision,
            mentions: m.mentions,
            reply_to: m.reply_to,
            run_id: m.run_id,
            created_at: m.created_at.to_rfc3339(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Run {
    pub id: RunId,
    pub conversation_id: ConversationId,
    pub actor_id: ActorId,
    pub message_id: MessageId,
    pub status: RunStatus,
    pub revision: Revision,
}
impl From<AiRun> for Run {
    fn from(r: AiRun) -> Self {
        Self {
            id: r.id,
            conversation_id: r.snapshot.scope.conversation_id,
            actor_id: r.snapshot.scope.actor_id,
            message_id: r.message_id,
            status: r.status,
            revision: r.revision,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Conversation {
    pub id: ConversationId,
    pub kind: ConversationKind,
    pub title: String,
    pub member_ids: Vec<ActorId>,
    pub members: Vec<Member>,
    pub pending_plans: Vec<Plan>,
    pub status: EntityStatus,
    pub paused: bool,
    pub revision: Revision,
    pub last_sequence: Sequence,
    pub legacy: bool,
    pub latest_message: Option<Message>,
    pub active_run: Option<Run>,
}
impl From<ConversationRecord> for Conversation {
    fn from(c: ConversationRecord) -> Self {
        Self {
            id: c.conversation.id,
            kind: c.conversation.kind,
            title: c.conversation.title,
            member_ids: c.members.iter().map(|m| m.actor_id).collect(),
            members: c.members.into_iter().map(Into::into).collect(),
            pending_plans: c.pending_plans.into_iter().map(Into::into).collect(),
            status: c.conversation.status,
            paused: c.conversation.paused,
            revision: c.conversation.revision,
            last_sequence: c.conversation.last_sequence,
            legacy: c.conversation.legacy,
            latest_message: c.latest_message.map(Into::into),
            active_run: c.active_run.map(Into::into),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Member {
    pub actor_id: ActorId,
    pub revision: Revision,
}
impl From<Membership> for Member {
    fn from(m: Membership) -> Self {
        Self {
            actor_id: m.actor_id,
            revision: m.revision,
        }
    }
}
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Plan {
    pub id: PlanId,
    pub actor_id: ActorId,
    pub trigger_id: MessageId,
    pub trigger_sequence: Sequence,
    pub ordinal: u32,
    pub status: PlanStatus,
    pub run_id: Option<RunId>,
    pub failure: Option<String>,
    pub revision: Revision,
}
impl From<ParticipationPlan> for Plan {
    fn from(p: ParticipationPlan) -> Self {
        Self {
            id: p.id,
            actor_id: p.actor_id,
            trigger_id: p.trigger_id,
            trigger_sequence: p.trigger_sequence,
            ordinal: p.ordinal,
            status: p.status,
            run_id: p.run_id,
            failure: p.failure,
            revision: p.revision,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelOption {
    pub provider_id: String,
    pub model_id: String,
    pub display_name: String,
    pub reasoning_levels: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Init {
    pub self_id: ActorId,
    pub self_name: String,
    pub contacts: Vec<Contact>,
    pub conversations: Vec<Conversation>,
    pub models: Vec<ModelOption>,
    pub cursor: EventCursor,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContactInput {
    pub request_id: CommandId,
    pub name: String,
    pub persona: String,
    pub binding: Option<Binding>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateContactInput {
    pub request_id: CommandId,
    pub name: String,
    pub persona: String,
    pub binding: Option<Binding>,
    pub expected_revision: Revision,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Command {
    pub request_id: CommandId,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SendInput {
    pub request_id: CommandId,
    #[serde(default)]
    pub mentions: Vec<ActorId>,
    pub text: String,
    pub reply_to: Option<MessageId>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SendReceipt {
    pub message: Message,
    /// Message persistence and AI availability are independent outcomes.
    pub execution_notice: Option<RunFailure>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MessagePage {
    pub messages: Vec<Message>,
    pub has_more: bool,
}

pub use events::SocialGenerationEvent as LiveEvent;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ErrorBody {
    pub code: &'static str,
    pub message: String,
    pub retryable: bool,
}
impl crate::SocialError {
    pub fn wire(&self) -> ErrorBody {
        use crate::SocialError::*;
        use storage::social::SocialStorageError as S;
        let (code, retryable) = match self {
            Invalid(_) | Storage(S::Invalid(_)) | Storage(S::Counter(_)) => {
                ("invalid_input", false)
            }
            Storage(S::NotFound(_)) => ("not_found", false),
            Storage(S::Forbidden(_)) => ("not_authorized", false),
            Storage(S::Conflict(_)) => ("revision_conflict", true),
            Storage(_) => ("storage_failure", true),
            Ai(_) | ModelUnavailable(_) => ("model_unavailable", false),
            BudgetExceeded => ("budget_exhausted", false),
            Unavailable => ("unavailable", true),
        };
        let message = if code == "storage_failure" {
            "消息存储失败，请检查本地数据目录后重试".into()
        } else {
            self.to_string()
        };
        ErrorBody {
            code,
            message,
            retryable,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateGroupInput {
    pub request_id: CommandId,
    pub title: String,
    pub member_ids: Vec<ActorId>,
}
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MembershipInput {
    pub request_id: CommandId,
    pub actor_id: ActorId,
    pub expected_revision: Revision,
    pub action: storage::social::MembershipAction,
}
