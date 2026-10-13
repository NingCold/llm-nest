//! Model-service settings, local request guard, and static frontend hosting.
use std::sync::Arc;

use ai_client::Protocol;
#[cfg(test)]
use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::{StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use runtime::config::persist::{ProviderDraft, ProviderModelDraft};
use runtime::runtime::Runtime;
use serde::{Deserialize, Serialize};

use crate::AppState;

// ─── Wire types (camelCase, mirroring the Tauri command surface) ──────────

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsSnapshot {
    pub providers: Vec<ProviderInfo>,
    pub version: String,
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
    pub image_input: ai_client::image_input::ImageInputCapability,
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

// ─── Router ───────────────────────────────────────────────────────────────

pub fn routes(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/api/settings", get(settings_snapshot))
        // Provider management (web settings): builtin templates + create/update/delete.
        .route("/api/providers/templates", get(provider_templates))
        .route("/api/catalog", get(catalog_status))
        .route("/api/catalog/sync", post(sync_catalog))
        .route("/api/models/image-input", post(set_model_image_input))
        .route("/api/providers", post(upsert_provider))
        .route("/api/providers/{id}", delete(delete_provider))
        .merge(crate::social_api::routes())
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

pub async fn settings_snapshot(State(state): State<Arc<AppState>>) -> Json<SettingsSnapshot> {
    Json(SettingsSnapshot {
        providers: build_providers(&state.runtime).await,
        version: env!("CARGO_PKG_VERSION").into(),
    })
}

/// Builtin catalog providers the settings can materialize as real routes.
pub async fn provider_templates(
    State(state): State<Arc<AppState>>,
) -> ApiResult<Json<Vec<ProviderTemplate>>> {
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
    Ok(Json(templates))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogSyncResponse {
    pub report: runtime::catalog::CatalogSyncReport,
    pub providers: Vec<ProviderInfo>,
}

pub async fn catalog_status(
    State(state): State<Arc<AppState>>,
) -> Json<runtime::catalog::CatalogStatus> {
    Json(state.runtime.catalog_status().await)
}

pub async fn sync_catalog(
    State(state): State<Arc<AppState>>,
) -> ApiResult<Json<CatalogSyncResponse>> {
    let report = state
        .runtime
        .sync_catalog()
        .await
        .map_err(|e| ApiError(e.to_string()))?;
    Ok(Json(CatalogSyncResponse {
        report,
        providers: build_providers(&state.runtime).await,
    }))
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

pub async fn set_model_image_input(
    State(state): State<Arc<AppState>>,
    Json(input): Json<runtime::image_input::ImageInputUpdate>,
) -> ApiResult<Json<Vec<ProviderInfo>>> {
    state
        .runtime
        .set_model_image_input(input)
        .await
        .map_err(|e| ApiError(e.to_string()))?;
    Ok(Json(build_providers(&state.runtime).await))
}

#[cfg(test)]
mod tests {
    use super::*;
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
                .uri("/api/settings")
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
