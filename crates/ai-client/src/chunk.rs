use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub enum ChatChunk {
    /// A fragment of the final answer.
    Delta {
        content: String,
    },
    /// A fragment of the model's thinking chain (e.g. OpenAI-compatible
    /// `delta.reasoning_content`); rendered distinctly by frontends and never
    /// stored into message history.
    ReasoningDelta {
        content: String,
    },
    Done,
}

#[deprecated(note = "use ChatChunk")]
pub type LlmChunk = ChatChunk;
