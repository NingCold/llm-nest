use std::pin::Pin;

use async_trait::async_trait;
use common::{ChatChunk, ChatRequest, ChatResponse, config::ProviderConfig};
use futures_util::Stream;

use crate::{
    error::{
        ProviderError,
        Result
    },
    protocols::openai::{
        chat::Response,
        convert, error::ErrorResponse,
        sse::OpenAIStream
    },
    traits::Provider
};

#[derive(Debug, Clone)]
pub struct OpenAIProvider {
    client: reqwest::Client,
    config: ProviderConfig,
    url: String,
}

impl OpenAIProvider {
    pub fn new(config: ProviderConfig) -> Result<Self> {
        let client = reqwest::Client::builder()
            .build()?;

        let url = format!("{}/chat/completions", config.base_url);
        Ok(Self {
            client,
            config,
            url,
        })
    }

    async fn send_request(
        &self,
        req: ChatRequest,
    ) -> Result<ChatResponse> {

        let request =
            convert::to_real_request(
                req,
            );
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
            }
        }

        let chat_response: Response = response.json().await?;
        chat_response.try_into()
    }

    async fn send_request_stream(
        &self,
        req: ChatRequest,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<ChatChunk>> + Send>>> {
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
                Ok(err) => Err(
                    ProviderError::ApiError {
                        status,
                        message: err.error.message,
                        kind: Some(err.error.kind),
                        code: err.error.code,
                    }
                ),

                Err(_) => Err(
                    ProviderError::ApiError {
                        status,
                        message: body,
                        kind: None,
                        code: None,
                    }
                )
            }
        }

        Ok(Box::pin(
            OpenAIStream::new(response)
        ))
    }
}

#[async_trait]
impl Provider for OpenAIProvider {
    async fn chat(
        &self,
        req: ChatRequest,
    ) -> Result<ChatResponse> {
        self.send_request(req).await
    }

    async fn chat_stream(
        &self,
        req: ChatRequest,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<ChatChunk>> + Send>>> {
        self.send_request_stream(req).await
    }
}