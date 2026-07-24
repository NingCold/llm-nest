use std::pin::Pin;
use std::sync::Arc;

use async_trait::async_trait;
use futures_util::Stream;
use futures_util::StreamExt;
use llm::{CompletionRequest, LlmChunk, LlmError};

use provider::ProviderRequest;

use crate::model_router::ModelRouter;

pub struct RuntimeLlmClient {
    router: Arc<ModelRouter>,
}

impl RuntimeLlmClient {
    pub fn new(router: Arc<ModelRouter>) -> Self {
        Self { router }
    }
}

#[async_trait]
impl llm::LlmClient for RuntimeLlmClient {
    async fn complete_stream(
        &self,
        req: CompletionRequest,
    ) -> llm::Result<Pin<Box<dyn Stream<Item = llm::Result<LlmChunk>> + Send>>> {
        let provider = self.router.route(&req.model).await
            .map_err(|e| LlmError::ProviderError(e.to_string()))?;

        let provider_req = ProviderRequest {
            model: req.model.model,
            messages: req.messages,
            parameters: req.options.into(),
        };

        let stream = provider.complete_stream(provider_req).await
            .map_err(|e| LlmError::ProviderError(e.to_string()))?;

        Ok(Box::pin(stream.map(|r| r.map_err(|e| LlmError::ProviderError(e.to_string())))))
    }
}