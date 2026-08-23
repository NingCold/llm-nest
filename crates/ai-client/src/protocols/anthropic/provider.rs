//! Anthropic Messages API provider (`POST {base}/messages`).
//!
//! Authentication differs from the OpenAI family: `x-api-key` plus
//! `anthropic-version` headers instead of Bearer. Configured route headers
//! and the per-route timeout apply as elsewhere. Streaming uses the shared
//! SSE frame stream with the Anthropic event vocabulary from
//! [`convert::parse_event`].

use std::collections::HashMap;
use std::time::Duration;

use async_trait::async_trait;
use futures_util::StreamExt;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};

use super::convert;
use crate::ai_provider::AiProvider;
use crate::config::Protocol;
use crate::error::{AiError, Result};
use crate::protocols::sse::SseDataStream;
use crate::request::ChatRequest;
use crate::response::ProviderResponse;
use crate::stream::ChatStream;

/// Anthropic requires this version header on every request.
const ANTHROPIC_VERSION: &str = "2023-06-01";

#[derive(Debug, Clone)]
pub struct AnthropicProvider {
    pub name: String,
    client: reqwest::Client,
    headers: HeaderMap,
    url: String,
}

impl AnthropicProvider {
    /// Quickstart constructor: no extra headers, default client timeout.
    pub fn new(name: impl Into<String>, base_url: &str, api_key: String) -> Result<Self> {
        Self::with_options(name, base_url, api_key, &HashMap::new(), None)
    }

    /// Constructor carrying the provider route's configured headers and
    /// request timeout. The `x-api-key` and `anthropic-version` headers are
    /// appended after the configured ones (they always win).
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
        header_map.insert(
            HeaderName::from_static("x-api-key"),
            HeaderValue::from_str(&api_key)
                .map_err(|e| AiError::ConfigError(format!("invalid api key header: {e}")))?,
        );
        header_map.insert(
            HeaderName::from_static("anthropic-version"),
            HeaderValue::from_static(ANTHROPIC_VERSION),
        );

        // `base_url` already carries the API prefix (`https://api.anthropic.com/v1`).
        let url = format!("{}/messages", base_url.trim_end_matches('/'));
        Ok(Self {
            name: name.into(),
            client,
            headers: header_map,
            url,
        })
    }

    async fn send_request(&self, req: ChatRequest) -> Result<ProviderResponse> {
        let request = convert::to_request(&req);
        let response = self
            .client
            .post(&self.url)
            .headers(self.headers.clone())
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
        let mut request = convert::to_request(&req);
        request.stream = true;
        let response = self
            .client
            .post(&self.url)
            .headers(self.headers.clone())
            .json(&request)
            .send()
            .await?;
        if !response.status().is_success() {
            return Err(Self::error_from_response(response).await);
        }
        let stream = SseDataStream::new(response).filter_map(|payload| async {
            match payload {
                Ok(data) => convert::parse_event(&data).transpose(),
                Err(err) => Some(Err(err)),
            }
        });
        Ok(ChatStream::new(stream))
    }

    /// Anthropic error body: `{ "type": "error", "error": { "type", "message" } }`.
    async fn error_from_response(response: reqwest::Response) -> AiError {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        #[derive(serde::Deserialize)]
        struct ErrorBody {
            error: ErrorDetail,
        }
        #[derive(serde::Deserialize)]
        struct ErrorDetail {
            #[serde(rename = "type")]
            kind: Option<String>,
            message: Option<String>,
        }
        match serde_json::from_str::<ErrorBody>(&body) {
            Ok(err) => AiError::ApiError {
                status,
                message: err.error.message.unwrap_or_default(),
                kind: err.error.kind,
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
impl AiProvider for AnthropicProvider {
    fn id(&self) -> String {
        self.name.clone()
    }

    fn supported_protocols(&self) -> &[Protocol] {
        &[Protocol::Anthropic]
    }

    async fn complete(&self, req: ChatRequest) -> Result<ProviderResponse> {
        self.send_request(req).await
    }

    async fn complete_stream(&self, req: ChatRequest) -> Result<ChatStream> {
        self.send_request_stream(req).await
    }

    /// Anthropic `GET {base}/models`; the route's `x-api-key` +
    /// `anthropic-version` headers apply as usual.
    async fn list_models(&self) -> Result<Vec<crate::ai_provider::WireModel>> {
        let base = self.url.strip_suffix("/messages").unwrap_or(&self.url);
        let response = self
            .client
            .get(format!("{base}/models"))
            .headers(self.headers.clone())
            .send()
            .await?;
        if !response.status().is_success() {
            return Err(Self::error_from_response(response).await);
        }

        #[derive(serde::Deserialize)]
        struct ModelsResponse {
            data: Vec<ModelEntry>,
        }
        #[derive(serde::Deserialize)]
        struct ModelEntry {
            id: String,
            #[serde(default)]
            display_name: Option<String>,
        }

        let body: ModelsResponse = response.json().await?;
        Ok(body
            .data
            .into_iter()
            .map(|m| crate::ai_provider::WireModel {
                id: m.id,
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
    fn appends_messages_endpoint_and_auth_headers() {
        let provider =
            AnthropicProvider::new("demo", "https://api.anthropic.com/v1", "sk-x".into())
                .expect("provider builds");
        assert_eq!(provider.url, "https://api.anthropic.com/v1/messages");
        assert_eq!(
            provider
                .headers
                .get("x-api-key")
                .and_then(|v| v.to_str().ok()),
            Some("sk-x")
        );
        assert_eq!(
            provider
                .headers
                .get("anthropic-version")
                .and_then(|v| v.to_str().ok()),
            Some(ANTHROPIC_VERSION)
        );
    }

    #[test]
    fn configured_headers_merge_with_auth_headers() {
        let provider = AnthropicProvider::with_options(
            "demo",
            "https://api.anthropic.com/v1",
            "sk-x".into(),
            &HashMap::from([("X-Custom".to_string(), "v1".to_string())]),
            None,
        )
        .expect("provider builds");
        assert_eq!(
            provider
                .headers
                .get("x-custom")
                .and_then(|v| v.to_str().ok()),
            Some("v1")
        );
        assert_eq!(
            provider
                .headers
                .get("x-api-key")
                .and_then(|v| v.to_str().ok()),
            Some("sk-x")
        );
    }
}
