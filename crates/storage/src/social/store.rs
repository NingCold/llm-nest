use super::{LegacyImportResult, LegacyPreview, SocialStorageResult};
use ::social::*;
use async_trait::async_trait;
use events::SocialEvent;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// The application supplies the persisted local operator, never a client sender.
/// Store validates it against the single persisted human, never an arbitrary sender.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandContext {
    pub operator: ActorId,
    pub request_id: CommandId,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateAi {
    pub display_name: String,
    pub persona: String,
    pub binding: Option<BindingCandidate>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BindingCandidate {
    pub provider_id: String,
    pub model_id: String,
    pub reasoning_effort: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppendMessage {
    pub conversation_id: ConversationId,
    pub body: MessageBody,
    pub reply_to: Option<MessageId>,
    /// Appends allocate sequence in the transaction; callers may opt into a revision check.
    pub expected_revision: Option<Revision>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendMessage {
    pub conversation_id: ConversationId,
    pub body: MessageBody,
    pub reply_to: Option<MessageId>,
    pub mentions: Vec<ActorId>,
}
#[derive(Debug, Clone)]
pub struct SavedMessage {
    pub message: SocialMessage,
    pub plans: Vec<ParticipationPlan>,
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HistoryVisibility {
    FromNow,
    All,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangeMembership {
    pub conversation_id: ConversationId,
    pub actor_id: ActorId,
    pub expected_revision: Revision,
    /// None removes the member; rejoining grants a new visibility interval.
    pub history: Option<HistoryVisibility>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviseMessage {
    pub message_id: MessageId,
    pub body: MessageBody,
    pub expected_revision: Revision,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationControl {
    pub conversation_id: ConversationId,
    pub expected_revision: Revision,
    pub status: EntityStatus,
    pub paused: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BeginRun {
    pub snapshot: ContextSnapshot,
    pub expected_revision: Revision,
    #[serde(default)]
    pub details: Option<RunDetails>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FinishRun {
    pub run_id: RunId,
    pub expected_revision: Revision,
    pub status: RunStatus,
    pub body: MessageBody,
    #[serde(default)]
    pub details: Option<RunDetails>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContactRecord {
    pub actor: Actor,
    pub profile: AiProfile,
    pub binding: Option<ExecutionBinding>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationRecord {
    pub conversation: Conversation,
    pub members: Vec<Membership>,
    pub latest_message: Option<SocialMessage>,
    pub active_run: Option<AiRun>,
    pub pending_plans: Vec<ParticipationPlan>,
}

#[derive(Debug, Clone)]
pub struct SocialOverview {
    pub human: Actor,
    pub contacts: Vec<ContactRecord>,
    pub conversations: Vec<ConversationRecord>,
    pub cursor: EventCursor,
}

#[derive(Debug, Clone)]
pub struct ContextMaterial {
    pub contact: ContactRecord,
    pub conversation: Conversation,
    pub members: Vec<Membership>,
    pub relationship_revision: Option<Revision>,
    /// Current public roster; other members' private profiles are not context.
    pub participant_names: Vec<String>,
    /// Recent committed history through the trigger, followed by completed answers to it.
    pub messages: Vec<SocialMessage>,
    pub trigger_id: MessageId,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateAi {
    pub display_name: String,
    pub persona: String,
    pub binding: Option<BindingCandidate>,
    pub expected_revision: Revision,
}

/// Execution details remain separate from social message content and context.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunDetails {
    pub provider: String,
    pub model: String,
    pub wire_model: String,
    pub protocol: String,
    pub reasoning: String,
    pub usage: Option<common::Usage>,
    pub timings: Option<common::MessageTimings>,
    pub error: Option<RunFailure>,
}

pub use events::SocialFailure as RunFailure;

/// Bounded, asynchronous, typed operations. Transactions never span model calls.
/// All mutation receipts and persistent references commit with their entities.
#[async_trait]
pub trait SocialStore: Send + Sync {
    async fn send_message(
        &self,
        ctx: CommandContext,
        input: SendMessage,
    ) -> SocialStorageResult<SavedMessage>;
    async fn change_membership(
        &self,
        ctx: CommandContext,
        input: ChangeMembership,
    ) -> SocialStorageResult<Conversation>;
    async fn plans(
        &self,
        operator: ActorId,
        id: ConversationId,
    ) -> SocialStorageResult<Vec<ParticipationPlan>>;
    async fn next_plan(
        &self,
        operator: ActorId,
        id: ConversationId,
    ) -> SocialStorageResult<Option<ParticipationPlan>>;
    async fn queued_conversations(
        &self,
        operator: ActorId,
    ) -> SocialStorageResult<Vec<ConversationId>>;
    async fn begin_plan(
        &self,
        operator: ActorId,
        plan: PlanId,
        input: BeginRun,
    ) -> SocialStorageResult<AiRun>;
    async fn fail_plan(
        &self,
        operator: ActorId,
        plan: PlanId,
        failure: String,
    ) -> SocialStorageResult<()>;
    async fn local_human(&self, initial_name: String) -> SocialStorageResult<Actor>;
    async fn actor(&self, operator: ActorId, id: ActorId) -> SocialStorageResult<Actor>;
    async fn overview(&self, operator: ActorId) -> SocialStorageResult<SocialOverview>;
    async fn contact(&self, operator: ActorId, id: ActorId) -> SocialStorageResult<ContactRecord>;
    async fn update_ai(
        &self,
        ctx: CommandContext,
        input: UpdateAi,
        id: ActorId,
    ) -> SocialStorageResult<ContactRecord>;
    async fn context_material(
        &self,
        operator: ActorId,
        scope: ContextScope,
        trigger: MessageId,
    ) -> SocialStorageResult<ContextMaterial>;
    async fn run(&self, operator: ActorId, id: RunId) -> SocialStorageResult<AiRun>;
    async fn run_for_trigger(
        &self,
        operator: ActorId,
        trigger: MessageId,
    ) -> SocialStorageResult<Option<AiRun>>;
    async fn run_details(
        &self,
        operator: ActorId,
        id: RunId,
    ) -> SocialStorageResult<Option<RunDetails>>;
    async fn create_ai(&self, ctx: CommandContext, input: CreateAi) -> SocialStorageResult<Actor>;
    async fn open_direct(
        &self,
        ctx: CommandContext,
        ai: ActorId,
    ) -> SocialStorageResult<Conversation>;
    async fn create_group(
        &self,
        ctx: CommandContext,
        title: String,
        members: Vec<ActorId>,
    ) -> SocialStorageResult<Conversation>;
    async fn conversation(
        &self,
        operator: ActorId,
        id: ConversationId,
    ) -> SocialStorageResult<Conversation>;
    async fn conversation_record(
        &self,
        operator: ActorId,
        id: ConversationId,
    ) -> SocialStorageResult<ConversationRecord>;
    async fn append_message(
        &self,
        ctx: CommandContext,
        input: AppendMessage,
    ) -> SocialStorageResult<SocialMessage>;
    async fn revise_message(
        &self,
        ctx: CommandContext,
        input: ReviseMessage,
    ) -> SocialStorageResult<SocialMessage>;
    async fn control_conversation(
        &self,
        ctx: CommandContext,
        input: ConversationControl,
    ) -> SocialStorageResult<Conversation>;
    async fn messages(
        &self,
        operator: ActorId,
        id: ConversationId,
        after: Sequence,
        limit: u32,
    ) -> SocialStorageResult<Vec<SocialMessage>>;
    async fn message_page(
        &self,
        operator: ActorId,
        id: ConversationId,
        before: Option<Sequence>,
    ) -> SocialStorageResult<(Vec<SocialMessage>, bool)>;
    async fn begin_run(&self, ctx: CommandContext, input: BeginRun) -> SocialStorageResult<AiRun>;
    async fn checkpoint_run(
        &self,
        operator: ActorId,
        run_id: RunId,
        body: MessageBody,
        details: Option<RunDetails>,
    ) -> SocialStorageResult<()>;
    async fn finish_run(&self, ctx: CommandContext, input: FinishRun)
    -> SocialStorageResult<AiRun>;
    async fn end_bout(
        &self,
        ctx: CommandContext,
        bout_id: BoutId,
        expected_revision: Revision,
    ) -> SocialStorageResult<Bout>;
    async fn events(
        &self,
        operator: ActorId,
        after: EventCursor,
        limit: u32,
    ) -> SocialStorageResult<Vec<SocialEvent>>;
    async fn backup(&self, destination: PathBuf) -> SocialStorageResult<()>;
    async fn preview_legacy(&self) -> SocialStorageResult<LegacyPreview>;
    async fn import_legacy(
        &self,
        operator: ActorId,
        preview: LegacyPreview,
    ) -> SocialStorageResult<LegacyImportResult>;
    /// Drain the writer after all feature tasks have stopped.
    async fn shutdown(&self) -> SocialStorageResult<()>;
}
