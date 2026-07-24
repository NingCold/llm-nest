use std::sync::Arc;

use crate::{
    config::RuntimeConfig,
    error::{Result, RuntimeError},
    event::RuntimeEvent,
    event_bus::EventBus,
    provider_manager::ProviderManager,
    runtime::Runtime,
    session_manager::SessionManager,
};

pub struct RuntimeBuilder {
    config: Option<RuntimeConfig>,
    state: Option<SessionManager>,
    event_bus: Option<EventBus<RuntimeEvent>>,
    provider_mgr: Option<ProviderManager>,
}

impl RuntimeBuilder {
    pub fn new() -> Self {
        Self {
            config: None,
            state: None,
            event_bus: None,
            provider_mgr: None,
        }
    }

    pub fn config(mut self, config: RuntimeConfig) -> Self {
        self.config = Some(config);
        self
    }

    pub fn state(mut self, state: SessionManager) -> Self {
        self.state = Some(state);
        self
    }

    pub fn event_bus(mut self, event_bus: EventBus<RuntimeEvent>) -> Self {
        self.event_bus = Some(event_bus);
        self
    }

    pub fn provider_mgr(mut self, provider_mgr: ProviderManager) -> Self {
        self.provider_mgr = Some(provider_mgr);
        self
    }

    pub fn build(self) -> Result<Runtime> {
        let provider_mgr = match self.provider_mgr {
            Some(mgr) => mgr,
            None => {
                let config = self
                    .config
                    .ok_or(RuntimeError::ConfigError("missing config".into()))?;
                ProviderManager::new(config.providers())?
            }
        };

        let state = self.state.unwrap_or_default();
        let event_bus = self.event_bus.unwrap_or_default();

        Ok(Runtime::new(state, event_bus, Arc::new(provider_mgr)))
    }
}

impl Default for RuntimeBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl Runtime {
    pub fn from_config(path: impl AsRef<std::path::Path>) -> Result<Self> {
        let config = crate::config::ConfigLoader::load(path)?;
        RuntimeBuilder::new().config(config).build()
    }

    pub fn builder() -> RuntimeBuilder {
        RuntimeBuilder::new()
    }
}