use std::fmt::{Display, Formatter, Result};

use serde::{Deserialize, Serialize};

use crate::reasoning::ReasoningCapability;

use super::Protocol;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ModelId(pub String);

impl ModelId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }
}

impl From<&str> for ModelId {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

impl Display for ModelId {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        self.0.fmt(f)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelConfig {
    /// The model name sent on the wire.
    pub model: String,
    /// Name shown by frontends; defaults to the wire name.
    pub display_name: Option<String>,
    /// Context capacity, informational (no implicit request clamping).
    #[serde(default)]
    pub context_window: Option<u32>,
    /// Output capability, informational (no implicit request clamping).
    #[serde(default)]
    pub max_tokens: Option<u32>,
    /// Declared reasoning support; absent means the model takes no effort field.
    #[serde(default)]
    pub reasoning: Option<ReasoningCapability>,
    /// Wire protocol override: this model is dispatched through `protocol`
    /// instead of the provider's default. Lets one provider route (shared
    /// key/base_url/headers) serve models that speak different APIs, e.g.
    /// chat completions and Responses on the same gateway.
    #[serde(default)]
    pub protocol: Option<Protocol>,
}
