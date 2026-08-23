use std::collections::HashMap;
use std::time::Duration;

use async_trait::async_trait;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};

use super::{chat::Response, convert, error::ErrorResponse, sse::OpenAIStream};
use crate::ai_provider::AiProvider;
use crate::config::Protocol;
use crate::error::{AiError, Result};
use crate::request::ChatRequest;
use crate::response::ProviderResponse;
use crate::stream::ChatStream;

#[derive(Debug, Clone)]
pub struct OpenAIProvider {
    pub name: String,
    client: reqwest::Client,
    /// Extra headers applied to every request, as configured per provider
    /// route. Kept separately so tests can assert what reaches the wire.
    headers: HeaderMap,
    api_key: String,
    url: String,
}

impl OpenAIProvider {
    /// Quickstart constructor: no extra headers, default client timeout.
    pub fn new(name: impl Into<String>, base_url: &str, api_key: String) -> Result<Self> {
        Self::with_options(name, base_url, api_key, &HashMap::new(), None)
    }

    /// Constructor carrying the provider route's configured headers and
    /// request timeout.
    ///
    /// Headers are validated at construction (invalid names or values fail
    /// naming the offending key) and attached to every request of this route.
    /// The timeout bounds the whole request, streaming reads included; a
    /// configured `0` is refused rather than silently meaning "no timeout".
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

        let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));
        Ok(Self {
            name: name.into(),
            client,
            headers: header_map,
            api_key,
            url,
        })
    }

    async fn send_request(&self, req: ChatRequest) -> Result<ProviderResponse> {
        let request = convert::to_real_request(&req);
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

        let chat_response: Response = response.json().await?;
        chat_response.try_into()
    }

    async fn send_request_stream(&self, req: ChatRequest) -> Result<ChatStream> {
        let mut request = convert::to_real_request(&req);
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

        Ok(ChatStream::new(OpenAIStream::new(response)))
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
impl AiProvider for OpenAIProvider {
    fn id(&self) -> String {
        self.name.clone()
    }

    fn supported_protocols(&self) -> &[Protocol] {
        &[Protocol::OpenAIChat]
    }

    async fn complete(&self, req: ChatRequest) -> Result<ProviderResponse> {
        self.send_request(req).await
    }

    async fn complete_stream(&self, req: ChatRequest) -> Result<ChatStream> {
        self.send_request_stream(req).await
    }

    /// OpenAI-compatible `GET {base}/models`; OpenRouter-style gateways
    /// publish `metadata.context_length`, which is picked up when present.
    async fn list_models(&self) -> Result<Vec<crate::ai_provider::WireModel>> {
        let base = self
            .url
            .strip_suffix("/chat/completions")
            .unwrap_or(&self.url);
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
            #[serde(default)]
            metadata: Option<EntryMetadata>,
        }
        #[derive(serde::Deserialize)]
        struct EntryMetadata {
            #[serde(default)]
            context_length: Option<u64>,
        }

        let body: ModelsResponse = response.json().await?;
        Ok(body
            .data
            .into_iter()
            .map(|m| crate::ai_provider::WireModel {
                id: m.id,
                display_name: None,
                context_window: m
                    .metadata
                    .and_then(|md| md.context_length)
                    .and_then(|n| u32::try_from(n).ok()),
                max_tokens: None,
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn applies_configured_headers() {
        let provider = OpenAIProvider::with_options(
            "demo",
            "https://example.com/v1",
            "key".into(),
            &HashMap::from([("X-Custom".to_string(), "v1".to_string())]),
            None,
        )
        .expect("provider builds");
        assert_eq!(
            provider
                .headers
                .get("x-custom")
                .map(|v| v.to_str().unwrap()),
            Some("v1")
        );
    }

    #[test]
    fn quickstart_has_no_extra_headers() {
        let provider = OpenAIProvider::new("demo", "https://example.com/v1", "key".into())
            .expect("provider builds");
        assert!(provider.headers.is_empty());
    }

    #[test]
    fn rejects_invalid_header_name() {
        let err = OpenAIProvider::with_options(
            "demo",
            "https://example.com/v1",
            "key".into(),
            &HashMap::from([("Bad Header".to_string(), "v".to_string())]),
            None,
        )
        .unwrap_err();
        assert!(matches!(err, AiError::ConfigError(_)));
        assert!(err.to_string().contains("Bad Header"));
    }

    #[test]
    fn rejects_zero_timeout() {
        let err = OpenAIProvider::with_options(
            "demo",
            "https://example.com/v1",
            "key".into(),
            &HashMap::new(),
            Some(0),
        )
        .unwrap_err();
        assert!(matches!(err, AiError::ConfigError(_)));
        assert!(err.to_string().contains("timeout_ms"));
    }
}
