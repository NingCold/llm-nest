use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use common::ToolDefinition;
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
        reg.register(Arc::new(crate::builtin::Echo));
        reg.register(Arc::new(crate::builtin::Add));
        reg
    }

    /// Register a tool (last registration wins on name collision).
    pub fn register(&self, tool: Arc<dyn Tool>) {
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
        tool.run(args).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builtin::{Add, Echo};

    #[test]
    fn registry_roundtrips_definitions_sorted() {
        let reg = ToolRegistry::new();
        reg.register(Arc::new(Echo));
        reg.register(Arc::new(Add));
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
}
