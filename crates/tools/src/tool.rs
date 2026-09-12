use std::fmt;

use common::ToolDefinition;
use futures_util::future::BoxFuture;
use serde_json::Value;

/// A tool the model can call. Implementations are registered in a
/// [`super::ToolRegistry`]; the model only ever sees the declaration
/// ([`Tool::definition`]) and the JSON arguments of its calls.
pub trait Tool: Send + Sync {
    fn name(&self) -> &'static str;

    fn description(&self) -> String;

    /// JSON Schema describing the `args` object passed to [`Tool::run`].
    fn parameters(&self) -> Value;

    /// The declaration sent to providers.
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: self.description(),
            parameters: self.parameters(),
        }
    }

    /// Execute without blocking the runtime. Implementations must yield and be
    /// safe to drop on timeout/cancellation. Side effects are not rolled back.
    /// Long-running blocking work belongs in a separately managed process.
    fn run<'a>(&'a self, args: Value) -> BoxFuture<'a, Result<Value, ToolError>>;

    /// Helper for small bounded computations; never invoked implicitly.
    fn run_sync(&self, _args: Value) -> Result<Value, ToolError> {
        Err(ToolError::NotImplemented(self.name().to_string()))
    }
}

/// Why a tool call failed. The message text is sent back to the model as an
/// error-flagged `ToolResult` (so it can recover), and surfaces to frontends
/// via the `ToolResult.is_error` flag.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolError {
    /// No tool with this name is registered.
    NotFound(String),
    /// Arguments failed JSON-Schema validation.
    InvalidArguments(String),
    /// The tool implementation failed.
    Execution(String),
    /// The tool has no implementation for the synchronous fallback.
    NotImplemented(String),
}

impl fmt::Display for ToolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ToolError::NotFound(name) => write!(f, "tool not found: {name}"),
            ToolError::InvalidArguments(msg) => write!(f, "invalid arguments: {msg}"),
            ToolError::Execution(msg) => write!(f, "tool execution failed: {msg}"),
            ToolError::NotImplemented(name) => write!(f, "tool {name} has no run implementation"),
        }
    }
}

impl std::error::Error for ToolError {}
