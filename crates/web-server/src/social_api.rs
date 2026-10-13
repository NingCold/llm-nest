use crate::AppState;
use axum::{
    Json, Router,
    extract::{FromRequest, FromRequestParts, Path, Query, Request, State},
    http::{StatusCode, request::Parts},
    response::{
        IntoResponse, Response,
        sse::{Event, KeepAlive, Sse},
    },
    routing::{get, post},
};
use serde::{Deserialize, de::DeserializeOwned};
use social::{
    ActorId, ContextScope, ConversationId, EventCursor, MemoryId, MessageId, RunId, Sequence,
};
use social_feature::{SocialError, wire};
use std::{convert::Infallible, sync::Arc, time::Duration};

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/social/init", get(init))
        .route("/api/social/messages/search", get(search_messages))
        .route("/api/social/messages/{id}/context", get(message_context))
        .route(
            "/api/social/conversations/{id}/list-state",
            post(update_conversation_list),
        )
        .route(
            "/api/social/history-import",
            get(preview_legacy).post(import_legacy),
        )
        .route(
            "/api/social/conversations/{id}/contact-settings",
            post(update_contact_settings),
        )
        .route(
            "/api/social/conversations/{id}/contact-intents",
            post(create_contact_intent),
        )
        .route(
            "/api/social/conversations/{id}/contact-intents/{intent}/cancel",
            post(cancel_contact_intent),
        )
        .route("/api/social/references/preview", post(reference_preview))
        .route(
            "/api/social/conversations/{id}/summaries",
            get(summaries).post(request_summary),
        )
        .route(
            "/api/social/conversations/{id}/summary-settings",
            post(configure_summaries),
        )
        .route(
            "/api/social/conversations/{id}/memories/{actor}",
            get(memories).post(create_memory),
        )
        .route(
            "/api/social/memories/{id}",
            axum::routing::patch(update_memory),
        )
        .route("/api/social/memories/{id}/forget", post(forget_memory))
        .route(
            "/api/social/ai-accounts",
            get(managed_accounts).post(create_ai),
        )
        .route(
            "/api/social/ai-accounts/{id}",
            get(managed_account).patch(update_ai),
        )
        .route("/api/social/accounts/{id}/direct", post(open_direct))
        .route(
            "/api/social/ai-accounts/{id}/control",
            post(control_account),
        )
        .route(
            "/api/social/accounts/{id}",
            get(account).patch(update_account),
        )
        .route("/api/social/contacts/{id}", post(set_contact))
        .route(
            "/api/social/ai-accounts/{id}/administrators",
            post(set_administrator),
        )
        .route("/api/social/ai-accounts/{id}/view", get(account_view))
        .route(
            "/api/social/ai-accounts/{id}/view/conversations/{conversation}/messages",
            get(account_view_messages),
        )
        .route("/api/social/groups", post(create_group))
        .route(
            "/api/social/conversations/{id}/members",
            post(change_membership),
        )
        .route("/api/social/conversations/{id}/plans", get(plans))
        .route(
            "/api/social/conversations/{id}/participation",
            post(update_participation),
        )
        .route(
            "/api/social/conversations/{id}/control",
            post(control_conversation),
        )
        .route(
            "/api/social/conversations/{id}/members/participation",
            post(pause_member),
        )
        .route("/api/social/messages/{id}", get(message))
        .route("/api/social/messages/{id}/revise", post(revise_message))
        .route("/api/social/messages/{id}/withdraw", post(withdraw_message))
        .route("/api/social/conversations/{id}", get(conversation))
        .route(
            "/api/social/conversations/{id}/messages",
            get(messages).post(send),
        )
        .route("/api/social/runs/{id}", get(run_details))
        .route("/api/social/runs/{id}/cancel", post(cancel))
        .route("/api/social/events", get(events))
        .route("/api/social/subscribe", get(subscribe))
}

async fn preview_legacy(
    State(state): State<Arc<AppState>>,
) -> Result<storage::social::LegacyPreview> {
    Ok(Json(state.social.preview_legacy().await?))
}

async fn import_legacy(
    State(state): State<Arc<AppState>>,
    ApiJson(preview): ApiJson<storage::social::LegacyPreview>,
) -> Result<storage::social::LegacyImportResult> {
    Ok(Json(state.social.import_legacy(preview).await?))
}

struct ApiError {
    status: StatusCode,
    body: wire::ErrorBody,
}
impl From<SocialError> for ApiError {
    fn from(e: SocialError) -> Self {
        let body = e.wire();
        let status = match body.code {
            "invalid_input" => StatusCode::BAD_REQUEST,
            "not_found" => StatusCode::NOT_FOUND,
            "not_authorized" => StatusCode::FORBIDDEN,
            "revision_conflict" => StatusCode::CONFLICT,
            "model_unavailable" | "budget_exhausted" => StatusCode::UNPROCESSABLE_ENTITY,
            _ => StatusCode::SERVICE_UNAVAILABLE,
        };
        Self { status, body }
    }
}
impl ApiError {
    fn invalid(status: StatusCode, message: String) -> Self {
        Self {
            status,
            body: SocialError::Invalid(message).wire(),
        }
    }
}
impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.status, Json(self.body)).into_response()
    }
}

// Keep transport parsing failures in the same error envelope as business failures.
struct ApiJson<T>(T);
impl<S: Send + Sync, T: DeserializeOwned> FromRequest<S> for ApiJson<T> {
    type Rejection = ApiError;
    async fn from_request(request: Request, state: &S) -> std::result::Result<Self, ApiError> {
        Json::<T>::from_request(request, state)
            .await
            .map(|Json(value)| Self(value))
            .map_err(|e| ApiError::invalid(e.status(), format!("请求正文无效：{}", e.body_text())))
    }
}
struct ApiPath<T>(T);
impl<S: Send + Sync, T: DeserializeOwned + Send> FromRequestParts<S> for ApiPath<T> {
    type Rejection = ApiError;
    async fn from_request_parts(
        parts: &mut Parts,
        state: &S,
    ) -> std::result::Result<Self, ApiError> {
        Path::<T>::from_request_parts(parts, state)
            .await
            .map(|Path(value)| Self(value))
            .map_err(|e| ApiError::invalid(e.status(), format!("路径参数无效：{}", e.body_text())))
    }
}
struct ApiQuery<T>(T);
impl<S: Send + Sync, T: DeserializeOwned> FromRequestParts<S> for ApiQuery<T> {
    type Rejection = ApiError;
    async fn from_request_parts(
        parts: &mut Parts,
        state: &S,
    ) -> std::result::Result<Self, ApiError> {
        Query::<T>::from_request_parts(parts, state)
            .await
            .map(|Query(value)| Self(value))
            .map_err(|e| ApiError::invalid(e.status(), format!("查询参数无效：{}", e.body_text())))
    }
}
type Result<T> = std::result::Result<Json<T>, ApiError>;
async fn memories(
    State(s): State<Arc<AppState>>,
    ApiPath((id, actor)): ApiPath<(ConversationId, ActorId)>,
) -> Result<Vec<wire::Memory>> {
    Ok(Json(
        s.social
            .memories(ContextScope {
                actor_id: actor,
                conversation_id: id,
            })
            .await?,
    ))
}
async fn create_memory(
    State(s): State<Arc<AppState>>,
    ApiPath((id, actor)): ApiPath<(ConversationId, ActorId)>,
    ApiJson(input): ApiJson<wire::CreateMemoryInput>,
) -> Result<wire::Memory> {
    Ok(Json(
        s.social
            .create_memory(
                ContextScope {
                    actor_id: actor,
                    conversation_id: id,
                },
                input,
            )
            .await?,
    ))
}
async fn update_memory(
    State(s): State<Arc<AppState>>,
    ApiPath(id): ApiPath<MemoryId>,
    ApiJson(input): ApiJson<wire::UpdateMemoryInput>,
) -> Result<wire::Memory> {
    Ok(Json(s.social.update_memory(id, input).await?))
}
async fn forget_memory(
    State(s): State<Arc<AppState>>,
    ApiPath(id): ApiPath<MemoryId>,
    ApiJson(input): ApiJson<wire::VersionedCommand>,
) -> Result<wire::Memory> {
    Ok(Json(s.social.forget_memory(id, input).await?))
}
async fn reference_preview(
    State(state): State<Arc<AppState>>,
    ApiJson(input): ApiJson<wire::ReferenceInput>,
) -> Result<wire::ReferencePreview> {
    Ok(Json(state.social.reference_preview(input).await?))
}
async fn summaries(
    State(s): State<Arc<AppState>>,
    ApiPath(id): ApiPath<ConversationId>,
    ApiQuery(query): ApiQuery<SummaryQuery>,
) -> Result<storage::social::SummaryState> {
    Ok(Json(s.social.summaries(id, query.before).await?))
}
#[derive(Deserialize)]
struct SummaryQuery {
    before: Option<Sequence>,
}
async fn configure_summaries(
    State(s): State<Arc<AppState>>,
    ApiPath(id): ApiPath<ConversationId>,
    ApiJson(input): ApiJson<wire::SummarySettingsInput>,
) -> Result<social::SummarySettings> {
    Ok(Json(s.social.configure_summaries(id, input).await?))
}
async fn request_summary(
    State(s): State<Arc<AppState>>,
    ApiPath(id): ApiPath<ConversationId>,
    ApiJson(input): ApiJson<wire::SummaryRequest>,
) -> Result<Option<social::ConversationSummary>> {
    Ok(Json(s.social.request_summary(id, input).await?))
}
async fn create_group(
    State(s): State<Arc<AppState>>,
    ApiJson(input): ApiJson<wire::CreateGroupInput>,
) -> Result<ConversationId> {
    Ok(Json(s.social.create_group(input).await?))
}
async fn change_membership(
    State(s): State<Arc<AppState>>,
    ApiPath(id): ApiPath<ConversationId>,
    ApiJson(input): ApiJson<wire::MembershipInput>,
) -> Result<wire::Conversation> {
    Ok(Json(s.social.change_membership(id, input).await?))
}
async fn control_conversation(
    State(s): State<Arc<AppState>>,
    ApiPath(id): ApiPath<ConversationId>,
    ApiJson(input): ApiJson<wire::ControlInput>,
) -> Result<wire::Conversation> {
    Ok(Json(s.social.control_conversation(id, input).await?))
}
async fn update_participation(
    State(s): State<Arc<AppState>>,
    ApiPath(id): ApiPath<ConversationId>,
    ApiJson(input): ApiJson<wire::ParticipationInput>,
) -> Result<wire::Conversation> {
    Ok(Json(s.social.update_participation(id, input).await?))
}
async fn update_contact_settings(
    State(s): State<Arc<AppState>>,
    ApiPath(id): ApiPath<ConversationId>,
    ApiJson(input): ApiJson<wire::ContactSettingsInput>,
) -> Result<storage::social::ContactScheduleView> {
    Ok(Json(s.social.update_contact_settings(id, input).await?))
}
async fn create_contact_intent(
    State(s): State<Arc<AppState>>,
    ApiPath(id): ApiPath<ConversationId>,
    ApiJson(input): ApiJson<wire::ContactIntentInput>,
) -> Result<social::ContactIntent> {
    Ok(Json(s.social.create_contact_intent(id, input).await?))
}
async fn cancel_contact_intent(
    State(s): State<Arc<AppState>>,
    ApiPath((id, intent)): ApiPath<(ConversationId, social::IntentId)>,
    ApiJson(input): ApiJson<wire::VersionedCommand>,
) -> Result<social::ContactIntent> {
    Ok(Json(
        s.social.cancel_contact_intent(id, intent, input).await?,
    ))
}
async fn pause_member(
    State(s): State<Arc<AppState>>,
    ApiPath(id): ApiPath<ConversationId>,
    ApiJson(input): ApiJson<wire::PauseMemberInput>,
) -> Result<wire::Conversation> {
    Ok(Json(s.social.pause_member(id, input).await?))
}
async fn revise_message(
    State(s): State<Arc<AppState>>,
    ApiPath(id): ApiPath<MessageId>,
    ApiJson(input): ApiJson<wire::ReviseInput>,
) -> Result<wire::Message> {
    Ok(Json(s.social.revise_message(id, input).await?))
}
async fn withdraw_message(
    State(s): State<Arc<AppState>>,
    ApiPath(id): ApiPath<MessageId>,
    ApiJson(input): ApiJson<wire::VersionedCommand>,
) -> Result<wire::Message> {
    Ok(Json(s.social.withdraw_message(id, input).await?))
}
async fn message(
    State(s): State<Arc<AppState>>,
    ApiPath(id): ApiPath<MessageId>,
) -> Result<wire::Message> {
    Ok(Json(s.social.message(id).await?))
}
async fn plans(
    State(s): State<Arc<AppState>>,
    ApiPath(id): ApiPath<ConversationId>,
) -> Result<Vec<wire::Plan>> {
    Ok(Json(s.social.plans(id).await?))
}
async fn init(State(s): State<Arc<AppState>>) -> Result<wire::Init> {
    Ok(Json(s.social.init().await?))
}
async fn create_ai(
    State(s): State<Arc<AppState>>,
    ApiJson(input): ApiJson<wire::AiInput>,
) -> Result<wire::ManagedAi> {
    Ok(Json(s.social.create_ai(input).await?))
}
async fn update_ai(
    State(s): State<Arc<AppState>>,
    ApiPath(id): ApiPath<ActorId>,
    ApiJson(input): ApiJson<wire::UpdateAiInput>,
) -> Result<wire::ManagedAi> {
    Ok(Json(s.social.update_ai(id, input).await?))
}
async fn open_direct(
    State(s): State<Arc<AppState>>,
    ApiPath(id): ApiPath<ActorId>,
    ApiJson(input): ApiJson<wire::Command>,
) -> Result<ConversationId> {
    Ok(Json(s.social.open_direct(id, input).await?))
}
async fn control_account(
    State(s): State<Arc<AppState>>,
    ApiPath(id): ApiPath<ActorId>,
    ApiJson(input): ApiJson<wire::AccountControlInput>,
) -> Result<wire::Account> {
    Ok(Json(s.social.control_account(id, input).await?))
}
async fn conversation(
    State(s): State<Arc<AppState>>,
    ApiPath(id): ApiPath<ConversationId>,
) -> Result<wire::Conversation> {
    Ok(Json(s.social.conversation(id).await?))
}
#[derive(Default, Deserialize)]
struct MessageQuery {
    before: Option<Sequence>,
}
async fn messages(
    State(s): State<Arc<AppState>>,
    ApiPath(id): ApiPath<ConversationId>,
    ApiQuery(q): ApiQuery<MessageQuery>,
) -> Result<wire::MessagePage> {
    Ok(Json(s.social.messages(id, q.before).await?))
}
async fn send(
    State(s): State<Arc<AppState>>,
    ApiPath(id): ApiPath<ConversationId>,
    ApiJson(input): ApiJson<wire::SendInput>,
) -> Result<wire::SendReceipt> {
    Ok(Json(s.social.send(id, input).await?))
}
async fn run_details(
    State(s): State<Arc<AppState>>,
    ApiPath(id): ApiPath<RunId>,
) -> Result<Option<storage::social::RunDetails>> {
    Ok(Json(s.social.run_details(id).await?))
}
async fn cancel(
    State(s): State<Arc<AppState>>,
    ApiPath(id): ApiPath<RunId>,
) -> std::result::Result<StatusCode, ApiError> {
    s.social.cancel_run(id).await?;
    Ok(StatusCode::NO_CONTENT)
}
#[derive(Default, Deserialize)]
struct EventQuery {
    #[serde(default)]
    after: EventCursor,
}
async fn events(
    State(s): State<Arc<AppState>>,
    ApiQuery(q): ApiQuery<EventQuery>,
) -> Result<Vec<events::SocialEvent>> {
    Ok(Json(s.social.events(q.after).await?))
}

async fn subscribe(
    State(s): State<Arc<AppState>>,
    ApiQuery(q): ApiQuery<EventQuery>,
) -> std::result::Result<
    Sse<impl futures_util::Stream<Item = std::result::Result<Event, Infallible>>>,
    ApiError,
> {
    let subscription = s.social.subscribe(q.after).await?;
    let stream = futures_util::stream::unfold(
        (subscription, s.shutdown.clone(), false),
        |(mut subscription, shutdown, failed)| async move {
            if failed {
                return None;
            }
            let next = tokio::select! {
                _ = shutdown.cancelled() => return None,
                event = subscription.next() => event,
            };
            let (frame, failed) = match next {
                Ok(Some(wire::SubscriptionEvent::Change { event })) => (
                    Event::default()
                        .event("change")
                        .id(event.cursor.to_string())
                        .json_data(event),
                    false,
                ),
                Ok(Some(wire::SubscriptionEvent::Generation { event })) => {
                    (Event::default().event("generation").json_data(event), false)
                }
                Ok(Some(wire::SubscriptionEvent::Resync)) => {
                    (Ok(Event::default().event("resync").data("{}")), false)
                }
                Ok(None) => return None,
                Err(error) => (
                    Event::default().event("error").json_data(error.wire()),
                    true,
                ),
            };
            Some((
                Ok(frame.expect("social event serializes")),
                (subscription, shutdown, failed),
            ))
        },
    );
    Ok(Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(10))))
}

async fn account(
    State(s): State<Arc<AppState>>,
    ApiPath(id): ApiPath<ActorId>,
) -> Result<wire::Account> {
    Ok(Json(s.social.account(id).await?))
}
async fn managed_accounts(State(s): State<Arc<AppState>>) -> Result<Vec<wire::ManagedAi>> {
    Ok(Json(s.social.managed_accounts().await?))
}
async fn managed_account(
    State(s): State<Arc<AppState>>,
    ApiPath(id): ApiPath<ActorId>,
) -> Result<wire::ManagedAi> {
    Ok(Json(s.social.managed_account(id).await?))
}
async fn set_administrator(
    State(s): State<Arc<AppState>>,
    ApiPath(id): ApiPath<ActorId>,
    ApiJson(input): ApiJson<social_feature::AdministratorInput>,
) -> Result<wire::ManagedAi> {
    Ok(Json(s.social.set_administrator(id, input).await?))
}
async fn set_contact(
    State(s): State<Arc<AppState>>,
    ApiPath(id): ApiPath<ActorId>,
    ApiJson(input): ApiJson<social_feature::SetContactInput>,
) -> Result<social::ContactEntry> {
    Ok(Json(s.social.set_contact(id, input).await?))
}
async fn update_account(
    State(s): State<Arc<AppState>>,
    ApiPath(id): ApiPath<ActorId>,
    ApiJson(input): ApiJson<social_feature::AccountInput>,
) -> Result<wire::Account> {
    Ok(Json(s.social.update_account(id, input).await?))
}
async fn account_view(
    State(s): State<Arc<AppState>>,
    ApiPath(id): ApiPath<ActorId>,
) -> Result<wire::AccountView> {
    Ok(Json(s.social.account_view(id).await?))
}
async fn account_view_messages(
    State(s): State<Arc<AppState>>,
    ApiPath((id, conversation)): ApiPath<(ActorId, ConversationId)>,
    ApiQuery(q): ApiQuery<MessageQuery>,
) -> Result<wire::MessagePage> {
    Ok(Json(
        s.social
            .account_view_messages(id, conversation, q.before)
            .await?,
    ))
}

async fn search_messages(
    State(s): State<Arc<AppState>>,
    ApiQuery(input): ApiQuery<storage::social::SearchMessages>,
) -> Result<wire::MessageSearchPage> {
    Ok(Json(s.social.search_messages(input).await?))
}
async fn message_context(
    State(s): State<Arc<AppState>>,
    ApiPath(id): ApiPath<MessageId>,
) -> Result<wire::MessageContext> {
    Ok(Json(s.social.message_context(id).await?))
}
async fn update_conversation_list(
    State(s): State<Arc<AppState>>,
    ApiPath(id): ApiPath<ConversationId>,
    ApiJson(input): ApiJson<wire::ConversationListInput>,
) -> Result<storage::social::ConversationListState> {
    Ok(Json(s.social.update_conversation_list(id, input).await?))
}
