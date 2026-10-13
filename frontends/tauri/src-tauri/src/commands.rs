//! Model-service settings. Chat and account operations live in social_commands.
use crate::BackendState;
use ai_client::Protocol;
use runtime::config::persist::{ProviderDraft, ProviderModelDraft};
use runtime::runtime::Runtime;
use serde::{Deserialize, Serialize};
use tauri::State;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsSnapshot {
    pub providers: Vec<ProviderInfo>,
    pub version: String,
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
    pub image_input: ai_client::image_input::ImageInputCapability,
    pub id: String,
    pub display_name: String,
    /// 模型支持的思考强度（off/low/medium/high/max），来自配置声明；
    /// None = 模型不支持思考。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_levels: Option<Vec<String>>,
}

#[tauri::command]
pub async fn settings_snapshot(state: State<'_, BackendState>) -> Result<SettingsSnapshot, String> {
    Ok(SettingsSnapshot {
        providers: build_providers(&state.get().await?.runtime).await,
        version: env!("CARGO_PKG_VERSION").into(),
    })
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
            image_input: m.spec.image_input,
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

#[tauri::command]
pub async fn set_model_image_input(
    state: State<'_, BackendState>,
    input: runtime::image_input::ImageInputUpdate,
) -> Result<Vec<ProviderInfo>, String> {
    let state = state.get().await?;
    state
        .runtime
        .set_model_image_input(input)
        .await
        .map_err(|e| e.to_string())?;
    Ok(build_providers(&state.runtime).await)
}

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

fn protocol_wire(p: &Protocol) -> String {
    serde_json::to_string(p)
        .unwrap_or_default()
        .trim_matches('"')
        .to_string()
}
/// Builtin catalog providers the settings can materialize as real routes.
#[tauri::command]
pub async fn provider_templates(
    state: State<'_, BackendState>,
) -> Result<Vec<ProviderTemplate>, String> {
    let state = state.get().await?;
    let mut templates = Vec::new();
    for e in ai_client::catalog::BUILTIN_PROVIDERS {
        templates.push(ProviderTemplate {
            id: e.id.to_string(),
            display_name: e.display_name.to_string(),
            protocol: protocol_wire(&e.protocol),
            base_url: e.base_url.to_string(),
            api_key_env: e.api_key_env.to_string(),
            default_model: e.default_model.to_string(),
            models: state
                .runtime
                .catalog_template_models(e.id)
                .await
                .into_iter()
                .map(|(id, display_name)| ProviderTemplateModel { id, display_name })
                .collect(),
        });
    }
    Ok(templates)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogSyncResponse {
    pub report: runtime::catalog::CatalogSyncReport,
    pub providers: Vec<ProviderInfo>,
}

#[tauri::command]
pub async fn catalog_status(
    state: State<'_, BackendState>,
) -> Result<runtime::catalog::CatalogStatus, String> {
    Ok(state.get().await?.runtime.catalog_status().await)
}

#[tauri::command]
pub async fn sync_catalog(state: State<'_, BackendState>) -> Result<CatalogSyncResponse, String> {
    let state = state.get().await?;
    let report = state
        .runtime
        .sync_catalog()
        .await
        .map_err(|e| e.to_string())?;
    Ok(CatalogSyncResponse {
        report,
        providers: build_providers(&state.runtime).await,
    })
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
#[tauri::command]
pub async fn upsert_provider(
    state: State<'_, BackendState>,
    payload: ProviderUpsertPayload,
) -> Result<Vec<ProviderInfo>, String> {
    let state = state.get().await?;
    let id = payload.id.trim().to_string();
    if !valid_provider_id(&id) {
        return Err(String::from(
            "provider id must be lowercase letters/digits/hyphens (e.g. acme-gateway)",
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
            return Err(format!(
                "provider '{id}' needs a protocol (new custom provider)"
            ));
        }
    };
    let base_url = payload.base_url.filter(|s| !s.trim().is_empty());
    if base_url.is_none() && builtin.is_none() && !existing {
        return Err(format!(
            "provider '{id}' needs a base_url (not in the builtin catalog)"
        ));
    }
    let api_key = payload.api_key.filter(|s| !s.trim().is_empty());
    let models = match payload.models {
        Some(list) if !list.is_empty() => {
            if list.iter().any(|m| m.id.trim().is_empty()) {
                return Err(String::from("every model needs a non-empty id"));
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
            return Err(format!("custom provider '{id}' needs at least one model"));
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
        .map_err(|e| e.to_string())?;
    Ok(build_providers(&state.runtime).await)
}

/// Remove a provider and return the refreshed list.
#[tauri::command]
pub async fn delete_provider(
    state: State<'_, BackendState>,
    id: String,
) -> Result<Vec<ProviderInfo>, String> {
    let state = state.get().await?;
    state
        .runtime
        .remove_provider(&id)
        .await
        .map_err(|e| e.to_string())?;
    Ok(build_providers(&state.runtime).await)
}
