//! HTTP server exposing the LLM Nest runtime to web frontends.
//!
//! Routes mirror the Tauri command surface (see `frontends/tauri/src-tauri/
//! src/commands.rs`) so a single `ChatApi`-shaped adapter works against both:
//! the browser talks to this server over HTTP + SSE, the desktop shell talks
//! over Tauri IPC.

pub mod api;

use std::collections::HashMap;
use std::sync::Arc;

use runtime::runtime::Runtime;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

/// Shared server state: the runtime plus the chat feature (mirrors the Tauri
/// `AppState`).
pub struct AppState {
    pub runtime: Runtime,
    pub chat: Arc<chat::ChatFeature>,
    pub cancel_map: Arc<Mutex<HashMap<String, CancellationToken>>>,
}

/// Build the axum router for the given state.
pub fn router(state: Arc<AppState>) -> axum::Router {
    api::routes(state)
}
