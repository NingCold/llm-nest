use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenerationParameters {
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
    pub top_p: Option<f32>,
    pub stream: bool,
}

impl From<llm::GenerationOptions> for GenerationParameters {
    fn from(opts: llm::GenerationOptions) -> Self {
        Self {
            temperature: opts.temperature,
            max_tokens: opts.max_tokens,
            top_p: opts.top_p,
            stream: opts.stream,
        }
    }
}