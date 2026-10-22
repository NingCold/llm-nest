use common::{GenerationOptions, Message};
use serde::{Deserialize, Serialize};

use crate::reasoning::ReasoningEffort;
use crate::router::ResolvedSelection;

/// Complete provider, model, and optional reasoning effort selected for a call.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelSelection {
    /// Registered provider route.
    pub provider: String,
    /// Provider-owned model id (wire name or configuration key).
    pub model: String,
    /// Provider-neutral reasoning effort; absent leaves provider default behavior.
    /// `alias` 让前端 camelCase 字段（reasoningEffort）也能反序列化；
    /// 序列化仍输出 reasoning_effort（存储/配置格式不变）。
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        alias = "reasoningEffort"
    )]
    pub reasoning_effort: Option<ReasoningEffort>,
}

#[derive(Debug, Clone)]
pub struct ChatRequest {
    pub selection: ModelSelection,
    pub messages: Vec<Message>,
    pub options: GenerationOptions,
    /// Tools the model may call; converted by each protocol to its own wire
    /// declaration. Empty = no tool calling.
    pub tools: Vec<common::ToolDefinition>,
    /// Routing result filled by the client before dispatch; `None` on the
    /// legacy (manually registered provider) path, which skips reasoning wire
    /// mapping. Not part of serialization.
    pub resolved: Option<ResolvedSelection>,
}

impl ChatRequest {
    /// Effective API model name; configuration aliases never reach the wire.
    pub fn wire_model(&self) -> &str {
        self.resolved
            .as_ref()
            .map(|r| r.spec.wire.as_str())
            .unwrap_or(&self.selection.model)
    }
}
