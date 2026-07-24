use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenerationOptions {
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
    pub top_p: Option<f32>,
    pub stream: bool,
}

impl Default for GenerationOptions {
    fn default() -> Self {
        Self {
            temperature: None,
            max_tokens: None,
            top_p: None,
            stream: false,
        }
    }
}