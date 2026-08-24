//! HTTP handlers: session CRUD, message history, and streaming chat via SSE.
//! Wire types are camelCase to match the frontend `ChatApi` contract.

use std::collections::HashMap;
use std::convert::Infallible;
use std::sync::Arc;

use ai_client::ModelSelection;
use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, patch, post};
use axum::{Json, Router};
use base64::Engine;
use common::{ContentPart, MessageTimings, Role, SessionId, Usage};
use events::ChatEvent;
use futures_util::StreamExt;
use runtime::runtime::Runtime;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tokio_util::sync::CancellationToken;

use crate::AppState;

// ─── Wire types (camelCase, mirroring the Tauri command surface) ──────────

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInit {
    pub config: GuiConfig,
    pub providers: Vec<ProviderInfo>,
    pub sessions: Vec<SessionSummary>,
    pub version: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GuiConfig {
    pub current_model: ModelSelection,
    pub temperature: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderInfo {
    pub id: String,
    pub display_name: String,
    pub models: Vec<ModelInfo>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelInfo {
    pub id: String,
    pub display_name: String,
    /// 模型支持的思考强度（off/low/medium/high/max），来自模型快照；
    /// None = 模型不支持思考。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_levels: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionSummary {
    pub id: String,
    pub title: String,
    pub created_at: String,
    pub updated_at: String,
    pub message_count: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GuiMessage {
    pub id: String,
    pub role: String,
    pub content: String,
    /// Persisted thinking chain, rendered as a collapsible card by the frontend.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<String>,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<i64>,
    /// Thinking phase duration in milliseconds (persisted per message).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking_ms: Option<u64>,
    /// Normalized token usage of the turn (prompt/completion/total/cached).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<Usage>,
    /// Timing statistics (ttft / thinking / total).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timings: Option<MessageTimings>,
    /// User feedback (up/down), persisted with the message.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub feedback: Option<common::Feedback>,
    /// Attachments (images/files) carried by this message, as data URLs —
    /// restored losslessly from the persisted form.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub attachments: Vec<GuiAttachment>,
    /// Tool calls (assistant messages) / results (tool messages) carried by
    /// this message, extracted from the content blocks.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<GuiToolBlock>,
}

/// One tool call or result attached to a message for frontend rendering.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GuiToolBlock {
    /// `"call"` or `"result"`.
    pub kind: String,
    pub id: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arguments: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_error: Option<bool>,
    /// Tool execution duration in ms (result blocks only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GuiAttachment {
    pub id: String,
    pub name: String,
    pub mime: String,
    pub size: usize,
    pub data_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GuiChatParams {
    pub session_id: String,
    #[serde(default)]
    pub message_id: String,
    pub input: String,
    pub model: ModelSelection,
    pub temperature: f32,
    #[serde(default)]
    pub max_tokens: Option<u32>,
    #[serde(default)]
    pub attachments: Vec<GuiAttachment>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GuiEvent {
    pub r#type: String,
    pub message_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<Usage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timings: Option<MessageTimings>,
    // Tool-call / tool-result event fields (present only for those types).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_arguments: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_error: Option<bool>,
    /// Tool execution duration in ms (tool_result events only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
}

impl GuiEvent {
    fn delta(message_id: &str, content: String) -> Self {
        Self {
            r#type: "delta".into(),
            message_id: message_id.into(),
            content: Some(content),
            error: None,
            usage: None,
            timings: None,
            tool_id: None,
            tool_name: None,
            tool_arguments: None,
            tool_content: None,
            is_error: None,
            duration_ms: None,
        }
    }

    fn reasoning(message_id: &str, content: String) -> Self {
        Self {
            r#type: "reasoning_delta".into(),
            message_id: message_id.into(),
            content: Some(content),
            error: None,
            usage: None,
            timings: None,
            tool_id: None,
            tool_name: None,
            tool_arguments: None,
            tool_content: None,
            is_error: None,
            duration_ms: None,
        }
    }

    fn finished(message_id: &str, usage: Option<Usage>, timings: Option<MessageTimings>) -> Self {
        Self {
            r#type: "finished".into(),
            message_id: message_id.into(),
            content: None,
            error: None,
            usage,
            timings,
            tool_id: None,
            tool_name: None,
            tool_arguments: None,
            tool_content: None,
            is_error: None,
            duration_ms: None,
        }
    }

    fn error(message_id: &str, error: String) -> Self {
        Self {
            r#type: "error".into(),
            message_id: message_id.into(),
            content: None,
            error: Some(error),
            usage: None,
            timings: None,
            tool_id: None,
            tool_name: None,
            tool_arguments: None,
            tool_content: None,
            is_error: None,
            duration_ms: None,
        }
    }

    fn cancelled(message_id: &str) -> Self {
        Self {
            r#type: "cancelled".into(),
            message_id: message_id.into(),
            content: None,
            error: None,
            usage: None,
            timings: None,
            tool_id: None,
            tool_name: None,
            tool_arguments: None,
            tool_content: None,
            is_error: None,
            duration_ms: None,
        }
    }

    fn tool_call(message_id: &str, id: String, name: String, arguments: String) -> Self {
        Self {
            r#type: "tool_call".into(),
            message_id: message_id.into(),
            content: None,
            error: None,
            usage: None,
            timings: None,
            tool_id: Some(id),
            tool_name: Some(name),
            tool_arguments: Some(arguments),
            tool_content: None,
            is_error: None,
            duration_ms: None,
        }
    }

    fn tool_result(
        message_id: &str,
        id: String,
        name: String,
        content: String,
        is_error: bool,
        duration_ms: Option<u64>,
    ) -> Self {
        Self {
            r#type: "tool_result".into(),
            message_id: message_id.into(),
            content: None,
            error: None,
            usage: None,
            timings: None,
            tool_id: Some(id),
            tool_name: Some(name),
            tool_arguments: None,
            tool_content: Some(content),
            is_error: Some(is_error),
            duration_ms,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenamePayload {
    pub title: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CancelPayload {
    pub session_id: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FeedbackPayload {
    pub feedback: Option<common::Feedback>,
}

// ─── Router ───────────────────────────────────────────────────────────────

pub fn routes(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/api/init", get(init_app))
        .route("/api/sessions", get(list_sessions).post(create_session))
        .route(
            "/api/sessions/{id}",
            delete(delete_session).patch(rename_session),
        )
        .route("/api/sessions/{id}/messages", get(get_messages))
        .route(
            "/api/sessions/{id}/messages/{idx}",
            patch(set_message_feedback),
        )
        .route("/api/sessions/{id}/chat", post(chat))
        .route("/api/cancel", post(cancel_chat))
        // Static frontend (production: built `frontends/web/dist`).
        .fallback_service(
            tower_http::services::ServeDir::new(static_dir())
                .append_index_html_on_directories(true),
        )
        .layer(tower_http::cors::CorsLayer::permissive())
        .with_state(state)
}

fn static_dir() -> String {
    std::env::var("LLMN_WEB_DIST").unwrap_or_else(|_| "frontends/web/dist".into())
}

// ─── Handlers ─────────────────────────────────────────────────────────────

type ApiResult<T> = Result<T, ApiError>;

#[derive(Debug)]
pub struct ApiError(String);

impl From<String> for ApiError {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": self.0 })),
        )
            .into_response()
    }
}

pub async fn init_app(State(state): State<Arc<AppState>>) -> ApiResult<Json<AppInit>> {
    let sessions = build_sessions(&state.runtime).await;
    let providers = build_providers(&state.runtime).await;
    let current_model = state
        .runtime
        .default_model()
        .await
        .unwrap_or_else(|| ModelSelection {
            provider: String::new(),
            model: String::new(),
            reasoning_effort: None,
        });
    Ok(Json(AppInit {
        config: GuiConfig {
            current_model,
            temperature: 0.7,
            max_tokens: None,
        },
        providers,
        sessions,
        version: env!("CARGO_PKG_VERSION").to_string(),
    }))
}

pub async fn list_sessions(
    State(state): State<Arc<AppState>>,
) -> ApiResult<Json<Vec<SessionSummary>>> {
    Ok(Json(build_sessions(&state.runtime).await))
}

pub async fn create_session(State(state): State<Arc<AppState>>) -> ApiResult<Json<SessionSummary>> {
    let id = state
        .runtime
        .create_session(None)
        .await
        .map_err(|e| e.to_string())?;
    let session = state
        .runtime
        .get_session(&id)
        .await
        .ok_or_else(|| "session creation failed".to_string())?;
    Ok(Json(to_summary(&session)))
}

pub async fn delete_session(
    State(state): State<Arc<AppState>>,
    Path(session_id): Path<String>,
) -> ApiResult<StatusCode> {
    let id: SessionId = session_id.parse().map_err(|e| format!("invalid id: {e}"))?;
    state
        .runtime
        .delete_session(id)
        .await
        .map_err(|e| e.to_string())?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn rename_session(
    State(state): State<Arc<AppState>>,
    Path(session_id): Path<String>,
    Json(payload): Json<RenamePayload>,
) -> ApiResult<StatusCode> {
    let id: SessionId = session_id.parse().map_err(|e| format!("invalid id: {e}"))?;
    state
        .runtime
        .rename_session(id, payload.title)
        .await
        .map_err(|e| e.to_string())?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn get_messages(
    State(state): State<Arc<AppState>>,
    Path(session_id): Path<String>,
) -> ApiResult<Json<Vec<GuiMessage>>> {
    let id: SessionId = session_id.parse().map_err(|e| format!("invalid id: {e}"))?;
    let session = state
        .runtime
        .get_session(&id)
        .await
        .ok_or_else(|| "session not found".to_string())?;
    Ok(Json(messages_to_gui(&session)))
}

/// Set (or clear, with `feedback: null`) the feedback on one message.
pub async fn set_message_feedback(
    State(state): State<Arc<AppState>>,
    Path((session_id, idx)): Path<(String, usize)>,
    Json(payload): Json<FeedbackPayload>,
) -> ApiResult<StatusCode> {
    let id: SessionId = session_id.parse().map_err(|e| format!("invalid id: {e}"))?;
    state
        .runtime
        .set_message_feedback(id, idx, payload.feedback)
        .await
        .map_err(|e| e.to_string())?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn cancel_chat(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<CancelPayload>,
) -> ApiResult<StatusCode> {
    let mut map = state.cancel_map.lock().await;
    if let Some(token) = map.remove(&payload.session_id) {
        token.cancel();
    }
    Ok(StatusCode::NO_CONTENT)
}

/// Streaming chat: consumes the ChatFeature event stream and forwards each
/// event as an SSE `data:` frame, with the frontend-precreated message id.
pub async fn chat(
    State(state): State<Arc<AppState>>,
    Path(session_id): Path<String>,
    Json(params): Json<GuiChatParams>,
) -> ApiResult<Response> {
    let sid: SessionId = session_id
        .parse()
        .map_err(|e| format!("invalid session id: {e}"))?;

    let cancel = CancellationToken::new();
    {
        let mut map = state.cancel_map.lock().await;
        map.insert(session_id.clone(), cancel.clone());
    }

    let (tx, rx) = mpsc::channel::<Result<String, Infallible>>(64);
    let chat = state.chat.clone();
    let cancel_map = state.cancel_map.clone();
    let key = session_id.clone();
    let wire_msg_id = params.message_id.clone();
    let input = params.input.clone();
    let model = params.model.clone();
    let temperature = params.temperature;
    let max_tokens = params.max_tokens;
    let attachments: Vec<ContentPart> = params
        .attachments
        .iter()
        .filter_map(data_url_to_content_part)
        .collect();

    tokio::spawn(async move {
        let options = common::GenerationOptions {
            stream: true,
            temperature: Some(temperature),
            max_tokens,
            top_p: None,
        };

        let mut stream = match chat
            .chat(sid, input, attachments, model, options, cancel.clone())
            .await
        {
            Ok(s) => s,
            Err(e) => {
                let _ = tx
                    .send(Ok(sse_frame(&GuiEvent::error(&wire_msg_id, e.to_string()))))
                    .await;
                cleanup(cancel_map, &key).await;
                return;
            }
        };

        while let Some(event) = stream.next().await {
            let (gui, is_terminal) = chat_event_to_gui(event, &wire_msg_id);
            if tx.send(Ok(sse_frame(&gui))).await.is_err() {
                break;
            }
            if is_terminal {
                break;
            }
        }
        cleanup(cancel_map, &key).await;
    });

    let body = Body::from_stream(ReceiverStream::new(rx));
    Ok(Response::builder()
        .header(header::CONTENT_TYPE, "text/event-stream")
        .header(header::CACHE_CONTROL, "no-cache")
        .header(header::CONNECTION, "keep-alive")
        .body(body)
        .expect("static response builder"))
}

// ─── Helpers ──────────────────────────────────────────────────────────────

fn sse_frame(event: &GuiEvent) -> String {
    let json = serde_json::to_string(event).unwrap_or_else(|_| "{}".into());
    format!("data: {json}\n\n")
}

fn chat_event_to_gui(event: ChatEvent, wire_msg_id: &str) -> (GuiEvent, bool) {
    match event {
        ChatEvent::Delta { content, .. } => (GuiEvent::delta(wire_msg_id, content), false),
        ChatEvent::ReasoningDelta { content, .. } => {
            (GuiEvent::reasoning(wire_msg_id, content), false)
        }
        ChatEvent::Finished { usage, timings, .. } => {
            (GuiEvent::finished(wire_msg_id, usage, timings), true)
        }
        ChatEvent::ToolCall { id, name, arguments, .. } => (
            GuiEvent::tool_call(wire_msg_id, id, name, arguments),
            false,
        ),
        ChatEvent::ToolResult {
            id,
            name,
            content,
            is_error,
            duration_ms,
            ..
        } => (
            GuiEvent::tool_result(wire_msg_id, id, name, content, is_error, duration_ms),
            false,
        ),
        ChatEvent::Error { error, .. } => (GuiEvent::error(wire_msg_id, error), true),
        ChatEvent::Cancelled { .. } => (GuiEvent::cancelled(wire_msg_id), true),
    }
}

fn messages_to_gui(session: &runtime::session::Session) -> Vec<GuiMessage> {
    session
        .messages()
        .iter()
        .filter(|m| matches!(m.role, Role::User | Role::Assistant | Role::Tool))
        .enumerate()
        .map(|(i, m)| GuiMessage {
            id: format!("m-{i}"),
            role: match m.role {
                Role::User => "user",
                Role::Assistant => "assistant",
                Role::Tool => "tool",
                _ => unreachable!("filtered above"),
            }
            .to_string(),
            content: m.text(),
            reasoning: m.reasoning().map(str::to_string),
            status: "done".into(),
            created_at: m.created_at,
            thinking_ms: m.thinking_ms,
            usage: m.usage.clone(),
            timings: m.timings,
            feedback: m.feedback,
            attachments: m
                .content
                .iter()
                .enumerate()
                .filter_map(|(j, part)| match part {
                    ContentPart::Image { mime, data } => {
                        let mime = mime.clone().unwrap_or_else(|| "image/png".to_string());
                        let meta = format!("data:{mime};base64,");
                        Some(GuiAttachment {
                            id: format!("a-{j}"),
                            name: format!("image{}", ext_for_mime(&mime)),
                            mime: mime.clone(),
                            size: data.len(),
                            data_url: format!(
                                "{meta}{}",
                                base64::engine::general_purpose::STANDARD.encode(data)
                            ),
                        })
                    }
                    ContentPart::File { mime, data } => Some(GuiAttachment {
                        id: format!("a-{j}"),
                        name: format!("file{}", ext_for_mime(mime)),
                        mime: mime.clone(),
                        size: data.len(),
                        data_url: format!(
                            "data:{mime};base64,{}",
                            base64::engine::general_purpose::STANDARD.encode(data)
                        ),
                    }),
                    _ => None,
                })
                .collect(),
            tools: m
                .content
                .iter()
                .filter_map(|part| match part {
                    ContentPart::ToolCall(tc) => Some(GuiToolBlock {
                        kind: "call".into(),
                        id: tc.id.clone(),
                        name: tc.name.clone(),
                        arguments: Some(tc.arguments.clone()),
                        content: None,
                        is_error: None,
                        duration_ms: None,
                    }),
                    ContentPart::ToolResult(tr) => Some(GuiToolBlock {
                        kind: "result".into(),
                        id: tr.id.clone(),
                        name: tr.name.clone(),
                        arguments: None,
                        content: Some(tr.content.clone()),
                        is_error: Some(tr.is_error),
                        duration_ms: tr.duration_ms,
                    }),
                    _ => None,
                })
                .collect(),
        })
        .collect()
}

/// File extension hint for attachment names.
fn ext_for_mime(mime: &str) -> &str {
    match mime {
        "image/png" => ".png",
        "image/jpeg" | "image/jpg" => ".jpg",
        "image/gif" => ".gif",
        "image/webp" => ".webp",
        "image/svg+xml" => ".svg",
        "application/pdf" => ".pdf",
        "text/plain" => ".txt",
        "application/json" => ".json",
        _ => "",
    }
}

fn data_url_to_content_part(a: &GuiAttachment) -> Option<ContentPart> {
    let (meta, b64) = a.data_url.split_once(',')?;
    let mime = meta
        .strip_prefix("data:")
        .and_then(|m| m.split(';').next())
        .filter(|m| !m.is_empty())
        .unwrap_or(&a.mime)
        .to_string();
    let data = base64::engine::general_purpose::STANDARD
        .decode(b64.trim())
        .ok()?;
    if a.mime.starts_with("image/") {
        Some(ContentPart::Image {
            mime: Some(mime),
            data,
        })
    } else {
        Some(ContentPart::File { mime, data })
    }
}

fn to_summary(session: &runtime::session::Session) -> SessionSummary {
    SessionSummary {
        id: session.id().to_string(),
        title: session.title().unwrap_or("Untitled").to_string(),
        created_at: session.created_at().to_rfc3339(),
        updated_at: session.updated_at().to_rfc3339(),
        message_count: session.messages().len(),
    }
}

async fn build_sessions(runtime: &Runtime) -> Vec<SessionSummary> {
    let ids = runtime.list_sessions().await;
    let mut sessions = Vec::with_capacity(ids.len());
    for id in &ids {
        if let Some(s) = runtime.get_session(id).await {
            sessions.push(to_summary(&s));
        }
    }
    sessions.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    sessions
}

async fn build_providers(runtime: &Runtime) -> Vec<ProviderInfo> {
    let models = runtime.list_models().await;
    let mut providers: Vec<ProviderInfo> = Vec::new();
    for m in models {
        // Group by provider; keep deterministic order (list_models is sorted).
        let provider = match providers.iter_mut().find(|p| p.id == m.provider) {
            Some(p) => p,
            None => {
                providers.push(ProviderInfo {
                    id: m.provider.clone(),
                    display_name: m.provider.clone(),
                    models: Vec::new(),
                });
                providers.last_mut().expect("just pushed")
            }
        };
        provider.models.push(ModelInfo {
            id: m.spec.id,
            display_name: m.spec.display_name,
            reasoning_levels: m
                .spec
                .reasoning
                .as_ref()
                .map(|r| r.levels.iter().map(|e| e.as_wire().to_string()).collect()),
        });
    }
    providers
}

async fn cleanup(
    cancel_map: Arc<tokio::sync::Mutex<HashMap<String, CancellationToken>>>,
    key: &str,
) {
    let mut map = cancel_map.lock().await;
    map.remove(key);
}
