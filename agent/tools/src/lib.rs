//! Tool execution layer.
//!
//! A tool is a named capability the model may call: the model sees its
//! [`common::ToolDefinition`] in the request's `tools` declaration, issues
//! `ToolCall`s during generation, and the [`ToolRegistry`] executes them,
//! feeding results back as `ToolResult` messages for the next model turn.

pub mod builtin;
mod registry;
mod tool;

pub use registry::ToolRegistry;
pub use tool::{Tool, ToolError};

pub mod process;
