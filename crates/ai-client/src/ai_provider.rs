use async_trait::async_trait;

use crate::config::Protocol;
use crate::error::Result;
use crate::request::ChatRequest;
use crate::response::ProviderResponse;
use crate::stream::ChatStream;

/// A model discovered from the provider's own `GET /models` endpoint.
/// Capability fields are informational; `None` means the endpoint did not
/// publish them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WireModel {
    pub id: String,
    pub display_name: Option<String>,
    pub context_window: Option<u32>,
    pub max_tokens: Option<u32>,
}

/// Protocol backend. Non OpenAI-compatible platforms implement this trait to integrate.
///
/// NOTE: methods are declared with `#[async_trait]`. Future optimization:
/// replace with hand-written `BoxFuture` to avoid the per-call box allocation.
#[async_trait]
pub trait AiProvider: Send + Sync {
    fn id(&self) -> String;

    fn supported_protocols(&self) -> &[Protocol];

    async fn complete(&self, req: ChatRequest) -> Result<ProviderResponse>;

    async fn complete_stream(&self, req: ChatRequest) -> Result<ChatStream>;

    /// Fetch the model list from the provider's own `GET /models` endpoint.
    /// Used by the catalog refresh path; the default returns an empty list
    /// for backends that do not expose one.
    async fn list_models(&self) -> Result<Vec<WireModel>> {
        Ok(Vec::new())
    }
}
