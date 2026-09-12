use std::path::PathBuf;

use ai_client::ModelSelection;
use base64::Engine;
use common::{ContentPart, MessageTimings, Role, SessionId, Usage};
use events::ChatEvent;
use futures_util::StreamExt;
use runtime::runtime::Runtime;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, State};

fn find_config_path() -> Result<PathBuf, String> {
    let candidates = [PathBuf::from("config/llmn.toml"), {
        let mut p = std::env::current_exe().map_err(|e| e.to_string())?;
        p.pop();
        p.pop();
        p.pop();
        p.push("config/llmn.toml");
        p
    }];
    for p in &candidates {
        if p.exists() {
            return Ok(p.clone());
        }
    }
    Err(format!(
        "config/llmn.toml not found, tried: {:?}",
        candidates
    ))
}

use crate::AppState;

// ─── GUI data types (serializable) ───────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInit {
    pub config: GuiConfig,
    pub providers: Vec<ProviderInfo>,
    pub sessions: Vec<SessionSummary>,
    pub version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GuiConfig {
    pub current_model: ModelSelection,
    pub temperature: f32,
    pub max_tokens: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderInfo {
    pub id: String,
    pub display_name: String,
    pub models: Vec<ModelInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelInfo {
    pub id: String,
    pub display_name: String,
    /// 模型支持的思考强度（off/low/medium/high/max），来自配置声明；
    /// None = 模型不支持思考。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_levels: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionSummary {
    pub model: Option<ModelSelection>,
    pub id: String,
    pub title: String,
    pub created_at: String,
    pub updated_at: String,
    pub message_count: usize,
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
    /// 前端预创建的 assistant 消息 id；后端事件统一用它，保证流式追加能
    /// 落到前端 store 里对应的消息上（后端 ChatEvent 的 message_id 是
    /// 内部生成的，与前端无共享，故在命令层重映射）。
    #[serde(default)]
    pub message_id: String,
    pub input: String,
    pub model: ModelSelection,
    pub temperature: f32,
    pub max_tokens: Option<u32>,
    /// 多模态附件（Base64 dataUrl），命令层解码为 common::ContentPart
    /// 后随用户消息一起进入会话与请求。
    #[serde(default)]
    pub attachments: Vec<GuiAttachment>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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

/// 历史消息 DTO：后端存储的 common::Message 没有 id，id 由索引生成（会话内
/// 稳定）；created_at 为 epoch 毫秒（由持久化的 Unix 秒换算）。
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
    /// Epoch milliseconds (converted from the persisted Unix-seconds value).
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

// ─── Commands ────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn init_app(state: State<'_, AppState>) -> Result<AppInit, String> {
    let sessions = build_sessions(&state.runtime).await;
    let (providers, config) = build_providers_and_config()?;

    Ok(AppInit {
        config,
        providers,
        sessions,
        version: env!("CARGO_PKG_VERSION").to_string(),
    })
}

#[tauri::command]
pub async fn chat(
    app: AppHandle,
    state: State<'_, AppState>,
    params: GuiChatParams,
) -> Result<(), String> {
    let session_id: SessionId = params
        .session_id
        .parse()
        .map_err(|e| format!("invalid session id: {}", e))?;

    let cancel = tokio_util::sync::CancellationToken::new();
    {
        let mut map = state.cancel_map.lock().await;
        if map.contains_key(&params.session_id) {
            return Err("a chat is already running for this session".into());
        }
        map.insert(params.session_id.clone(), cancel.clone());
    }

    let app_clone = app.clone();
    let cancel_map = state.cancel_map.clone();
    let session_key = params.session_id.clone();
    let wire_msg_id = params.message_id.clone();
    let chat = state.chat.clone();
    let input = params.input.clone();
    let model = params.model.clone();
    let temperature = params.temperature;
    let max_tokens = params.max_tokens;
    let attachments: Vec<ContentPart> = params
        .attachments
        .iter()
        .filter_map(data_url_to_content_part)
        .collect();

    tauri::async_runtime::spawn(async move {
        let options = common::GenerationOptions {
            stream: true,
            temperature: Some(temperature),
            max_tokens,
            top_p: None,
        };

        let mut stream = match chat
            .chat_with_edit(
                session_id,
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
                cancel_map.lock().await.remove(&session_key);
                let _ = app_clone.emit("chat-event", GuiEvent::error(&wire_msg_id, e.to_string()));
                return;
            }
        };

        // 消费 ChatFeature 事件流，转发为 GUI 事件；message_id 一律用前端
        // 预创建的 id（前端 store 按它定位流式消息）。
        while let Some(event) = stream.next().await {
            let (gui, is_terminal) = chat_event_to_gui(event, &wire_msg_id);
            if is_terminal {
                cancel_map.lock().await.remove(&session_key);
                let _ = app_clone.emit("chat-event", gui);
                return;
            }
            if app_clone.emit("chat-event", gui).is_err() {
                break;
            }
        }

        cancel.cancel();
        cancel_map.lock().await.remove(&session_key);
    });

    Ok(())
}

#[tauri::command]
pub async fn cancel_chat(state: State<'_, AppState>, session_id: String) -> Result<(), String> {
    let map = state.cancel_map.lock().await;
    if let Some(token) = map.get(&session_id) {
        token.cancel();
    }
    Ok(())
}

#[tauri::command]
pub async fn get_messages(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<Vec<GuiMessage>, String> {
    let id: SessionId = session_id
        .parse()
        .map_err(|e| format!("invalid session id: {}", e))?;
    let session = state
        .runtime
        .get_session(&id)
        .await
        .ok_or("session not found")?;
    Ok(messages_to_gui(&session))
}

#[tauri::command]
pub async fn new_session(state: State<'_, AppState>) -> Result<SessionSummary, String> {
    let id = state
        .runtime
        .create_session(None)
        .await
        .map_err(|e| e.to_string())?;
    let session = state
        .runtime
        .get_session(&id)
        .await
        .ok_or("session creation failed")?;
    Ok(to_summary(&session))
}

#[tauri::command]
pub async fn delete_session(state: State<'_, AppState>, session_id: String) -> Result<(), String> {
    let id = session_id
        .parse()
        .map_err(|e| format!("invalid id: {}", e))?;
    state
        .runtime
        .delete_session(id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn rename_session(
    state: State<'_, AppState>,
    session_id: String,
    title: String,
) -> Result<(), String> {
    let id = session_id
        .parse()
        .map_err(|e| format!("invalid id: {}", e))?;
    state
        .runtime
        .rename_session(id, title)
        .await
        .map_err(|e| e.to_string())
}

/// Set (or clear, with `feedback: null`) the feedback on one message.
#[tauri::command]
pub async fn set_message_feedback(
    state: State<'_, AppState>,
    session_id: String,
    message_id: String,
    revision: String,
    feedback: Option<common::Feedback>,
) -> Result<(), String> {
    let id = session_id
        .parse()
        .map_err(|e| format!("invalid id: {}", e))?;
    state
        .runtime
        .feedback_by_id(id, &message_id, &revision, feedback)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn list_sessions(state: State<'_, AppState>) -> Result<Vec<SessionSummary>, String> {
    Ok(build_sessions(&state.runtime).await)
}

#[tauri::command]
pub async fn set_config(_state: State<'_, AppState>, _config: GuiConfig) -> Result<(), String> {
    Ok(())
}

// ─── Helpers ─────────────────────────────────────────────────────────────

/// 把前端 Base64 dataUrl（`data:<mime>;base64,<data>`）解码为
/// common::ContentPart（图片/文件），供多模态消息使用。
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

/// 把 ChatFeature 事件映射为 GUI 事件；返回 (事件, 是否终态)。
/// message_id 统一重映射为前端预创建的 id。
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

/// 会话历史 → GUI 消息：只暴露 user/assistant，id 用会话内索引生成
/// （后端存储的 Message 无 id；索引在会话生命周期内稳定）。
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
                        Some(GuiAttachment {
                            id: format!("a-{j}"),
                            name: format!("image{}", ext_for_mime(&mime)),
                            mime: mime.clone(),
                            size: data.len(),
                            data_url: format!(
                                "data:{mime};base64,{}",
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

fn build_providers_and_config() -> Result<(Vec<ProviderInfo>, GuiConfig), String> {
    let config_path = find_config_path()?;
    let config: runtime::config::RuntimeConfig = runtime::config::ConfigLoader::load(&config_path)
        .map_err(|e| format!("failed to load config: {}", e))?;

    let mut providers = Vec::new();
    let mut default_model = ModelSelection {
        provider: String::new(),
        model: String::new(),
        reasoning_effort: None,
    };

    for (provider_id, provider_config) in &config.providers {
        let display_name = provider_id.0.clone();
        let mut models = Vec::new();

        for (model_id, model_config) in &provider_config.models {
            let display_name = model_config
                .display_name
                .clone()
                .unwrap_or_else(|| model_id.0.clone());
            models.push(ModelInfo {
                id: model_id.0.clone(),
                display_name,
                reasoning_levels: model_config
                    .reasoning
                    .as_ref()
                    .map(|r| r.levels.iter().map(|e| e.as_wire().to_string()).collect()),
            });
        }

        if default_model.provider.is_empty() && !models.is_empty() {
            default_model = ModelSelection {
                provider: provider_id.0.clone(),
                model: models[0].id.clone(),
                reasoning_effort: None,
            };
        }

        providers.push(ProviderInfo {
            id: provider_id.0.clone(),
            display_name,
            models,
        });
    }

    let gui_config = GuiConfig {
        current_model: default_model,
        temperature: 0.7,
        max_tokens: None,
    };

    Ok((providers, gui_config))
}

#[cfg(test)]
mod tests {
    use super::*;
    use common::{ContentPart, Message, Role};

    fn attach(id: &str, mime: &str, data_url: &str) -> GuiAttachment {
        GuiAttachment {
            id: id.into(),
            name: format!("{id}.bin"),
            mime: mime.into(),
            size: data_url.len(),
            data_url: data_url.into(),
        }
    }

    #[test]
    fn data_url_decodes_image() {
        // "hi" in base64
        let a = attach("a1", "image/png", "data:image/png;base64,aGk=");
        let part = data_url_to_content_part(&a).expect("should decode");
        match part {
            ContentPart::Image { mime, data } => {
                assert_eq!(mime.as_deref(), Some("image/png"));
                assert_eq!(data, b"hi");
            }
            other => panic!("expected Image, got {:?}", other),
        }
    }

    #[test]
    fn data_url_decodes_file() {
        let a = attach("f1", "application/pdf", "data:application/pdf;base64,AAEC");
        let part = data_url_to_content_part(&a).expect("should decode");
        match part {
            ContentPart::File { mime, data } => {
                assert_eq!(mime, "application/pdf");
                assert_eq!(data, vec![0x00, 0x01, 0x02]);
            }
            other => panic!("expected File, got {:?}", other),
        }
    }

    #[test]
    fn data_url_falls_back_to_attachment_mime() {
        // meta 无 mime 前缀
        let a = attach("f2", "text/plain", "data:,aGVsbG8=");
        let part = data_url_to_content_part(&a).expect("should decode");
        match part {
            ContentPart::File { mime, data } => {
                assert_eq!(mime, "text/plain");
                assert_eq!(data, b"hello");
            }
            other => panic!("expected File, got {:?}", other),
        }
    }

    #[test]
    fn data_url_invalid_returns_none() {
        let a = attach("b1", "image/png", "data:image/png;base64,!!!not-base64!!!");
        assert!(data_url_to_content_part(&a).is_none());
    }

    #[test]
    fn chat_event_maps_to_gui_with_wire_id() {
        // 后端 message_id 与前端 wire id 不同 → 必须重映射
        let (ev, term) = chat_event_to_gui(
            ChatEvent::Delta {
                message_id: common::MessageId::new(),
                content: "hi".into(),
            },
            "ai-frontend-1",
        );
        assert!(!term);
        assert_eq!(ev.r#type, "delta");
        assert_eq!(ev.message_id, "ai-frontend-1");
        assert_eq!(ev.content.as_deref(), Some("hi"));

        let (ev, term) = chat_event_to_gui(
            ChatEvent::ReasoningDelta {
                message_id: common::MessageId::new(),
                content: "think".into(),
            },
            "ai-frontend-1",
        );
        assert!(!term);
        assert_eq!(ev.r#type, "reasoning_delta");
        assert_eq!(ev.message_id, "ai-frontend-1");

        let (ev, term) = chat_event_to_gui(
            ChatEvent::Finished {
                message_id: common::MessageId::new(),
                usage: Some(common::Usage {
                    prompt_tokens: 10,
                    completion_tokens: 20,
                    total_tokens: 30,
                    cached_tokens: 4,
                }),
                timings: Some(common::MessageTimings {
                    ttft_ms: Some(100),
                    reasoning_ms: Some(200),
                    total_ms: Some(300),
                }),
            },
            "ai-frontend-1",
        );
        assert!(term);
        assert_eq!(ev.r#type, "finished");
        assert_eq!(ev.message_id, "ai-frontend-1");
        let u = ev.usage.expect("usage passthrough");
        assert_eq!(u.prompt_tokens, 10);
        assert_eq!(u.completion_tokens, 20);
        assert_eq!(u.cached_tokens, 4);
        let t = ev.timings.expect("timings passthrough");
        assert_eq!(t.ttft_ms, Some(100));
        assert_eq!(t.reasoning_ms, Some(200));
        assert_eq!(t.total_ms, Some(300));

        let (ev, term) = chat_event_to_gui(
            ChatEvent::Error {
                message_id: common::MessageId::new(),
                error: "boom".into(),
            },
            "ai-frontend-1",
        );
        assert!(term);
        assert_eq!(ev.r#type, "error");
        assert_eq!(ev.error.as_deref(), Some("boom"));

        let (ev, term) = chat_event_to_gui(
            ChatEvent::Cancelled {
                message_id: common::MessageId::new(),
            },
            "ai-frontend-1",
        );
        assert!(term);
        assert_eq!(ev.r#type, "cancelled");
    }

    #[test]
    fn messages_to_gui_filters_and_builds_ids() {
        let mut session = runtime::session::Session::new(None);
        session.push(Message::system("sys"));
        session.push(Message::user("hello"));
        let mut assistant = Message::assistant("world");
        assistant.feedback = Some(common::Feedback::Down);
        // Persisted value is Unix seconds; the wire value must be ms.
        assistant.created_at = Some(1_787_587_347);
        session.push(assistant);
        session.push(Message::new(
            Role::Tool,
            vec![ContentPart::Text("tool".into())],
        ));

        let gui = messages_to_gui(&session);
        // system is skipped; user/assistant/tool are kept
        assert_eq!(gui.len(), 3);
        assert_eq!(gui[0].id, "m-0");
        assert_eq!(gui[0].role, "user");
        assert_eq!(gui[0].content, "hello");
        assert_eq!(gui[1].id, "m-1");
        assert_eq!(gui[1].role, "assistant");
        assert_eq!(gui[1].content, "world");
        assert_eq!(gui[0].status, "done");
        assert!(gui[0].created_at.is_none());
        // feedback transparently passes through
        assert_eq!(gui[1].feedback, Some(common::Feedback::Down));
        // created_at: persisted Unix seconds → wire epoch milliseconds
        assert_eq!(gui[1].created_at, Some(1_787_587_347_000));
        // tool-role messages are kept and carry their blocks in `tools`
        assert_eq!(gui[2].role, "tool");
        assert!(gui[2].tools.is_empty());
    }

    #[test]
    fn messages_to_gui_multimodal_joins_text() {
        let mut session = runtime::session::Session::new(None);
        session.push(Message::new(
            Role::User,
            vec![
                ContentPart::Text("看图：".into()),
                ContentPart::Image {
                    mime: Some("image/png".into()),
                    data: vec![1, 2, 3],
                },
            ],
        ));
        let gui = messages_to_gui(&session);
        assert_eq!(gui.len(), 1);
        assert_eq!(gui[0].content, "看图：");
    }
}
