use std::collections::HashMap;

pub trait Plugin: Send + Sync {
    fn name(&self) -> &'static str;
    fn on_load(&self) -> anyhow::Result<()> {
        Ok(())
    }
    fn on_unload(&self) -> anyhow::Result<()> {
        Ok(())
    }
}

pub struct PluginManager {
    plugins: HashMap<String, Box<dyn Plugin>>,
}

impl PluginManager {
    pub fn new() -> Self {
        Self { plugins: HashMap::new() }
    }

    pub fn register(&mut self, plugin: Box<dyn Plugin>) -> anyhow::Result<()> {
        let name = plugin.name().to_string();
        plugin.on_load()?;
        self.plugins.insert(name, plugin);
        Ok(())
    }

    pub fn get(&self, name: &str) -> Option<&dyn Plugin> {
        self.plugins.get(name).map(|p| p.as_ref())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestPlugin;

    impl Plugin for TestPlugin {
        fn name(&self) -> &'static str {
            "test"
        }
    }

    #[test]
    fn test_register_and_get() {
        let mut mgr = PluginManager::new();
        mgr.register(Box::new(TestPlugin)).unwrap();
        assert!(mgr.get("test").is_some());
        assert!(mgr.get("unknown").is_none());
    }
}