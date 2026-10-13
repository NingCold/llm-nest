use super::{LegacyImportResult, LegacyPreview, SocialStorageResult};
use ::social::*;
use async_trait::async_trait;
use events::SocialEvent;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// The application supplies the persisted local operator, never a client sender.
/// Store checks account status and use-case permissions. GUI supplies its trusted local human;
/// entering an AI's read-only viewpoint never changes this operator.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandContext {
    pub operator: ActorId,
    pub request_id: CommandId,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryMessageSource {
    pub message_id: MessageId,
    pub revision: Revision,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryContent {
    pub kind: MemoryKind,
    pub text: String,
    pub source: Option<MemoryMessageSource>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateMemory {
    pub scope: ContextScope,
    pub content: MemoryContent,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateMemory {
    pub id: MemoryId,
    pub expected_revision: Revision,
    pub content: MemoryContent,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateAi {
    pub display_name: String,
    pub bio: String,
    pub avatar_ref: Option<String>,
    pub persona: String,
    pub binding: Option<BindingCandidate>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BindingCandidate {
    pub provider_id: String,
    pub model_id: String,
    pub reasoning_effort: Option<String>,
    // Omitted defaults preserve digests of pre-parameter command receipts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
}
impl BindingCandidate {
    pub(super) fn validate_generation(&self) -> SocialStorageResult<()> {
        if self
            .temperature
            .is_some_and(|t| !t.is_finite() || !(0.0..=2.0).contains(&t))
        {
            return Err(super::SocialStorageError::Invalid(
                "temperature must be finite and in 0..=2".into(),
            ));
        }
        if self.max_tokens == Some(0) {
            return Err(super::SocialStorageError::Invalid(
                "max_tokens must be positive".into(),
            ));
        }
        Ok(())
    }
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reference: Option<ReferenceSelection>,
}
#[derive(Debug, Clone)]
pub struct SavedMessage {
    pub message: SocialMessage,
    pub plans: Vec<ParticipationPlan>,
    /// Resolved response targets include paused/archived identities so the use case can explain availability.
    pub response_targets: Vec<ActorId>,
}
#[derive(Debug, Clone)]
pub struct ReferencePreview {
    pub conversation: Conversation,
    pub messages: Vec<SocialMessage>,
    pub summary: Option<ConversationSummary>,
    pub dependencies: Vec<VisibilityDependency>,
}
#[derive(Debug, Clone)]
pub struct ReferenceMaterial {
    pub message_id: MessageId,
    pub revision: Revision,
    pub required: bool,
    pub preview: ReferencePreview,
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MembershipAction {
    Add,
    Remove,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangeMembership {
    pub conversation_id: ConversationId,
    pub actor_id: ActorId,
    pub expected_revision: Revision,
    pub action: MembershipAction,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviseMessage {
    pub message_id: MessageId,
    pub body: MessageBody,
    pub expected_revision: Revision,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WithdrawMessage {
    pub message_id: MessageId,
    pub expected_revision: Revision,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PauseMember {
    pub conversation_id: ConversationId,
    pub actor_id: ActorId,
    pub expected_revision: Revision,
    pub paused: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EndConversation {
    pub conversation_id: ConversationId,
    pub expected_revision: Revision,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationControl {
    pub conversation_id: ConversationId,
    pub expected_revision: Revision,
    pub action: ConversationAction,
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConversationAction {
    Pause,
    Resume,
    Archive,
    Restore,
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
    pub contact_schedule: ContactScheduleView,
    pub participation: ParticipationView,
    pub memory_owner_ids: Vec<ActorId>,
    pub conversation: Conversation,
    pub members: Vec<Membership>,
    pub latest_message: Option<SocialMessage>,
    pub active_run: Option<AiRun>,
    pub current_bout: Option<Bout>,
    pub pending_plans: Vec<ParticipationPlan>,
}

#[derive(Debug, Clone)]
pub struct SocialOverview {
    pub account: Actor,
    pub accounts: Vec<Actor>,
    pub address_book: Vec<ContactEntry>,
    pub conversations: Vec<ConversationRecord>,
    pub cursor: EventCursor,
}

#[derive(Debug, Clone)]
pub struct ContextMaterial {
    pub required_message_ids: Vec<MessageId>,
    pub reference: Option<ReferenceMaterial>,
    pub summaries: Vec<ConversationSummary>,
    pub memories: Vec<MemoryRecord>,
    pub contact: ContactRecord,
    pub conversation: Conversation,
    pub members: Vec<Membership>,
    pub relationship_revision: Option<Revision>,
    /// Current public roster; other members' private profiles are not context.
    pub participant_names: Vec<String>,
    /// Recent committed history through the trigger, followed by completed answers to it.
    pub messages: Vec<SocialMessage>,
    pub trigger: ContextTrigger,
}

#[derive(Debug, Clone)]
pub enum ContextTrigger {
    Message(MessageId),
    Contact(ContactIntent),
}
impl ContextTrigger {
    pub fn message_id(&self) -> Option<MessageId> {
        match self {
            Self::Message(id) => Some(*id),
            Self::Contact(_) => None,
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContactScheduleView {
    pub settings: ContactSettings,
    pub intents: Vec<ContactIntent>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateContactIntent {
    pub conversation_id: ConversationId,
    pub actor_id: ActorId,
    pub text: String,
    pub due_at: chrono::DateTime<chrono::FixedOffset>,
    pub expires_at: chrono::DateTime<chrono::Utc>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateContactSettings {
    pub conversation_id: ConversationId,
    pub settings: ContactSettings,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CancelContactIntent {
    pub conversation_id: ConversationId,
    pub intent_id: IntentId,
    pub expected_revision: Revision,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParticipationView {
    pub policy: ParticipationPolicy,
    pub state: ParticipationState,
    pub budget_day: String,
    pub auto_turns_today: u32,
    pub reserved_tokens_today: u32,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateParticipation {
    pub conversation_id: ConversationId,
    pub policy: ParticipationPolicy,
}
pub struct ParticipationTick {
    pub plan: Option<ParticipationPlan>,
    pub wake_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateAi {
    pub display_name: String,
    pub bio: String,
    pub avatar_ref: Option<String>,
    pub persona: String,
    pub binding: Option<BindingCandidate>,
    pub expected_revision: Revision,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountControl {
    pub actor_id: ActorId,
    pub expected_revision: Revision,
    pub action: AccountAction,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AccountAction {
    Archive,
    Restore,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetContact {
    pub account_id: ActorId,
    pub listed: bool,
    pub expected_revision: Option<Revision>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetAdministrator {
    pub account_id: ActorId,
    pub granted: bool,
    pub expected_revision: Revision,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateAccount {
    pub display_name: String,
    pub bio: String,
    pub expected_revision: Revision,
}
#[derive(Debug, Clone)]
pub struct ManagedAccount {
    pub contact: ContactRecord,
    pub administrators: Vec<Administrator>,
    pub is_initial_administrator: bool,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SummaryState {
    pub settings: SummarySettings,
    pub segments: Vec<ConversationSummary>,
    pub has_more: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigureSummaries {
    pub enabled: bool,
    pub binding: Option<SummaryBinding>,
    pub expected_revision: Revision,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueueSummary {
    pub conversation_id: ConversationId,
    pub retry: Option<(SummaryId, Revision)>,
    pub automatic: bool,
    pub settings_revision: Revision,
    pub input_budget: u32,
}
#[derive(Debug, Clone)]
pub struct SummaryWork {
    pub summary: ConversationSummary,
    pub messages: Vec<SocialMessage>,
}
#[derive(Debug, Clone)]
pub struct FinishSummary {
    pub id: SummaryId,
    pub expected_revision: Revision,
    pub status: SummaryStatus,
    pub text: String,
    pub error: Option<String>,
}

/// Bounded, asynchronous, typed operations. Transactions never span model calls.
/// All mutation receipts and persistent references commit with their entities.
#[async_trait]
pub trait SocialStore: Send + Sync {
    async fn reference_preview(
        &self,
        operator: ActorId,
        actor: ActorId,
        selection: ReferenceSelection,
    ) -> SocialStorageResult<ReferencePreview>;
    async fn interrupt_summary_tasks(&self) -> SocialStorageResult<()>;
    async fn summaries(
        &self,
        operator: ActorId,
        id: ConversationId,
        before: Option<Sequence>,
    ) -> SocialStorageResult<SummaryState>;
    async fn configure_summaries(
        &self,
        ctx: CommandContext,
        id: ConversationId,
        input: ConfigureSummaries,
    ) -> SocialStorageResult<SummarySettings>;
    async fn queue_summary(
        &self,
        ctx: CommandContext,
        input: QueueSummary,
    ) -> SocialStorageResult<Option<ConversationSummary>>;
    async fn begin_summary(
        &self,
        operator: ActorId,
        id: SummaryId,
    ) -> SocialStorageResult<Option<SummaryWork>>;
    async fn finish_summary(
        &self,
        operator: ActorId,
        input: FinishSummary,
    ) -> SocialStorageResult<ConversationSummary>;
    async fn memories(
        &self,
        operator: ActorId,
        scope: ContextScope,
    ) -> SocialStorageResult<Vec<MemoryRecord>>;
    async fn memory(&self, operator: ActorId, id: MemoryId) -> SocialStorageResult<MemoryRecord>;
    async fn create_memory(
        &self,
        ctx: CommandContext,
        input: CreateMemory,
    ) -> SocialStorageResult<MemoryRecord>;
    async fn update_memory(
        &self,
        ctx: CommandContext,
        input: UpdateMemory,
    ) -> SocialStorageResult<MemoryRecord>;
    async fn forget_memory(
        &self,
        ctx: CommandContext,
        id: MemoryId,
        expected_revision: Revision,
    ) -> SocialStorageResult<MemoryRecord>;
    async fn managed_accounts(&self, operator: ActorId)
    -> SocialStorageResult<Vec<ManagedAccount>>;
    async fn managed_account(
        &self,
        operator: ActorId,
        id: ActorId,
    ) -> SocialStorageResult<ManagedAccount>;
    async fn set_administrator(
        &self,
        ctx: CommandContext,
        id: ActorId,
        input: SetAdministrator,
    ) -> SocialStorageResult<ManagedAccount>;
    async fn set_contact(
        &self,
        ctx: CommandContext,
        input: SetContact,
    ) -> SocialStorageResult<ContactEntry>;
    async fn update_account(
        &self,
        ctx: CommandContext,
        id: ActorId,
        input: UpdateAccount,
    ) -> SocialStorageResult<Actor>;
    async fn account_view(
        &self,
        operator: ActorId,
        id: ActorId,
    ) -> SocialStorageResult<SocialOverview>;
    async fn account_view_messages(
        &self,
        operator: ActorId,
        id: ActorId,
        conversation: ConversationId,
        before: Option<Sequence>,
    ) -> SocialStorageResult<(Vec<SocialMessage>, bool)>;
    async fn message(&self, operator: ActorId, id: MessageId)
    -> SocialStorageResult<SocialMessage>;
    async fn withdraw_message(
        &self,
        ctx: CommandContext,
        input: WithdrawMessage,
    ) -> SocialStorageResult<SocialMessage>;
    async fn pause_member(
        &self,
        ctx: CommandContext,
        input: PauseMember,
    ) -> SocialStorageResult<Conversation>;
    async fn end_conversation(
        &self,
        ctx: CommandContext,
        input: EndConversation,
    ) -> SocialStorageResult<Conversation>;
    async fn cancel_run(&self, operator: ActorId, id: RunId) -> SocialStorageResult<AiRun>;
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
    async fn participation_tick(
        &self,
        operator: ActorId,
        id: ConversationId,
    ) -> SocialStorageResult<ParticipationTick>;
    async fn update_contact_settings(
        &self,
        ctx: CommandContext,
        input: UpdateContactSettings,
    ) -> SocialStorageResult<ContactScheduleView>;
    async fn create_contact_intent(
        &self,
        ctx: CommandContext,
        input: CreateContactIntent,
    ) -> SocialStorageResult<ContactIntent>;
    async fn cancel_contact_intent(
        &self,
        ctx: CommandContext,
        input: CancelContactIntent,
    ) -> SocialStorageResult<ContactIntent>;
    async fn plan_material(
        &self,
        operator: ActorId,
        id: PlanId,
    ) -> SocialStorageResult<ContextMaterial>;
    async fn update_participation(
        &self,
        ctx: CommandContext,
        input: UpdateParticipation,
    ) -> SocialStorageResult<Conversation>;
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
        reason: Option<ParticipationReason>,
    ) -> SocialStorageResult<()>;
    async fn local_human(&self, initial_name: String) -> SocialStorageResult<Actor>;
    async fn actor(&self, operator: ActorId, id: ActorId) -> SocialStorageResult<Actor>;
    async fn overview(&self, operator: ActorId) -> SocialStorageResult<SocialOverview>;
    async fn contact(&self, operator: ActorId, id: ActorId) -> SocialStorageResult<ContactRecord>;
    async fn control_account(
        &self,
        ctx: CommandContext,
        input: AccountControl,
    ) -> SocialStorageResult<ContactRecord>;
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
