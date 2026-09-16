//! Built-in tools registered by default in [`super::ToolRegistry::with_builtins`].

use serde_json::{Value, json};

use crate::tool::{Tool, ToolError};

/// Echo the given text back. Useful as the simplest end-to-end tool.
pub struct Echo;

impl Tool for Echo {
    fn name(&self) -> &'static str {
        "echo"
    }

    fn description(&self) -> String {
        "Echoes the given text back verbatim. Use to verify tool calling works.".to_string()
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "text": { "type": "string", "description": "The text to echo" }
            },
            "required": ["text"]
        })
    }

    fn run<'a>(
        &'a self,
        args: Value,
    ) -> futures_util::future::BoxFuture<'a, Result<Value, ToolError>> {
        Box::pin(async move { self.run_sync(args) })
    }

    fn run_sync(&self, args: Value) -> Result<Value, ToolError> {
        let text = args
            .get("text")
            .and_then(Value::as_str)
            .ok_or_else(|| ToolError::InvalidArguments("missing string field 'text'".into()))?;
        Ok(json!({ "echo": text }))
    }
}

/// Add two numbers. A tiny real computation so the agent loop's result
/// feedback is observable.
pub struct Add;

impl Tool for Add {
    fn name(&self) -> &'static str {
        "add"
    }

    fn description(&self) -> String {
        "Adds two numbers and returns the sum.".to_string()
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "a": { "type": "number", "description": "First addend" },
                "b": { "type": "number", "description": "Second addend" }
            },
            "required": ["a", "b"]
        })
    }

    fn run<'a>(
        &'a self,
        args: Value,
    ) -> futures_util::future::BoxFuture<'a, Result<Value, ToolError>> {
        Box::pin(async move { self.run_sync(args) })
    }

    fn run_sync(&self, args: Value) -> Result<Value, ToolError> {
        let a = args
            .get("a")
            .and_then(Value::as_f64)
            .ok_or_else(|| ToolError::InvalidArguments("missing number field 'a'".into()))?;
        let b = args
            .get("b")
            .and_then(Value::as_f64)
            .ok_or_else(|| ToolError::InvalidArguments("missing number field 'b'".into()))?;
        Ok(json!({ "sum": a + b }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn echo_returns_text() {
        let out = Echo.run_sync(json!({ "text": "hi" })).unwrap();
        assert_eq!(out["echo"], "hi");
    }

    #[test]
    fn add_sums_numbers() {
        let out = Add.run_sync(json!({ "a": 2, "b": 3 })).unwrap();
        assert_eq!(out["sum"], 5.0);
    }

    #[test]
    fn add_rejects_bad_args() {
        assert!(Add.run_sync(json!({ "a": "x" })).is_err());
    }
}
