use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use common::ToolDefinition;
use futures_util::FutureExt;
use serde_json::Value;

use crate::tool::{Tool, ToolError};

/// Registry of tools available to the chat agent loop. Thread-safe with
/// interior mutability (share the registry, register tools anytime).
#[derive(Clone, Default)]
pub struct ToolRegistry {
    tools: Arc<RwLock<HashMap<&'static str, Arc<dyn Tool>>>>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Registry with the built-in tools (`echo`, `add`).
    pub fn with_builtins() -> Self {
        let reg = Self::new();
        reg.register_trusted_in_process(Arc::new(crate::process::ProcessTool::builtin(Arc::new(
            crate::builtin::Echo,
        ))));
        reg.register_trusted_in_process(Arc::new(crate::process::ProcessTool::builtin(Arc::new(
            crate::builtin::Add,
        ))));
        reg
    }

    /// Explicitly trust an in-process implementation (not a security sandbox).
    /// Model-selected tools cannot call this host-only API. Defaults use ProcessTool.
    /// Last registration wins on name collision.
    pub fn register_trusted_in_process(&self, tool: Arc<dyn Tool>) {
        self.tools
            .write()
            .expect("registry lock poisoned")
            .insert(tool.name(), tool);
    }

    /// Declarations for every registered tool, sorted by name for a
    /// deterministic request.
    pub fn definitions(&self) -> Vec<ToolDefinition> {
        let map = self.tools.read().expect("registry lock poisoned");
        let mut names: Vec<_> = map.keys().copied().collect();
        names.sort_unstable();
        names.into_iter().map(|n| map[n].definition()).collect()
    }

    pub fn get(&self, name: &str) -> Option<Arc<dyn Tool>> {
        self.tools
            .read()
            .expect("registry lock poisoned")
            .get(name)
            .cloned()
    }

    pub fn is_empty(&self) -> bool {
        self.tools
            .read()
            .expect("registry lock poisoned")
            .is_empty()
    }

    pub fn len(&self) -> usize {
        self.tools.read().expect("registry lock poisoned").len()
    }

    /// Run a tool by name. `None`/non-object arguments are rejected; the tool
    /// implementation validates the rest.
    pub async fn run(&self, name: &str, args: Value) -> Result<Value, ToolError> {
        self.run_with_timeout(name, args, std::time::Duration::from_secs(30))
            .await
    }

    /// Bound cooperative async execution and returned output (64 KiB).
    pub async fn run_with_timeout(
        &self,
        name: &str,
        args: Value,
        timeout: std::time::Duration,
    ) -> Result<Value, ToolError> {
        let tool = self
            .tools
            .read()
            .expect("registry lock poisoned")
            .get(name)
            .cloned()
            .ok_or_else(|| ToolError::NotFound(name.to_string()))?;
        if !args.is_object() {
            return Err(ToolError::InvalidArguments(format!(
                "{name} expects a JSON object of arguments"
            )));
        }
        let execution = std::panic::AssertUnwindSafe(async { tool.run(args).await }).catch_unwind();
        let value = tokio::time::timeout(timeout, execution)
            .await
            .map_err(|_| {
                ToolError::Execution(format!("{name} timed out after {} ms", timeout.as_millis()))
            })?
            .map_err(|_| ToolError::Execution(format!("{name} panicked")))??;
        if value.to_string().len() > 64 * 1024 {
            return Err(ToolError::Execution(format!(
                "{name} output exceeded 64 KiB"
            )));
        }
        Ok(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builtin::{Add, Echo};

    #[test]
    fn registry_roundtrips_definitions_sorted() {
        let reg = ToolRegistry::new();
        reg.register_trusted_in_process(Arc::new(Echo));
        reg.register_trusted_in_process(Arc::new(Add));
        let defs = reg.definitions();
        assert_eq!(defs.len(), 2);
        assert_eq!(defs[0].name, "add");
        assert_eq!(defs[1].name, "echo");
    }

    #[test]
    fn unknown_tool_errors() {
        let reg = ToolRegistry::new();
        assert!(reg.get("nope").is_none());
        assert!(reg.is_empty());
    }
    struct EdgeTool(u8, Arc<std::sync::atomic::AtomicBool>);
    struct Dropped(Arc<std::sync::atomic::AtomicBool>);
    impl Drop for Dropped {
        fn drop(&mut self) {
            self.0.store(true, std::sync::atomic::Ordering::SeqCst);
        }
    }
    impl Tool for EdgeTool {
        fn name(&self) -> &'static str {
            "edge"
        }
        fn description(&self) -> String {
            String::new()
        }
        fn parameters(&self) -> Value {
            serde_json::json!({"type":"object"})
        }
        fn run<'a>(
            &'a self,
            _: Value,
        ) -> futures_util::future::BoxFuture<'a, Result<Value, ToolError>> {
            Box::pin(async move {
                let _guard = Dropped(self.1.clone());
                match self.0 {
                    0 => std::future::pending().await,
                    1 => panic!("fixture panic"),
                    _ => Ok(Value::String("x".repeat(65537))),
                }
            })
        }
    }
    #[tokio::test]
    async fn timeout_drops_tool_future() {
        let dropped = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let reg = ToolRegistry::new();
        reg.register_trusted_in_process(Arc::new(EdgeTool(0, dropped.clone())));
        let result = reg
            .run_with_timeout(
                "edge",
                serde_json::json!({}),
                std::time::Duration::from_millis(10),
            )
            .await;
        assert!(result.unwrap_err().to_string().contains("timed out"));
        assert!(dropped.load(std::sync::atomic::Ordering::SeqCst));
    }
    #[tokio::test]
    async fn panic_and_excessive_output_become_errors() {
        for mode in [1, 2] {
            let reg = ToolRegistry::new();
            reg.register_trusted_in_process(Arc::new(EdgeTool(mode, Arc::default())));
            assert!(reg.run("edge", serde_json::json!({})).await.is_err());
        }
    }
}
