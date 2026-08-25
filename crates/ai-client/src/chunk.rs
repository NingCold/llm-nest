use common::Usage;

#[derive(Debug, Clone)]
pub enum ChatChunk {
    /// A fragment of the final answer.
    Delta { content: String },
    /// A fragment of the model's thinking chain (e.g. OpenAI-compatible
    /// `delta.reasoning_content`); rendered distinctly by frontends and never
    /// stored into message history.
    ReasoningDelta { content: String },
    /// The model requested a tool call. Streams assemble the (possibly
    /// fragmented) call before yielding this chunk.
    ToolCall {
        id: String,
        name: String,
        /// Raw JSON arguments as a string (may be partial only if the stream
        /// ended mid-call).
        arguments: String,
        /// Gemini thought signature (`thoughtSignature`): must be echoed back
        /// on the assistant functionCall part in the next request. `None` for
        /// protocols without the mechanism.
        thought_signature: Option<String>,
    },
    /// Stream end. Carries the turn's token usage when the protocol reports
    /// it on a terminal event (OpenAI final chunk / `response.completed`,
    /// Anthropic `message_delta`, Gemini `usageMetadata`).
    Done { usage: Option<Usage> },
}

#[deprecated(note = "use ChatChunk")]
pub type LlmChunk = ChatChunk;
