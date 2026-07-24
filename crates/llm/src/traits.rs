use std::pin::Pin;

use async_trait::async_trait;
use futures_util::Stream;

use crate::{CompletionRequest, LlmChunk, error::Result};

#[async_trait]
pub trait LlmClient: Send + Sync {
    async fn complete_stream(
        &self,
        req: CompletionRequest,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<LlmChunk>> + Send>>>;
}