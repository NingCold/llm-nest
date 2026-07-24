use std::{fmt::Debug, pin::Pin};

use async_trait::async_trait;
use futures_util::Stream;
use llm::LlmChunk;

use crate::error::Result;
use crate::ProviderRequest;
use crate::ProviderResponse;

#[async_trait]
pub trait Provider: Send + Sync + Debug {
    async fn complete(&self, req: ProviderRequest) -> Result<ProviderResponse>;

    async fn complete_stream(
        &self,
        req: ProviderRequest,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<LlmChunk>> + Send>>>;
}