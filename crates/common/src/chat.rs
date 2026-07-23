use serde::{Deserialize, Serialize};

use crate::{Usage, config::{ModelId, provider::ProviderId}, message::Message};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatOptions {
    pub provider: ProviderId,
    pub model: ModelId,
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
    pub top_p: Option<f32>,
    pub stream: bool,
}

impl Default for ChatOptions {
    fn default() -> Self {
        Self {
            provider: ProviderId::new("chatecnu"),
            model: ModelId::new("ecnu-max"),
            temperature: None,
            max_tokens: None,
            top_p: None,
            stream: false,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ChatRequest {
    pub messages: Vec<Message>,
    pub options: ChatOptions,
}

impl ChatRequest {
    pub fn new(
        messages: Vec<Message>,
        options: ChatOptions,
    ) -> Self {
        Self {
            messages,
            options,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct ChatResponse {
    pub message: Message,
    pub usage: Option<Usage>,
}

#[derive(Debug, Clone, Deserialize)]
pub enum ChatChunk {
    Delta {
        content: String,
    },
    Done,
}