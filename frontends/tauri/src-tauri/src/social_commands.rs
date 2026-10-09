//! The same business DTOs and feature methods as the HTTP adapter.
use crate::BackendState;
use social::{ActorId, ConversationId, EventCursor, RunId, Sequence};
use social_feature::{SocialError, wire};

type Result<T> = std::result::Result<T, wire::ErrorBody>;
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
pub async fn social_create_contact(
    state: tauri::State<'_, BackendState>,
    input: wire::ContactInput,
) -> Result<wire::Contact> {
    feature(state)
        .await?
        .create_contact(input)
        .await
        .map_err(|e| e.wire())
}
#[tauri::command]
pub async fn social_update_contact(
    state: tauri::State<'_, BackendState>,
    id: ActorId,
    input: wire::UpdateContactInput,
) -> Result<wire::Contact> {
    feature(state)
        .await?
        .update_contact(id, input)
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
