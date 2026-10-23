pub mod auth;
pub mod chat;
pub mod consciousness;
pub mod db;
mod groups;
mod management;
pub mod nodes;
pub mod participation;
use db as sqlx;

use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Request, State},
    http::{StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
};
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

pub struct AppState {
    pub relay: consciousness::RelayHub,
    pub pool: sqlx::PgPool,
    pub attachments: PathBuf,
    pub public_url: String,
    pub mail: auth::Mailer,
}
pub type Result<T> = std::result::Result<T, AppError>;
#[derive(Debug)]
pub struct AppError {
    pub status: StatusCode,
    pub code: &'static str,
    pub message: String,
}
impl AppError {
    pub fn bad(message: &str) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            code: "invalid_request",
            message: message.into(),
        }
    }
    pub fn unauthorized() -> Self {
        Self {
            status: StatusCode::UNAUTHORIZED,
            code: "unauthorized",
            message: "请重新登录".into(),
        }
    }
    pub fn forbidden() -> Self {
        Self {
            status: StatusCode::FORBIDDEN,
            code: "forbidden",
            message: "没有操作权限".into(),
        }
    }
    pub fn not_found() -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            code: "not_found",
            message: "内容不存在或不可访问".into(),
        }
    }
    pub fn conflict(message: &str) -> Self {
        Self {
            status: StatusCode::CONFLICT,
            code: "conflict",
            message: message.into(),
        }
    }
    pub fn internal() -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            code: "internal",
            message: "服务暂时无法完成操作".into(),
        }
    }
}
impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}
impl std::error::Error for AppError {}
impl From<sqlx::Error> for AppError {
    fn from(_: sqlx::Error) -> Self {
        Self::internal()
    }
}
impl From<std::io::Error> for AppError {
    fn from(_: std::io::Error) -> Self {
        Self::internal()
    }
}
impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(serde_json::json!({"code":self.code,"message":self.message})),
        )
            .into_response()
    }
}

struct Boundary {
    origin: String,
    auth_window: Mutex<(Instant, u32)>,
}
async fn boundary(State(state): State<Arc<Boundary>>, request: Request, next: Next) -> Response {
    let method = request.method();
    if !matches!(
        *method,
        axum::http::Method::GET | axum::http::Method::HEAD | axum::http::Method::OPTIONS
    ) {
        let headers = request.headers();
        let origin_matches = headers
            .get(header::ORIGIN)
            .is_none_or(|v| v == state.origin.as_str());
        let site_matches = headers
            .get("sec-fetch-site")
            .is_none_or(|v| v != "cross-site");
        if headers.get("x-llmn-client").is_none_or(|v| v != "1") || !origin_matches || !site_matches
        {
            return AppError::forbidden().into_response();
        }
        if request.uri().path().starts_with("/auth/") {
            let mut window = state.auth_window.lock().unwrap();
            if window.0.elapsed() >= Duration::from_secs(60) {
                *window = (Instant::now(), 0);
            }
            window.1 += 1;
            if window.1 > 120 {
                return (
                    StatusCode::TOO_MANY_REQUESTS,
                    Json(
                        serde_json::json!({"code":"rate_limited","message":"请求较多，请稍后再试"}),
                    ),
                )
                    .into_response();
            }
        }
    }
    let mut response = next.run(request).await;
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, "no-store".parse().unwrap());
    response
        .headers_mut()
        .insert("x-content-type-options", "nosniff".parse().unwrap());
    response
}

pub fn router(state: Arc<AppState>, web_dir: PathBuf) -> Router {
    let boundary_state = Arc::new(Boundary {
        origin: state.public_url.clone(),
        auth_window: Mutex::new((Instant::now(), 0)),
    });
    let api = Router::new()
        .merge(auth::routes().layer(DefaultBodyLimit::max(16 * 1024)))
        .merge(chat::routes().layer(DefaultBodyLimit::max(40 * 1024 * 1024)))
        .merge(participation::routes().layer(DefaultBodyLimit::max(16 * 1024)))
        .merge(nodes::routes().layer(DefaultBodyLimit::max(1024 * 1024)))
        .fallback(|| async { AppError::not_found() })
        .layer(middleware::from_fn_with_state(boundary_state, boundary));
    Router::new()
        .route("/health", axum::routing::get(|| async { "ok" }))
        .nest("/api", api)
        .fallback_service(
            tower_http::services::ServeDir::new(&web_dir).not_found_service(
                tower_http::services::ServeFile::new(web_dir.join("index.html")),
            ),
        )
        .with_state(state)
}
