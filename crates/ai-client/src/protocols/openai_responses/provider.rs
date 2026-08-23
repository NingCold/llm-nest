//! OpenAI Responses API provider (`POST {base}/responses`).
//!
//! Authentication and request plumbing mirror the chat-completions provider:
//! bearer auth, configured route headers, per-route timeout. Streaming uses
//! the shared SSE frame stream with the Responses event vocabulary from
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
use crate::protocols::openai::error::ErrorResponse;
use crate::protocols::sse::SseDataStream;
use crate::request::ChatRequest;
use crate::response::ProviderResponse;
use crate::stream::ChatStream;

#[derive(Debug, Clone)]
pub struct OpenAIResponsesProvider {
    pub name: String,
    client: reqwest::Client,
    headers: HeaderMap,
    api_key: String,
    url: String,
}

impl OpenAIResponsesProvider {
    /// Quickstart constructor: no extra headers, default client timeout.
    pub fn new(name: impl Into<String>, base_url: &str, api_key: String) -> Result<Self> {
        Self::with_options(name, base_url, api_key, &HashMap::new(), None)
    }

    /// Constructor carrying the provider route's configured headers and
    /// request timeout (same validation as the chat-completions provider).
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

        let url = format!("{}/responses", base_url.trim_end_matches('/'));
        Ok(Self {
            name: name.into(),
            client,
            headers: header_map,
            api_key,
            url,
        })
    }

    async fn send_request(&self, req: ChatRequest) -> Result<ProviderResponse> {
        let request = convert::to_request(&req);
        let response = self
            .client
            .post(&self.url)
            .headers(self.headers.clone())
            .bearer_auth(&self.api_key)
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
            .bearer_auth(&self.api_key)
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

    async fn error_from_response(response: reqwest::Response) -> AiError {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        match serde_json::from_str::<ErrorResponse>(&body) {
            Ok(err) => AiError::ApiError {
                status,
                message: err.error.message,
                kind: Some(err.error.kind),
                code: err.error.code,
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
impl AiProvider for OpenAIResponsesProvider {
    fn id(&self) -> String {
        self.name.clone()
    }

    fn supported_protocols(&self) -> &[Protocol] {
        &[Protocol::OpenAIResponses]
    }

    async fn complete(&self, req: ChatRequest) -> Result<ProviderResponse> {
        self.send_request(req).await
    }

    async fn complete_stream(&self, req: ChatRequest) -> Result<ChatStream> {
        self.send_request_stream(req).await
    }

    /// `GET {base}/models` on the same gateway (OpenAI-compatible shape).
    async fn list_models(&self) -> Result<Vec<crate::ai_provider::WireModel>> {
        let base = self.url.strip_suffix("/responses").unwrap_or(&self.url);
        let response = self
            .client
            .get(format!("{base}/models"))
            .headers(self.headers.clone())
            .bearer_auth(&self.api_key)
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
        }

        let body: ModelsResponse = response.json().await?;
        Ok(body
            .data
            .into_iter()
            .map(|m| crate::ai_provider::WireModel {
                id: m.id,
                display_name: None,
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
    fn appends_responses_endpoint() {
        let provider = OpenAIResponsesProvider::new("demo", "https://example.com/v1", "k".into())
            .expect("provider builds");
        assert_eq!(provider.url, "https://example.com/v1/responses");
    }

    #[test]
    fn trims_trailing_slash() {
        let provider = OpenAIResponsesProvider::new("demo", "https://example.com/v1/", "k".into())
            .expect("provider builds");
        assert_eq!(provider.url, "https://example.com/v1/responses");
    }
}
