use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub enum LlmChunk {
    Delta { content: String },
    Done,
}