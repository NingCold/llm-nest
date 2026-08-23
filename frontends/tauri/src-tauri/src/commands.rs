use std::path::PathBuf;

use ai_client::ModelSelection;
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionSummary {
    pub id: String,
    pub title: String,
    pub created_at: String,
    pub updated_at: String,
    pub message_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GuiChatParams {
    pub session_id: String,
    pub input: String,
    pub model: ModelSelection,
    pub temperature: f32,
    pub max_tokens: Option<u32>,
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
    let cancel = tokio_util::sync::CancellationToken::new();
    {
        let mut map = state.cancel_map.lock().await;
        map.insert(params.session_id.clone(), cancel.clone());
    }

    let app_clone = app.clone();
    let cancel_map = state.cancel_map.clone();
    let session_id_str = params.session_id.clone();

    tauri::async_runtime::spawn(async move {
        use tokio::time::{Duration, sleep};

        let msg_id = format!("test-{}", params.session_id);

        let _ = app_clone.emit(
            "chat-event",
            GuiEvent {
                r#type: "delta".into(),
                message_id: msg_id.clone(),
                content: Some("Hello from Rust! ".into()),
                error: None,
            },
        );

        sleep(Duration::from_secs(1)).await;

        let _ = app_clone.emit(
            "chat-event",
            GuiEvent {
                r#type: "delta".into(),
                message_id: msg_id.clone(),
                content: Some("This is a test message. ".into()),
                error: None,
            },
        );

        sleep(Duration::from_secs(1)).await;

        let _ = app_clone.emit(
            "chat-event",
            GuiEvent {
                r#type: "finished".into(),
                message_id: msg_id,
                content: None,
                error: None,
            },
        );

        let mut map = cancel_map.lock().await;
        map.remove(&session_id_str);
    });

    Ok(())
}

#[tauri::command]
pub async fn cancel_chat(state: State<'_, AppState>, session_id: String) -> Result<(), String> {
    let mut map = state.cancel_map.lock().await;
    if let Some(token) = map.remove(&session_id) {
        token.cancel();
    }
    Ok(())
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

#[tauri::command]
pub async fn list_sessions(state: State<'_, AppState>) -> Result<Vec<SessionSummary>, String> {
    Ok(build_sessions(&state.runtime).await)
}

#[tauri::command]
pub async fn set_config(_state: State<'_, AppState>, _config: GuiConfig) -> Result<(), String> {
    Ok(())
}

// ─── Helpers ─────────────────────────────────────────────────────────────

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
