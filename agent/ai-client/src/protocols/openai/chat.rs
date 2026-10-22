use common::Usage;
use serde::{Deserialize, Serialize};

/// OpenAI wire usage: flat token counts plus cache-hit details nested under
/// `prompt_tokens_details` (absent on plain completions).
#[derive(Debug, Clone, Deserialize)]
pub struct RawUsage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
    #[serde(default)]
    pub prompt_tokens_details: Option<PromptTokensDetails>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PromptTokensDetails {
    #[serde(default)]
    pub cached_tokens: u32,
}

impl From<RawUsage> for Usage {
    fn from(u: RawUsage) -> Self {
        Usage {
            prompt_tokens: u.prompt_tokens,
            completion_tokens: u.completion_tokens,
            total_tokens: u.total_tokens,
            cached_tokens: u
                .prompt_tokens_details
                .map(|d| d.cached_tokens)
                .unwrap_or(0),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Request {
    pub model: String,
    /// Wire-form messages (role + content blocks via [`common::Message::to_wire_value`]).
    pub messages: Vec<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f32>,
    #[serde(default)]
    pub stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream_options: Option<serde_json::Value>,
    /// OpenAI `reasoning_effort` parameter (`low`/`medium`/`high`/`max`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<String>,
    /// DeepSeek `thinking` parameter (`{ "type": "enabled" | "disabled" }`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking: Option<ThinkingParam>,
    /// Function-calling declarations.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<ToolDecl>,
}

/// OpenAI function-calling declaration.
#[derive(Debug, Clone, Serialize)]
pub struct ToolDecl {
    #[serde(rename = "type")]
    pub typ: String,
    pub function: FunctionDecl,
}

#[derive(Debug, Clone, Serialize)]
pub struct FunctionDecl {
    pub name: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub description: String,
    #[serde(skip_serializing_if = "serde_json::Value::is_null")]
    pub parameters: serde_json::Value,
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
    pub usage: Option<RawUsage>,
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
    /// Present on the final chunk of streams that report usage.
    #[serde(default)]
    pub usage: Option<RawUsage>,
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
    /// Some OpenAI-compatible gateways (including ecnu-max) emit `reasoning`.
    /// Keep both fields: a serde alias would reject responses containing both.
    #[serde(default)]
    pub reasoning: Option<String>,
    /// Function-call fragments (streamed piecemeal; assembled by the stream
    /// parser keyed by `index`).
    #[serde(default)]
    pub tool_calls: Option<Vec<DeltaToolCall>>,
}

/// One piece of a streamed tool call: `id`/`name` arrive on the first
/// fragment of an index, `arguments` may arrive over several.
#[derive(Debug, Clone, Deserialize)]
pub struct DeltaToolCall {
    pub index: u32,
    pub id: Option<String>,
    #[serde(rename = "type")]
    pub typ: Option<String>,
    pub function: Option<DeltaFunction>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DeltaFunction {
    pub name: Option<String>,
    pub arguments: Option<String>,
}
