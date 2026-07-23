use std::{fmt::Debug, pin::Pin};

use async_trait::async_trait;
use common::{ChatChunk, ChatRequest, ChatResponse};
use futures_util::Stream;

use crate::error::Result;

#[async_trait]
pub trait Provider: Send + Sync + Debug {
    async fn chat(
        &self,
        req: ChatRequest
    ) -> Result<ChatResponse>;

    async fn chat_stream(
        &self,
        req: ChatRequest,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<ChatChunk>> + Send>>>;
}