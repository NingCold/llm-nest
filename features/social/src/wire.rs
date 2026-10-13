//! Shared business DTOs. HTTP and IPC only select their transport.
use serde::{Deserialize, Serialize};
use social::*;
use storage::social::{BindingCandidate, ConversationRecord, RunFailure};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Memory {
    pub id: MemoryId,
    pub actor_id: ActorId,
    pub conversation_id: ConversationId,
    pub kind: MemoryKind,
    pub text: String,
    pub status: MaterialStatus,
    pub revision: Revision,
    pub source: Option<storage::social::MemoryMessageSource>,
    pub created_by: ActorId,
    pub updated_by: ActorId,
    pub created_at: String,
    pub updated_at: String,
}
impl From<MemoryRecord> for Memory {
    fn from(m: MemoryRecord) -> Self {
        let source = m.sources.iter().find_map(|s| match s {
            SourceRef::Message {
                message_id,
                revision,
                ..
            } => Some(storage::social::MemoryMessageSource {
                message_id: *message_id,
                revision: *revision,
            }),
            _ => None,
        });
        Self {
            id: m.id,
            actor_id: m.owner_actor_id,
            conversation_id: m.conversation_id,
            kind: m.kind,
            text: m.text,
            status: m.status,
            revision: m.revision,
            source,
            created_by: m.created_by,
            updated_by: m.updated_by,
            created_at: m.created_at.to_rfc3339(),
            updated_at: m.updated_at.to_rfc3339(),
        }
    }
}
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateMemoryInput {
    pub request_id: CommandId,
    #[serde(flatten)]
    pub content: storage::social::MemoryContent,
}
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateMemoryInput {
    pub request_id: CommandId,
    pub expected_revision: Revision,
    #[serde(flatten)]
    pub content: storage::social::MemoryContent,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Binding {
    pub provider_id: String,
    pub model_id: String,
    pub reasoning_effort: Option<String>,
    #[serde(default)]
    pub temperature: Option<f32>,
    #[serde(default)]
    pub max_tokens: Option<u32>,
}
impl From<ExecutionBinding> for Binding {
    fn from(b: ExecutionBinding) -> Self {
        Self {
            provider_id: b.provider_id,
            model_id: b.model_id,
            reasoning_effort: b.reasoning_effort,
            temperature: b.temperature,
            max_tokens: b.max_tokens,
        }
    }
}
impl From<Binding> for BindingCandidate {
    fn from(b: Binding) -> Self {
        Self {
            provider_id: b.provider_id,
            model_id: b.model_id,
            reasoning_effort: b.reasoning_effort,
            temperature: b.temperature,
            max_tokens: b.max_tokens,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    pub id: ActorId,
    pub kind: ActorKind,
    pub name: String,
    pub avatar_ref: Option<String>,
    pub bio: String,
    pub revision: Revision,
    pub status: EntityStatus,
}
impl From<Actor> for Account {
    fn from(c: Actor) -> Self {
        Self {
            id: c.id,
            kind: c.kind,
            name: c.display_name,
            avatar_ref: c.avatar_ref,
            bio: c.bio,
            revision: c.revision,
            status: c.status,
        }
    }
}
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedAi {
    pub account: Account,
    pub persona: String,
    pub binding: Option<Binding>,
    pub administrators: Vec<social::Administrator>,
    pub is_initial_administrator: bool,
}
impl From<storage::social::ManagedAccount> for ManagedAi {
    fn from(c: storage::social::ManagedAccount) -> Self {
        Self {
            account: c.contact.actor.into(),
            persona: c.contact.profile.persona,
            binding: c.contact.binding.map(|b| Binding {
                provider_id: b.provider_id,
                model_id: b.model_id,
                reasoning_effort: b.reasoning_effort,
                temperature: b.temperature,
                max_tokens: b.max_tokens,
            }),
            administrators: c.administrators,
            is_initial_administrator: c.is_initial_administrator,
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
    pub memory_owner_ids: Vec<ActorId>,
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
    pub current_bout: Option<Bout>,
}
impl From<ConversationRecord> for Conversation {
    fn from(c: ConversationRecord) -> Self {
        Self {
            memory_owner_ids: c.memory_owner_ids,
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
            current_bout: c.current_bout.map(Into::into),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Bout {
    pub id: BoutId,
    pub status: BoutStatus,
    pub revision: Revision,
}
impl From<social::Bout> for Bout {
    fn from(b: social::Bout) -> Self {
        Self {
            id: b.id,
            status: b.status,
            revision: b.revision,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviseInput {
    pub request_id: CommandId,
    pub expected_revision: Revision,
    pub text: String,
}
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionedCommand {
    pub request_id: CommandId,
    pub expected_revision: Revision,
}
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ControlAction {
    Pause,
    Resume,
    Archive,
    Restore,
    End,
}
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ControlInput {
    pub request_id: CommandId,
    pub expected_revision: Revision,
    pub action: ControlAction,
}
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PauseMemberInput {
    pub request_id: CommandId,
    pub actor_id: ActorId,
    pub expected_revision: Revision,
    pub paused: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Member {
    pub actor_id: ActorId,
    pub paused: bool,
    pub revision: Revision,
}
impl From<Membership> for Member {
    fn from(m: Membership) -> Self {
        Self {
            actor_id: m.actor_id,
            paused: m.paused,
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
    pub generation_limits:
        std::collections::BTreeMap<String, ai_client::generation::GenerationLimits>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Init {
    pub self_id: ActorId,
    pub self_name: String,
    pub self_account: Account,
    pub accounts: Vec<Account>,
    pub address_book: Vec<social::ContactEntry>,
    pub conversations: Vec<Conversation>,
    pub models: Vec<ModelOption>,
    pub cursor: EventCursor,
}
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountView {
    pub operator_id: ActorId,
    pub snapshot: Init,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiInput {
    pub request_id: CommandId,
    pub name: String,
    #[serde(default)]
    pub bio: String,
    pub persona: String,
    pub binding: Option<Binding>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateAiInput {
    pub request_id: CommandId,
    pub name: String,
    pub bio: String,
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
pub struct AccountControlInput {
    pub request_id: CommandId,
    pub expected_revision: Revision,
    pub action: storage::social::AccountAction,
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
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SubscriptionEvent {
    Change { event: events::SocialEvent },
    Generation { event: LiveEvent },
    Resync,
}

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
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SummarySettingsInput {
    pub request_id: social::CommandId,
    #[serde(flatten)]
    pub settings: storage::social::ConfigureSummaries,
}
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SummaryRequest {
    pub request_id: social::CommandId,
    pub retry: Option<SummaryRetry>,
}
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SummaryRetry {
    pub id: social::SummaryId,
    pub expected_revision: social::Revision,
}
