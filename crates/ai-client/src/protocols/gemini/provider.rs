//! Gemini API provider. The model name lives in the URL path, so the endpoint
//! is assembled per request: `{base}/models/{model}:generateContent` (non-
//! streaming) and `:streamGenerateContent?alt=sse` (streaming). Auth is the
//! `x-goog-api-key` header (Gemini has no Bearer scheme); configured route
//! headers and the per-route timeout apply as elsewhere.

use std::collections::HashMap;
use std::time::Duration;

use async_trait::async_trait;
use futures_util::StreamExt;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};

use super::convert;
use crate::ai_provider::AiProvider;
use crate::chunk::ChatChunk;
use crate::config::Protocol;
use crate::error::{AiError, Result};
use crate::protocols::sse::SseDataStream;
use crate::request::ChatRequest;
use crate::response::ProviderResponse;
use crate::stream::ChatStream;

#[derive(Debug, Clone)]
pub struct GeminiProvider {
    pub name: String,
    client: reqwest::Client,
    headers: HeaderMap,
    api_key: String,
    base_url: String,
}

impl GeminiProvider {
    /// Quickstart constructor: no extra headers, default client timeout.
    pub fn new(name: impl Into<String>, base_url: &str, api_key: String) -> Result<Self> {
        Self::with_options(name, base_url, api_key, &HashMap::new(), None)
    }

    /// Constructor carrying the provider route's configured headers and
    /// request timeout (same validation as the other providers).
    pub fn with_options(
        name: impl Into<String>,
        base_url: &str,
        api_key: String,
        headers: &HashMap<String, String>,
        timeout_ms: Option<u64>,
    ) -> Result<Self> {
        if timeout_ms == Some(0) {
            return Err(AiError::ConfigError(
                "timeout_ms must be a positive number of milliseconds".into(),
            ));
        }
        let mut builder = reqwest::Client::builder();
        if let Some(ms) = timeout_ms {
            builder = builder.timeout(Duration::from_millis(ms));
        }
        let client = builder.build()?;

        let mut header_map = HeaderMap::new();
        for (name, value) in headers {
            let header_name = HeaderName::from_bytes(name.as_bytes())
                .map_err(|e| AiError::ConfigError(format!("invalid header name '{name}': {e}")))?;
            let header_value = HeaderValue::from_str(value).map_err(|e| {
                AiError::ConfigError(format!("invalid header value for '{name}': {e}"))
            })?;
            header_map.insert(header_name, header_value);
        }

        Ok(Self {
            name: name.into(),
            client,
            headers: header_map,
            api_key,
            base_url: base_url.trim_end_matches('/').to_string(),
        })
    }

    fn endpoint(&self, model: &str, stream: bool) -> String {
        let action = if stream {
            ":streamGenerateContent?alt=sse"
        } else {
            ":generateContent"
        };
        format!("{}/models/{model}{action}", self.base_url)
    }

    async fn send_request(&self, req: ChatRequest) -> Result<ProviderResponse> {
        let request = convert::to_request(&req);
        let response = self
            .client
            .post(self.endpoint(&req.selection.model, false))
            .headers(self.headers.clone())
            .header("x-goog-api-key", &self.api_key)
            .json(&request)
            .send()
            .await?;
        if !response.status().is_success() {
            return Err(Self::error_from_response(response).await);
        }
        let resp: convert::Response = response.json().await?;
        convert::to_provider_response(resp)
    }

    async fn send_request_stream(&self, req: ChatRequest) -> Result<ChatStream> {
        let request = convert::to_request(&req);
        let response = self
            .client
            .post(self.endpoint(&req.selection.model, true))
            .headers(self.headers.clone())
            .header("x-goog-api-key", &self.api_key)
            .json(&request)
            .send()
            .await?;
        if !response.status().is_success() {
            return Err(Self::error_from_response(response).await);
        }
        // parse_event yields Vec<ChatChunk>: text/reasoning deltas, functionCall
        // ToolCalls (possibly several per chunk), and a terminal Done only on
        // the LAST frame (finishReason / usage-only). Gemini rides usageMetadata
        // on every frame, so a premature Done would kill the stream at frame 1.
        // unfold keeps a pending queue and appends a synthetic final Done if the
        // body ends without one (abnormal termination still closes the turn).
        let stream = futures_util::stream::unfold(
            (
                SseDataStream::new(response),
                std::collections::VecDeque::new(),
                false,
            ),
            |(mut sse, mut pending, mut done_seen)| async move {
                loop {
                    if let Some(item) = pending.pop_front() {
                        return Some((item, (sse, pending, done_seen)));
                    }
                    if done_seen {
                        return None;
                    }
                    match sse.next().await {
                        Some(Ok(data)) => match convert::parse_event(&data) {
                            Ok(chunks) => {
                                if chunks.iter().any(|c| matches!(c, ChatChunk::Done { .. })) {
                                    done_seen = true;
                                }
                                pending.extend(chunks.into_iter().map(Ok));
                            }
                            Err(err) => pending.push_back(Err(err)),
                        },
                        Some(Err(err)) => pending.push_back(Err(err)),
                        None => {
                            // Body ended without a terminal frame: close with
                            // a bare Done so the turn always finishes cleanly.
                            done_seen = true;
                            pending.push_back(Ok(ChatChunk::Done { usage: None }));
                        }
                    }
                }
            },
        );
        Ok(ChatStream::new(stream))
    }

    /// Gemini error body: `{ "error": { "code", "message", "status" } }`.
    async fn error_from_response(response: reqwest::Response) -> AiError {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        #[derive(serde::Deserialize)]
        struct ErrorBody {
            error: ErrorDetail,
        }
        #[derive(serde::Deserialize)]
        struct ErrorDetail {
            message: Option<String>,
            status: Option<String>,
        }
        match serde_json::from_str::<ErrorBody>(&body) {
            Ok(err) => AiError::ApiError {
                status,
                message: err.error.message.unwrap_or_default(),
                kind: err.error.status,
                code: None,
            },
            Err(_) => AiError::ApiError {
                status,
                message: body,
                kind: None,
                code: None,
            },
        }
    }
}

#[async_trait]
impl AiProvider for GeminiProvider {
    fn id(&self) -> String {
        self.name.clone()
    }

    fn supported_protocols(&self) -> &[Protocol] {
        &[Protocol::Gemini]
    }

    async fn complete(&self, req: ChatRequest) -> Result<ProviderResponse> {
        self.send_request(req).await
    }

    async fn complete_stream(&self, req: ChatRequest) -> Result<ChatStream> {
        self.send_request_stream(req).await
    }

    /// Gemini `GET {base}/models`; model ids come as `models/<id>` and are
    /// stripped to the bare wire name. Auth is the `x-goog-api-key` header.
    async fn list_models(&self) -> Result<Vec<crate::ai_provider::WireModel>> {
        let response = self
            .client
            .get(format!("{}/models", self.base_url))
            .headers(self.headers.clone())
            .header("x-goog-api-key", &self.api_key)
            .send()
            .await?;
        if !response.status().is_success() {
            return Err(Self::error_from_response(response).await);
        }

        #[derive(serde::Deserialize)]
        struct ModelsResponse {
            #[serde(default)]
            models: Vec<ModelEntry>,
        }
        #[derive(serde::Deserialize)]
        struct ModelEntry {
            name: String,
            #[serde(default)]
            display_name: Option<String>,
        }

        let body: ModelsResponse = response.json().await?;
        Ok(body
            .models
            .into_iter()
            .map(|m| crate::ai_provider::WireModel {
                id: m
                    .name
                    .strip_prefix("models/")
                    .unwrap_or(&m.name)
                    .to_string(),
                display_name: m.display_name,
                context_window: None,
                max_tokens: None,
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_model_endpoints() {
        let provider = GeminiProvider::new(
            "gemini",
            "https://generativelanguage.googleapis.com/v1beta",
            "k".into(),
        )
        .expect("provider builds");
        assert_eq!(
            provider.endpoint("gemini-2.5-pro", false),
            "https://generativelanguage.googleapis.com/v1beta/models/gemini-2.5-pro:generateContent"
        );
        assert_eq!(
            provider.endpoint("gemini-2.5-pro", true),
            "https://generativelanguage.googleapis.com/v1beta/models/gemini-2.5-pro:streamGenerateContent?alt=sse"
        );
    }
}
