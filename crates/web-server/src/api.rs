//! HTTP handlers: session CRUD, message history, and streaming chat via SSE.
//! Wire types are camelCase to match the frontend `ChatApi` contract.

use std::collections::HashMap;
use std::convert::Infallible;
use std::sync::Arc;

use ai_client::{ModelSelection, Protocol};
use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::{StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, patch, post};
use axum::{Json, Router};
use base64::Engine;
use common::{ContentPart, MessageTimings, Role, SessionId, Usage};
use events::ChatEvent;
use futures_util::StreamExt;
use runtime::config::persist::{ProviderDraft, ProviderModelDraft};
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

pub use runtime::config::GuiConfig;

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

/// A builtin catalog provider the web settings can materialize (add key /
/// tweak endpoint), DSH-style "known route".
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderTemplate {
    pub id: String,
    pub display_name: String,
    pub protocol: String,
    pub base_url: String,
    /// Conventional env var for the key, as a hint.
    pub api_key_env: String,
    pub default_model: String,
    pub models: Vec<ProviderTemplateModel>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderTemplateModel {
    pub id: String,
    pub display_name: String,
}

/// Body of `POST /api/providers` — the web settings provider form.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderUpsertPayload {
    pub id: String,
    /// Wire protocol. Required when creating a provider the catalog does not
    /// already describe; absent on edits keeps the stored value.
    #[serde(default)]
    pub protocol: Option<ai_client::Protocol>,
    /// Endpoint override; absent/blank keeps the stored value (builtin fallback).
    #[serde(default)]
    pub base_url: Option<String>,
    /// Direct key to store; blank/absent keeps the existing field.
    #[serde(default)]
    pub api_key: Option<String>,
    /// Full intended model list. `None`/empty on a catalog provider keeps the
    /// builtin/configured models; a custom provider requires at least one.
    #[serde(default)]
    pub models: Option<Vec<ProviderModelPayload>>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderModelPayload {
    pub id: String,
    /// Wire model name; absent = the id itself.
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub display_name: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionSummary {
    pub model: Option<ModelSelection>,
    pub id: String,
    pub title: String,
    pub created_at: String,
    pub updated_at: String,
    pub message_count: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GuiMessage {
    pub revision: String,
    pub id: String,
    pub role: String,
    pub content: String,
    /// Persisted thinking chain, rendered as a collapsible card by the frontend.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<String>,
    pub status: String,
    pub error: Option<String>,
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
    #[serde(default)]
    pub edit: Option<runtime::session_manager::ChatEdit>,
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
    pub revision: String,
    pub feedback: Option<common::Feedback>,
}

// ─── Router ───────────────────────────────────────────────────────────────

pub fn routes(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/api/init", get(init_app))
        .route("/api/config", axum::routing::put(set_config))
        .route("/api/sessions", get(list_sessions).post(create_session))
        .route(
            "/api/sessions/{id}",
            delete(delete_session).patch(rename_session),
        )
        .route("/api/sessions/{id}/messages", get(get_messages))
        .route(
            "/api/sessions/{id}/messages/{message_id}",
            patch(set_message_feedback),
        )
        .route("/api/sessions/{id}/chat", post(chat))
        .route("/api/cancel", post(cancel_chat))
        // Provider management (web settings): builtin templates + create/update/delete.
        .route("/api/providers/templates", get(provider_templates))
        .route("/api/providers", post(upsert_provider))
        .route("/api/providers/{id}", delete(delete_provider))
        // Static frontend (production: built `frontends/web/dist`).
        .fallback(serve_static)
        .layer(axum::middleware::from_fn(local_request_guard))
        .with_state(state)
}

fn static_dir() -> String {
    std::env::var("LLMN_WEB_DIST").unwrap_or_else(|_| "frontends/web/dist".into())
}

/// Serve the built frontend with cache headers that keep rebuilds from
/// breaking open tabs:
/// - `/assets/*` — Vite content-hashed filenames are immutable: a long,
///   immutable cache is safe and correct (a rebuild changes the hash).
/// - everything else (`index.html`, `/icon.png`, …) — `no-cache`, so every
///   load revalidates and a new build is picked up immediately.
///
/// `index.html` previously rode ServeDir's default (no explicit cache header);
/// browsers then heuristically cached the *old* HTML referencing hashed
/// assets that a rebuild had already deleted → 404 on the JS → blank page.
async fn serve_static(uri: Uri) -> Response {
    serve_file(std::path::Path::new(&static_dir()), &uri).await
}

async fn serve_file(dist: &std::path::Path, uri: &Uri) -> Response {
    let rel = uri.path().trim_start_matches('/');
    // Path-traversal guard (redundant with the starts_with check, but cheap).
    if rel.contains("..") {
        return (StatusCode::NOT_FOUND, "not found").into_response();
    }
    let file = if rel.is_empty() {
        dist.join("index.html")
    } else {
        let candidate = dist.join(rel);
        if candidate.is_dir() {
            candidate.join("index.html")
        } else {
            candidate
        }
    };
    if !file.starts_with(dist) {
        return (StatusCode::NOT_FOUND, "not found").into_response();
    }
    match tokio::fs::read(&file).await {
        Ok(bytes) => {
            let cache = if rel.starts_with("assets/") {
                "public, max-age=31536000, immutable"
            } else {
                "no-cache"
            };
            (
                [
                    (header::CONTENT_TYPE, mime_for(&file)),
                    (header::CACHE_CONTROL, cache),
                ],
                bytes,
            )
                .into_response()
        }
        Err(_) => (StatusCode::NOT_FOUND, "not found").into_response(),
    }
}

fn mime_for(path: &std::path::Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()) {
        Some("html") => "text/html; charset=utf-8",
        Some("js") => "text/javascript",
        Some("css") => "text/css",
        Some("png") => "image/png",
        Some("svg") => "image/svg+xml",
        Some("ico") => "image/x-icon",
        Some("woff2") => "font/woff2",
        Some("json") | Some("map") => "application/json",
        _ => "application/octet-stream",
    }
}

/// The wire name of a protocol (`"openai"`, `"gemini"`, …) — the same strings
/// the frontend select sends back; `Protocol` has no `Display`.
fn protocol_wire(p: &Protocol) -> String {
    serde_json::to_string(p)
        .unwrap_or_default()
        .trim_matches('"')
        .to_string()
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
    Ok(Json(AppInit {
        config: state
            .runtime
            .gui_config()
            .await
            .map_err(|e| e.to_string())?,
        providers,
        sessions,
        version: env!("CARGO_PKG_VERSION").to_string(),
    }))
}

pub async fn set_config(
    State(state): State<Arc<AppState>>,
    Json(config): Json<GuiConfig>,
) -> ApiResult<StatusCode> {
    state
        .runtime
        .set_gui_config(&config)
        .await
        .map_err(|e| e.to_string())?;
    Ok(StatusCode::NO_CONTENT)
}

/// Builtin catalog providers the settings can materialize as real routes.
pub async fn provider_templates() -> ApiResult<Json<Vec<ProviderTemplate>>> {
    let templates = ai_client::catalog::BUILTIN_PROVIDERS
        .iter()
        .map(|e| ProviderTemplate {
            id: e.id.to_string(),
            display_name: e.display_name.to_string(),
            protocol: protocol_wire(&e.protocol),
            base_url: e.base_url.to_string(),
            api_key_env: e.api_key_env.to_string(),
            default_model: e.default_model.to_string(),
            models: e
                .models
                .iter()
                .map(|m| ProviderTemplateModel {
                    id: m.id.to_string(),
                    display_name: m.display_name.to_string(),
                })
                .collect(),
        })
        .collect();
    Ok(Json(templates))
}

/// A route id usable as a TOML key and provider id: lowercase kebab.
fn valid_provider_id(id: &str) -> bool {
    let mut chars = id.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_lowercase())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// Create or update a provider from the web settings form, then return the
/// refreshed provider list. The write is validated by the model router before
/// it swaps — an invalid provider keeps the old config and errors here.
pub async fn upsert_provider(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<ProviderUpsertPayload>,
) -> ApiResult<Json<Vec<ProviderInfo>>> {
    let id = payload.id.trim().to_string();
    if !valid_provider_id(&id) {
        return Err(ApiError(
            "provider id must be lowercase letters/digits/hyphens (e.g. acme-gateway)".into(),
        ));
    }
    let builtin = ai_client::catalog::builtin_provider(&id);
    // A protocol is required only when nothing can supply one: editing an
    // existing route or materializing a catalog entry may omit it.
    let existing = state
        .runtime
        .list_models()
        .await
        .iter()
        .any(|m| m.provider == id);
    let protocol = match payload.protocol {
        Some(p) => Some(protocol_wire(&p)),
        None if existing || builtin.is_some() => None,
        None => {
            return Err(ApiError(format!(
                "provider '{id}' needs a protocol (new custom provider)"
            )));
        }
    };
    let base_url = payload.base_url.filter(|s| !s.trim().is_empty());
    if base_url.is_none() && builtin.is_none() && !existing {
        return Err(ApiError(format!(
            "provider '{id}' needs a base_url (not in the builtin catalog)"
        )));
    }
    let api_key = payload.api_key.filter(|s| !s.trim().is_empty());
    let models = match payload.models {
        Some(list) if !list.is_empty() => {
            if list.iter().any(|m| m.id.trim().is_empty()) {
                return Err(ApiError("every model needs a non-empty id".into()));
            }
            Some(
                list.into_iter()
                    .map(|m| {
                        let id = m.id.trim().to_string();
                        let wire = m
                            .model
                            .filter(|s| !s.trim().is_empty())
                            .unwrap_or_else(|| id.clone());
                        ProviderModelDraft {
                            id,
                            wire,
                            display_name: m.display_name.filter(|s| !s.trim().is_empty()),
                        }
                    })
                    .collect(),
            )
        }
        // An explicit empty list only means "keep builtin/configured" — and a
        // custom provider has no builtin to fall back on, so it must list one.
        Some(_) if builtin.is_none() => {
            return Err(ApiError(format!(
                "custom provider '{id}' needs at least one model"
            )));
        }
        _ => None,
    };
    let draft = ProviderDraft {
        id: id.clone(),
        protocol,
        base_url,
        api_key,
        models,
    };
    state
        .runtime
        .upsert_provider(&draft)
        .await
        .map_err(|e| ApiError(e.to_string()))?;
    Ok(Json(build_providers(&state.runtime).await))
}

/// Remove a provider and return the refreshed list.
pub async fn delete_provider(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> ApiResult<Json<Vec<ProviderInfo>>> {
    state
        .runtime
        .remove_provider(&id)
        .await
        .map_err(|e| ApiError(e.to_string()))?;
    Ok(Json(build_providers(&state.runtime).await))
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
    Path((session_id, message_id)): Path<(String, String)>,
    Json(payload): Json<FeedbackPayload>,
) -> ApiResult<StatusCode> {
    let id: SessionId = session_id.parse().map_err(|e| format!("invalid id: {e}"))?;
    state
        .runtime
        .feedback_by_id(id, &message_id, &payload.revision, payload.feedback)
        .await
        .map_err(|e| e.to_string())?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn cancel_chat(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<CancelPayload>,
) -> ApiResult<StatusCode> {
    let map = state.cancel_map.lock().await;
    if let Some(token) = map.get(&payload.session_id) {
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
        if map.contains_key(&session_id) {
            return Err(ApiError(
                "a chat is already running for this session".into(),
            ));
        }
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
            .chat_with_edit(
                sid,
                input,
                attachments,
                model,
                options,
                cancel.clone(),
                params.edit,
            )
            .await
        {
            Ok(s) => s,
            Err(e) => {
                cleanup(cancel_map, &key).await;
                let _ = tx
                    .send(Ok(sse_frame(&GuiEvent::error(&wire_msg_id, e.to_string()))))
                    .await;
                return;
            }
        };

        loop {
            let event = tokio::select! {
                biased;
                _ = tx.closed() => break,
                event = stream.next() => match event { Some(event) => event, None => break },
            };
            let (gui, is_terminal) = chat_event_to_gui(event, &wire_msg_id);
            if is_terminal {
                cleanup(cancel_map, &key).await;
                let _ = tx.send(Ok(sse_frame(&gui))).await;
                return;
            }
            if tx.send(Ok(sse_frame(&gui))).await.is_err() {
                break;
            }
        }
        cancel.cancel();
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
        ChatEvent::ToolCall {
            id,
            name,
            arguments,
            ..
        } => (GuiEvent::tool_call(wire_msg_id, id, name, arguments), false),
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
        .map(|m| GuiMessage {
            revision: session.updated_at().to_rfc3339(),
            id: m.id.expect("stored messages have IDs").to_string(),
            role: match m.role {
                Role::User => "user",
                Role::Assistant => "assistant",
                Role::Tool => "tool",
                _ => unreachable!("filtered above"),
            }
            .to_string(),
            content: m.text(),
            reasoning: m.reasoning().map(str::to_string),
            status: match &m.interruption {
                Some(common::Interruption::Cancelled) => "cancelled",
                Some(common::Interruption::Failed(_)) => "error",
                None => "done",
            }
            .into(),
            error: match &m.interruption {
                Some(common::Interruption::Failed(error)) => Some(error.clone()),
                _ => None,
            },
            // Persisted `created_at` is Unix seconds; the frontend contract
            // (formatMessageTime / Date) is epoch milliseconds.
            created_at: m.created_at.map(|s| s * 1000),
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
        model: session.model().cloned(),
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

#[cfg(test)]
mod tests {
    use super::*;
    use common::Message;

    /// Regression: the persisted `created_at` is Unix seconds, but the wire
    /// contract (frontend `formatMessageTime` / `Date`) is epoch milliseconds.
    /// The old pass-through made reloaded messages render as 1970 timestamps
    /// (e.g. 00:33:07 instead of 00:02:27 +8h).
    #[test]
    fn messages_to_gui_converts_created_at_seconds_to_millis() {
        let mut session = runtime::session::Session::new(None);
        let mut assistant = Message::assistant("hi");
        assistant.created_at = Some(1_787_587_347); // 2026-08-25 00:02:27 +08
        session.push(assistant);

        let gui = messages_to_gui(&session);
        assert_eq!(gui[0].created_at, Some(1_787_587_347_000));
    }

    /// Messages without a persisted timestamp stay `None` (frontend falls back).
    #[test]
    fn messages_to_gui_keeps_missing_created_at_as_none() {
        let mut session = runtime::session::Session::new(None);
        session.push(Message::user("hello"));
        let gui = messages_to_gui(&session);
        assert!(gui[0].created_at.is_none());
    }

    /// Static serving: index.html revalidates every load (`no-cache`), hashed
    /// assets get a long immutable cache, missing files and traversal 404.
    #[tokio::test]
    async fn serve_file_cache_headers_and_guards() {
        let tmp = std::env::temp_dir().join(format!("llmn-static-{}", std::process::id()));
        let assets = tmp.join("assets");
        std::fs::create_dir_all(&assets).unwrap();
        std::fs::write(tmp.join("index.html"), "<html></html>").unwrap();
        std::fs::write(assets.join("app-hash.js"), "console.log(1)").unwrap();

        let html = serve_file(&tmp, &Uri::from_static("/")).await;
        assert_eq!(html.status(), StatusCode::OK);
        assert_eq!(html.headers()[header::CACHE_CONTROL], "no-cache");
        assert!(
            html.headers()[header::CONTENT_TYPE]
                .to_str()
                .unwrap()
                .starts_with("text/html")
        );

        let asset = serve_file(&tmp, &Uri::from_static("/assets/app-hash.js")).await;
        assert_eq!(asset.status(), StatusCode::OK);
        assert_eq!(
            asset.headers()[header::CACHE_CONTROL],
            "public, max-age=31536000, immutable"
        );
        assert!(
            asset.headers()[header::CONTENT_TYPE]
                .to_str()
                .unwrap()
                .starts_with("text/javascript")
        );

        let missing = serve_file(&tmp, &Uri::from_static("/nope.js")).await;
        assert_eq!(missing.status(), StatusCode::NOT_FOUND);

        let traversal = serve_file(&tmp, &Uri::from_static("/../etc/passwd")).await;
        assert_eq!(traversal.status(), StatusCode::NOT_FOUND);

        let _ = std::fs::remove_dir_all(&tmp);
    }
}

/// Loopback-only service: reject DNS rebinding and cross-site API requests.
/// The custom header forces cross-origin browsers to preflight; CORS is not enabled.
async fn local_request_guard(
    req: axum::extract::Request,
    next: axum::middleware::Next,
) -> Response {
    if let Err(error) = validate_local_request(&req) {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({ "error": error })),
        )
            .into_response();
    }
    next.run(req).await
}

fn validate_local_request(req: &axum::extract::Request) -> Result<(), &'static str> {
    let host = req
        .headers()
        .get(header::HOST)
        .and_then(|h| h.to_str().ok())
        .unwrap_or("");
    let hostname = host.split(':').next().unwrap_or("");
    if !matches!(hostname, "localhost" | "127.0.0.1") {
        return Err("invalid local host");
    }
    if req.uri().path().starts_with("/api/") {
        if req
            .headers()
            .get("x-llmn-client")
            .and_then(|v| v.to_str().ok())
            != Some("1")
        {
            return Err("missing local client header");
        }
        if let Some(origin) = req.headers().get(header::ORIGIN) {
            if origin.to_str().ok() != Some(format!("http://{host}").as_str()) {
                return Err("cross-origin request rejected");
            }
        }
    }
    Ok(())
}
#[cfg(test)]
mod local_access_tests {
    use super::*;
    #[test]
    fn rejects_cross_origin_rebinding_and_simple_requests() {
        let request = |host: &str, origin: &str, client: &str| {
            axum::http::Request::builder()
                .uri("/api/init")
                .header("host", host)
                .header("origin", origin)
                .header("x-llmn-client", client)
                .body(Body::empty())
                .unwrap()
        };
        assert!(
            validate_local_request(&request("localhost:5173", "http://localhost:5173", "1"))
                .is_ok()
        );
        assert!(
            validate_local_request(&request("127.0.0.1:8080", "https://evil.test", "1")).is_err()
        );
        assert!(
            validate_local_request(&request("evil.test:8080", "http://evil.test:8080", "1"))
                .is_err()
        );
        assert!(
            validate_local_request(&request("localhost:8080", "http://localhost:8080", ""))
                .is_err()
        );
    }
}
