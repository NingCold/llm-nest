use super::{LegacyImportResult, LegacyPreview, SocialStorageResult};
use ::social::*;
use async_trait::async_trait;
use events::SocialEvent;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// An authenticated local operator is supplied by the future application layer.
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
    pub expected_revision: Revision,
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
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FinishRun {
    pub run_id: RunId,
    pub expected_revision: Revision,
    pub status: RunStatus,
    pub body: MessageBody,
}

/// Bounded, asynchronous, typed operations. Transactions never span model calls.
/// All mutation receipts and persistent references commit with their entities.
#[async_trait]
pub trait SocialStore: Send + Sync {
    async fn local_human(&self, initial_name: String) -> SocialStorageResult<Actor>;
    async fn actor(&self, operator: ActorId, id: ActorId) -> SocialStorageResult<Actor>;
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
    async fn begin_run(&self, ctx: CommandContext, input: BeginRun) -> SocialStorageResult<AiRun>;
    async fn checkpoint_run(
        &self,
        operator: ActorId,
        run_id: RunId,
        body: MessageBody,
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
}
