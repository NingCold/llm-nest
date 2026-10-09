use crate::AppState;
use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::StatusCode,
    response::{
        IntoResponse, Response,
        sse::{Event, KeepAlive, Sse},
    },
    routing::{get, post},
};
use serde::Deserialize;
use social::{ActorId, ConversationId, EventCursor, MessageId, RunId, Sequence};
use social_feature::{SocialError, wire};
use std::{convert::Infallible, sync::Arc, time::Duration};

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/social/init", get(init))
        .route("/api/social/contacts", post(create_contact))
        .route(
            "/api/social/contacts/{id}",
            axum::routing::patch(update_contact),
        )
        .route("/api/social/contacts/{id}/direct", post(open_direct))
        .route("/api/social/groups", post(create_group))
        .route(
            "/api/social/conversations/{id}/members",
            post(change_membership),
        )
        .route("/api/social/conversations/{id}/plans", get(plans))
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

struct ApiError(SocialError);
impl From<SocialError> for ApiError {
    fn from(e: SocialError) -> Self {
        Self(e)
    }
}
impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let error = self.0.wire();
        let status = match error.code {
            "invalid_input" => StatusCode::BAD_REQUEST,
            "not_found" => StatusCode::NOT_FOUND,
            "not_authorized" => StatusCode::FORBIDDEN,
            "revision_conflict" => StatusCode::CONFLICT,
            "model_unavailable" | "budget_exhausted" => StatusCode::UNPROCESSABLE_ENTITY,
            _ => StatusCode::SERVICE_UNAVAILABLE,
        };
        (status, Json(error)).into_response()
    }
}
type Result<T> = std::result::Result<Json<T>, ApiError>;
async fn create_group(
    State(s): State<Arc<AppState>>,
    Json(input): Json<wire::CreateGroupInput>,
) -> Result<ConversationId> {
    Ok(Json(s.social.create_group(input).await?))
}
async fn change_membership(
    State(s): State<Arc<AppState>>,
    Path(id): Path<ConversationId>,
    Json(input): Json<wire::MembershipInput>,
) -> Result<wire::Conversation> {
    Ok(Json(s.social.change_membership(id, input).await?))
}
async fn control_conversation(
    State(s): State<Arc<AppState>>,
    Path(id): Path<ConversationId>,
    Json(input): Json<wire::ControlInput>,
) -> Result<wire::Conversation> {
    Ok(Json(s.social.control_conversation(id, input).await?))
}
async fn pause_member(
    State(s): State<Arc<AppState>>,
    Path(id): Path<ConversationId>,
    Json(input): Json<wire::PauseMemberInput>,
) -> Result<wire::Conversation> {
    Ok(Json(s.social.pause_member(id, input).await?))
}
async fn revise_message(
    State(s): State<Arc<AppState>>,
    Path(id): Path<MessageId>,
    Json(input): Json<wire::ReviseInput>,
) -> Result<wire::Message> {
    Ok(Json(s.social.revise_message(id, input).await?))
}
async fn withdraw_message(
    State(s): State<Arc<AppState>>,
    Path(id): Path<MessageId>,
    Json(input): Json<wire::VersionedCommand>,
) -> Result<wire::Message> {
    Ok(Json(s.social.withdraw_message(id, input).await?))
}
async fn message(
    State(s): State<Arc<AppState>>,
    Path(id): Path<MessageId>,
) -> Result<wire::Message> {
    Ok(Json(s.social.message(id).await?))
}
async fn plans(
    State(s): State<Arc<AppState>>,
    Path(id): Path<ConversationId>,
) -> Result<Vec<wire::Plan>> {
    Ok(Json(s.social.plans(id).await?))
}
async fn init(State(s): State<Arc<AppState>>) -> Result<wire::Init> {
    Ok(Json(s.social.init().await?))
}
async fn create_contact(
    State(s): State<Arc<AppState>>,
    Json(input): Json<wire::ContactInput>,
) -> Result<wire::Contact> {
    Ok(Json(s.social.create_contact(input).await?))
}
async fn update_contact(
    State(s): State<Arc<AppState>>,
    Path(id): Path<ActorId>,
    Json(input): Json<wire::UpdateContactInput>,
) -> Result<wire::Contact> {
    Ok(Json(s.social.update_contact(id, input).await?))
}
async fn open_direct(
    State(s): State<Arc<AppState>>,
    Path(id): Path<ActorId>,
    Json(input): Json<wire::Command>,
) -> Result<ConversationId> {
    Ok(Json(s.social.open_direct(id, input).await?))
}
async fn conversation(
    State(s): State<Arc<AppState>>,
    Path(id): Path<ConversationId>,
) -> Result<wire::Conversation> {
    Ok(Json(s.social.conversation(id).await?))
}
#[derive(Default, Deserialize)]
struct MessageQuery {
    before: Option<Sequence>,
}
async fn messages(
    State(s): State<Arc<AppState>>,
    Path(id): Path<ConversationId>,
    Query(q): Query<MessageQuery>,
) -> Result<wire::MessagePage> {
    Ok(Json(s.social.messages(id, q.before).await?))
}
async fn send(
    State(s): State<Arc<AppState>>,
    Path(id): Path<ConversationId>,
    Json(input): Json<wire::SendInput>,
) -> Result<wire::SendReceipt> {
    Ok(Json(s.social.send(id, input).await?))
}
async fn run_details(
    State(s): State<Arc<AppState>>,
    Path(id): Path<RunId>,
) -> Result<Option<storage::social::RunDetails>> {
    Ok(Json(s.social.run_details(id).await?))
}
async fn cancel(
    State(s): State<Arc<AppState>>,
    Path(id): Path<RunId>,
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
    Query(q): Query<EventQuery>,
) -> Result<Vec<events::SocialEvent>> {
    Ok(Json(s.social.events(q.after).await?))
}

async fn subscribe(
    State(s): State<Arc<AppState>>,
    Query(q): Query<EventQuery>,
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
