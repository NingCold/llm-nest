//! The same business DTOs and feature methods as the HTTP adapter.
use crate::BackendState;
use social::{
    ActorId, ContextScope, ConversationId, EventCursor, MemoryId, MessageId, RunId, Sequence,
};
use social_feature::{SocialError, wire};

type Result<T> = std::result::Result<T, wire::ErrorBody>;
#[tauri::command]
pub async fn social_summaries(
    state: tauri::State<'_, BackendState>,
    id: ConversationId,
    before: Option<Sequence>,
) -> Result<storage::social::SummaryState> {
    feature(state)
        .await?
        .summaries(id, before)
        .await
        .map_err(|e| e.wire())
}
#[tauri::command]
pub async fn social_configure_summaries(
    state: tauri::State<'_, BackendState>,
    id: ConversationId,
    input: wire::SummarySettingsInput,
) -> Result<social::SummarySettings> {
    feature(state)
        .await?
        .configure_summaries(id, input)
        .await
        .map_err(|e| e.wire())
}
#[tauri::command]
pub async fn social_request_summary(
    state: tauri::State<'_, BackendState>,
    id: ConversationId,
    input: wire::SummaryRequest,
) -> Result<Option<social::ConversationSummary>> {
    feature(state)
        .await?
        .request_summary(id, input)
        .await
        .map_err(|e| e.wire())
}

#[tauri::command]
pub async fn social_memories(
    state: tauri::State<'_, BackendState>,
    id: ConversationId,
    actor: ActorId,
) -> Result<Vec<wire::Memory>> {
    feature(state)
        .await?
        .memories(ContextScope {
            actor_id: actor,
            conversation_id: id,
        })
        .await
        .map_err(|e| e.wire())
}
#[tauri::command]
pub async fn social_create_memory(
    state: tauri::State<'_, BackendState>,
    id: ConversationId,
    actor: ActorId,
    input: wire::CreateMemoryInput,
) -> Result<wire::Memory> {
    feature(state)
        .await?
        .create_memory(
            ContextScope {
                actor_id: actor,
                conversation_id: id,
            },
            input,
        )
        .await
        .map_err(|e| e.wire())
}
#[tauri::command]
pub async fn social_update_memory(
    state: tauri::State<'_, BackendState>,
    id: MemoryId,
    input: wire::UpdateMemoryInput,
) -> Result<wire::Memory> {
    feature(state)
        .await?
        .update_memory(id, input)
        .await
        .map_err(|e| e.wire())
}
#[tauri::command]
pub async fn social_forget_memory(
    state: tauri::State<'_, BackendState>,
    id: MemoryId,
    input: wire::VersionedCommand,
) -> Result<wire::Memory> {
    feature(state)
        .await?
        .forget_memory(id, input)
        .await
        .map_err(|e| e.wire())
}

#[tauri::command]
pub async fn social_control_account(
    state: tauri::State<'_, BackendState>,
    id: ActorId,
    input: wire::AccountControlInput,
) -> Result<wire::Account> {
    feature(state)
        .await?
        .control_account(id, input)
        .await
        .map_err(|e| e.wire())
}

#[tauri::command]
pub async fn social_subscribe(
    state: tauri::State<'_, BackendState>,
    window: tauri::WebviewWindow,
    id: uuid::Uuid,
    after: EventCursor,
    channel: tauri::ipc::Channel<crate::social_subscription::Delivery>,
) -> Result<()> {
    // Register before async initialization so reload/unsubscribe also stops pending startup.
    let lease = state.subscriptions.start(window.label(), id)?;
    let feature = feature(state.clone()).await?;
    if lease.stop.is_cancelled() {
        return Err(crate::social_subscription::unavailable("订阅已释放"));
    }
    let subscription = feature
        .subscribe(after)
        .await
        .map_err(|error| error.wire())?;
    state.subscriptions.spawn(lease, subscription, channel)
}
#[tauri::command]
pub fn social_ack_subscription(
    state: tauri::State<'_, BackendState>,
    window: tauri::WebviewWindow,
    id: uuid::Uuid,
    sequence: u32,
) -> Result<()> {
    state
        .subscriptions
        .acknowledge(window.label(), id, sequence)
}
#[tauri::command]
pub fn social_unsubscribe(
    state: tauri::State<'_, BackendState>,
    window: tauri::WebviewWindow,
    id: uuid::Uuid,
) {
    state.subscriptions.remove(window.label(), id);
}
#[tauri::command]
pub async fn social_create_group(
    state: tauri::State<'_, BackendState>,
    input: wire::CreateGroupInput,
) -> Result<ConversationId> {
    feature(state)
        .await?
        .create_group(input)
        .await
        .map_err(|e| e.wire())
}
#[tauri::command]
pub async fn social_change_membership(
    state: tauri::State<'_, BackendState>,
    id: ConversationId,
    input: wire::MembershipInput,
) -> Result<wire::Conversation> {
    feature(state)
        .await?
        .change_membership(id, input)
        .await
        .map_err(|e| e.wire())
}
#[tauri::command]
pub async fn social_plans(
    state: tauri::State<'_, BackendState>,
    id: ConversationId,
) -> Result<Vec<wire::Plan>> {
    feature(state).await?.plans(id).await.map_err(|e| e.wire())
}
async fn feature(
    state: tauri::State<'_, BackendState>,
) -> Result<std::sync::Arc<social_feature::SocialFeature>> {
    state
        .get()
        .await
        .map(|s| s.social.clone())
        .map_err(|message| wire::ErrorBody {
            code: "unavailable",
            message,
            retryable: true,
        })
}
#[tauri::command]
pub async fn social_init(state: tauri::State<'_, BackendState>) -> Result<wire::Init> {
    feature(state).await?.init().await.map_err(|e| e.wire())
}
#[tauri::command]
pub async fn social_create_ai(
    state: tauri::State<'_, BackendState>,
    input: wire::AiInput,
) -> Result<wire::ManagedAi> {
    feature(state)
        .await?
        .create_ai(input)
        .await
        .map_err(|e| e.wire())
}
#[tauri::command]
pub async fn social_update_ai(
    state: tauri::State<'_, BackendState>,
    id: ActorId,
    input: wire::UpdateAiInput,
) -> Result<wire::ManagedAi> {
    feature(state)
        .await?
        .update_ai(id, input)
        .await
        .map_err(|e| e.wire())
}
#[tauri::command]
pub async fn social_open_direct(
    state: tauri::State<'_, BackendState>,
    id: ActorId,
    input: wire::Command,
) -> Result<ConversationId> {
    feature(state)
        .await?
        .open_direct(id, input)
        .await
        .map_err(|e| e.wire())
}
#[tauri::command]
pub async fn social_conversation(
    state: tauri::State<'_, BackendState>,
    id: ConversationId,
) -> Result<wire::Conversation> {
    feature(state)
        .await?
        .conversation(id)
        .await
        .map_err(|e| e.wire())
}
#[tauri::command]
pub async fn social_messages(
    state: tauri::State<'_, BackendState>,
    id: ConversationId,
    before: Option<Sequence>,
) -> Result<wire::MessagePage> {
    feature(state)
        .await?
        .messages(id, before)
        .await
        .map_err(|e| e.wire())
}
#[tauri::command]
pub async fn social_send(
    state: tauri::State<'_, BackendState>,
    id: ConversationId,
    input: wire::SendInput,
) -> Result<wire::SendReceipt> {
    feature(state)
        .await?
        .send(id, input)
        .await
        .map_err(|e| e.wire())
}
#[tauri::command]
pub async fn social_events(
    state: tauri::State<'_, BackendState>,
    after: EventCursor,
) -> Result<Vec<events::SocialEvent>> {
    feature(state)
        .await?
        .events(after)
        .await
        .map_err(|e| e.wire())
}
#[tauri::command]
pub async fn social_run_details(
    state: tauri::State<'_, BackendState>,
    id: RunId,
) -> Result<Option<storage::social::RunDetails>> {
    feature(state)
        .await?
        .run_details(id)
        .await
        .map_err(|e| e.wire())
}
#[tauri::command]
pub async fn social_cancel_run(state: tauri::State<'_, BackendState>, id: RunId) -> Result<()> {
    feature(state)
        .await?
        .cancel_run(id)
        .await
        .map_err(|e: SocialError| e.wire())
}

#[tauri::command]
pub async fn social_control_conversation(
    state: tauri::State<'_, BackendState>,
    id: ConversationId,
    input: wire::ControlInput,
) -> Result<wire::Conversation> {
    feature(state)
        .await?
        .control_conversation(id, input)
        .await
        .map_err(|e| e.wire())
}

#[tauri::command]
pub async fn social_pause_member(
    state: tauri::State<'_, BackendState>,
    id: ConversationId,
    input: wire::PauseMemberInput,
) -> Result<wire::Conversation> {
    feature(state)
        .await?
        .pause_member(id, input)
        .await
        .map_err(|e| e.wire())
}

#[tauri::command]
pub async fn social_revise_message(
    state: tauri::State<'_, BackendState>,
    id: MessageId,
    input: wire::ReviseInput,
) -> Result<wire::Message> {
    feature(state)
        .await?
        .revise_message(id, input)
        .await
        .map_err(|e| e.wire())
}

#[tauri::command]
pub async fn social_withdraw_message(
    state: tauri::State<'_, BackendState>,
    id: MessageId,
    input: wire::VersionedCommand,
) -> Result<wire::Message> {
    feature(state)
        .await?
        .withdraw_message(id, input)
        .await
        .map_err(|e| e.wire())
}

#[tauri::command]
pub async fn social_message(
    state: tauri::State<'_, BackendState>,
    id: MessageId,
) -> Result<wire::Message> {
    feature(state)
        .await?
        .message(id)
        .await
        .map_err(|e| e.wire())
}

#[tauri::command]
pub async fn social_account(
    state: tauri::State<'_, BackendState>,
    id: ActorId,
) -> Result<wire::Account> {
    feature(state)
        .await?
        .account(id)
        .await
        .map_err(|e| e.wire())
}

#[tauri::command]
pub async fn social_managed_accounts(
    state: tauri::State<'_, BackendState>,
) -> Result<Vec<wire::ManagedAi>> {
    feature(state)
        .await?
        .managed_accounts()
        .await
        .map_err(|e| e.wire())
}

#[tauri::command]
pub async fn social_managed_account(
    state: tauri::State<'_, BackendState>,
    id: ActorId,
) -> Result<wire::ManagedAi> {
    feature(state)
        .await?
        .managed_account(id)
        .await
        .map_err(|e| e.wire())
}

#[tauri::command]
pub async fn social_set_administrator(
    state: tauri::State<'_, BackendState>,
    id: ActorId,
    input: social_feature::AdministratorInput,
) -> Result<wire::ManagedAi> {
    feature(state)
        .await?
        .set_administrator(id, input)
        .await
        .map_err(|e| e.wire())
}

#[tauri::command]
pub async fn social_set_contact(
    state: tauri::State<'_, BackendState>,
    id: ActorId,
    input: social_feature::SetContactInput,
) -> Result<social::ContactEntry> {
    feature(state)
        .await?
        .set_contact(id, input)
        .await
        .map_err(|e| e.wire())
}

#[tauri::command]
pub async fn social_update_account(
    state: tauri::State<'_, BackendState>,
    id: ActorId,
    input: social_feature::AccountInput,
) -> Result<wire::Account> {
    feature(state)
        .await?
        .update_account(id, input)
        .await
        .map_err(|e| e.wire())
}

#[tauri::command]
pub async fn social_account_view(
    state: tauri::State<'_, BackendState>,
    id: ActorId,
) -> Result<wire::AccountView> {
    feature(state)
        .await?
        .account_view(id)
        .await
        .map_err(|e| e.wire())
}

#[tauri::command]
pub async fn social_account_view_messages(
    state: tauri::State<'_, BackendState>,
    id: ActorId,
    conversation: ConversationId,
    before: Option<Sequence>,
) -> Result<wire::MessagePage> {
    feature(state)
        .await?
        .account_view_messages(id, conversation, before)
        .await
        .map_err(|e| e.wire())
}

#[tauri::command]
pub async fn social_reference_preview(
    state: tauri::State<'_, BackendState>,
    input: wire::ReferenceInput,
) -> Result<wire::ReferencePreview> {
    feature(state)
        .await?
        .reference_preview(input)
        .await
        .map_err(|e| e.wire())
}
