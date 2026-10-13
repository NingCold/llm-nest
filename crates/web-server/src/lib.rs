pub mod api;
pub mod social_api;

use std::sync::Arc;

use runtime::runtime::Runtime;
use tokio_util::sync::CancellationToken;

/// Shared social runtime for all HTTP requests.
pub struct AppState {
    pub runtime: Runtime,
    pub social: Arc<social_feature::SocialFeature>,
    pub shutdown: CancellationToken,
}

/// Build the axum router for the given state.
pub fn router(state: Arc<AppState>) -> axum::Router {
    api::routes(state)
}
