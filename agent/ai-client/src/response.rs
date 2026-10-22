use common::{Message, Usage};

#[derive(Debug, Clone)]
pub struct ProviderResponse {
    pub message: Message,
    /// Thinking chain of the turn, when the protocol returns one
    /// (OpenAI-compatible `reasoning_content`); not part of `message`.
    pub reasoning: Option<String>,
    pub usage: Option<Usage>,
}
