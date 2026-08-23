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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<ReasoningEffort>,
}

#[derive(Debug, Clone)]
pub struct ChatRequest {
    pub selection: ModelSelection,
    pub messages: Vec<Message>,
    pub options: GenerationOptions,
    /// Routing result filled by the client before dispatch; `None` on the
    /// legacy (manually registered provider) path, which skips reasoning wire
    /// mapping. Not part of serialization.
    pub resolved: Option<ResolvedSelection>,
}
