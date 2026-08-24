use serde::{Deserialize, Serialize};

/// A tool the model may call. Sent to providers as the tools declaration
/// (each protocol converts it to its own wire form), executed by
/// [`crates::tools::ToolRegistry`] when the model issues a call.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    /// JSON Schema (draft-07 subset) describing the arguments object.
    pub parameters: serde_json::Value,
}
