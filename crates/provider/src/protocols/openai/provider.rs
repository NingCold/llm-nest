use std::pin::Pin;

use async_trait::async_trait;
use futures_util::Stream;
use llm::LlmChunk;

use crate::{
    error::{ProviderError, Result},
    factory::ResolvedProviderConfig,
    protocols::openai::{
        chat::Response, convert, error::ErrorResponse, sse::OpenAIStream,
    },
    traits::Provider,
    ProviderRequest, ProviderResponse,
};

#[derive(Debug, Clone)]
pub struct OpenAIProvider {
    client: reqwest::Client,
    config: ResolvedProviderConfig,
    url: String,
}

impl OpenAIProvider {
    pub fn new(config: ResolvedProviderConfig) -> Result<Self> {
        let client = reqwest::Client::builder().build()?;
        let url = format!("{}/chat/completions", config.base_url);
        Ok(Self { client, config, url })
    }

    async fn send_request(&self, req: ProviderRequest) -> Result<ProviderResponse> {
        let request = convert::to_real_request(req);
        let response = self
            .client
            .post(&self.url)
            .bearer_auth(&self.config.api_key)
            .json(&request)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await?;
            return match serde_json::from_str::<ErrorResponse>(&body) {
                Ok(err) => Err(ProviderError::ApiError {
                    status,
                    message: err.error.message,
                    kind: Some(err.error.kind),
                    code: err.error.code,
                }),
                Err(_) => Err(ProviderError::ApiError {
                    status,
                    message: body,
                    kind: None,
                    code: None,
                }),
            };
        }

        let chat_response: Response = response.json().await?;
        chat_response.try_into()
    }

    async fn send_request_stream(
        &self,
        req: ProviderRequest,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<LlmChunk>> + Send>>> {
        let mut request = convert::to_real_request(req);
        request.stream = true;
        let response = self
            .client
            .post(&self.url)
            .bearer_auth(&self.config.api_key)
            .json(&request)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await?;
            return match serde_json::from_str::<ErrorResponse>(&body) {
                Ok(err) => Err(ProviderError::ApiError {
                    status,
                    message: err.error.message,
                    kind: Some(err.error.kind),
                    code: err.error.code,
                }),
                Err(_) => Err(ProviderError::ApiError {
                    status,
                    message: body,
                    kind: None,
                    code: None,
                }),
            };
        }

        Ok(Box::pin(OpenAIStream::new(response)))
    }
}

#[async_trait]
impl Provider for OpenAIProvider {
    async fn complete(&self, req: ProviderRequest) -> Result<ProviderResponse> {
        self.send_request(req).await
    }

    async fn complete_stream(
        &self,
        req: ProviderRequest,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<LlmChunk>> + Send>>> {
        self.send_request_stream(req).await
    }
}