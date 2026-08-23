//! Provider-neutral reasoning-effort vocabulary.
//!
//! Model routing works on neutral levels (mirroring Pi's thinking levels in
//! reduced form); each protocol maps them to its own wire spelling at request
//! conversion time. `Off` is an explicit request to disable reasoning on a
//! model that normally reasons; `None` in [`ModelSelection`] leaves the
//! provider default untouched.

use serde::{Deserialize, Serialize};

/// Provider-neutral reasoning effort levels.
///
/// Mirrors the Pi/DSH vocabulary (`off`/`low`/`high`/`max`) with `medium`
/// kept for OpenAI-family providers that accept it. `Max` is the top
/// strength tier (ECNU `ecnu-max`, newer OpenAI models).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReasoningEffort {
    /// Explicitly disable reasoning (mapped only when the model's format can express it).
    Off,
    Low,
    Medium,
    High,
    Max,
}

impl ReasoningEffort {
    /// OpenAI `reasoning_effort` wire spelling for the non-`Off` levels.
    pub fn as_wire(&self) -> &'static str {
        match self {
            ReasoningEffort::Off => "off",
            ReasoningEffort::Low => "low",
            ReasoningEffort::Medium => "medium",
            ReasoningEffort::High => "high",
            ReasoningEffort::Max => "max",
        }
    }
}

/// Wire format a model's reasoning level is dispatched through.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ReasoningFormat {
    /// OpenAI `reasoning_effort` request parameter (chat) / `reasoning: { effort }` (responses).
    #[serde(rename = "openai-effort")]
    OpenAIEffort,
    /// DeepSeek `thinking: { type: "enabled" | "disabled" }` request parameter.
    #[serde(rename = "deepseek-thinking")]
    DeepSeekThinking,
    /// DeepSeek-style `thinking` switch plus `reasoning_effort` strength
    /// (ECNU `ecnu-max`): `thinking: {type: "enabled"}` + `reasoning_effort:
    /// "low"|"high"|"max"`. The effort field only takes effect while thinking
    /// is enabled, so `Off` emits `thinking: {type: "disabled"}` and no
    /// `reasoning_effort`.
    #[serde(rename = "deepseek-effort")]
    DeepSeekEffort,
    /// Anthropic `thinking: { type, budget_tokens }` block.
    #[serde(rename = "anthropic-thinking")]
    AnthropicThinking,
    /// Gemini `generationConfig.thinkingConfig.thinkingBudget` (0 = off).
    #[serde(rename = "gemini-thinking")]
    GeminiThinking,
}

/// Declared reasoning support of one model: which neutral levels it accepts
/// and how they reach the wire. `levels` must be non-empty (validated when the
/// model catalog is built).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReasoningCapability {
    pub levels: Vec<ReasoningEffort>,
    pub format: ReasoningFormat,
    /// Thinking budget in tokens for formats that carry one (Anthropic
    /// `thinking.budget_tokens`); defaults to [`DEFAULT_THINKING_BUDGET_TOKENS`]
    /// when absent.
    #[serde(default)]
    pub budget_tokens: Option<u32>,
}

/// Default Anthropic `thinking.budget_tokens` when a capability declares no
/// budget. Deliberately modest: an enormous budget wastes the model's thinking
/// on trivial turns, and the value is per-request overridable via
/// [`ReasoningCapability::budget_tokens`].
pub const DEFAULT_THINKING_BUDGET_TOKENS: u32 = 4096;
