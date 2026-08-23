use common::{Message, Usage};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize)]
pub struct Request {
    pub model: String,
    pub messages: Vec<Message>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f32>,
    #[serde(default)]
    pub stream: bool,
    /// OpenAI `reasoning_effort` parameter (`low`/`medium`/`high`/`max`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<String>,
    /// DeepSeek `thinking` parameter (`{ "type": "enabled" | "disabled" }`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking: Option<ThinkingParam>,
}

/// DeepSeek `thinking` request parameter.
#[derive(Debug, Clone, Serialize)]
pub struct ThinkingParam {
    #[serde(rename = "type")]
    pub typ: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Response {
    pub id: String,
    pub object: Option<String>,
    pub created: Option<u64>,
    pub choices: Vec<Choice>,
    pub usage: Option<Usage>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Choice {
    pub index: u32,
    /// Raw message JSON: `content` may be a string or an array of blocks
    /// (deserialized via `common::Message`); thinking chains ride along as
    /// `reasoning_content` and are extracted separately.
    pub message: serde_json::Value,
    pub finish_reason: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct StreamResponse {
    pub id: String,
    pub object: Option<String>,
    pub created: Option<u64>,
    pub choices: Vec<StreamChoice>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct StreamChoice {
    pub index: u32,
    pub delta: Delta,
    pub finish_reason: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Delta {
    pub role: Option<String>,
    pub content: Option<String>,
    /// Thinking chain fragment (DeepSeek / ecnu-max / kimi / glm style);
    /// absent for plain chat completions.
    #[serde(default)]
    pub reasoning_content: Option<String>,
}
