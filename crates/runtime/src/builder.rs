use provider::manager::ProviderManager;

use crate::{
    config::RuntimeConfig, error::{Result, RuntimeError}, event::RuntimeEvent, event_bus::EventBus, runtime::Runtime, state::RuntimeState
};

#[derive(Debug, Clone)]
pub struct RuntimeBuilder {
    config: Option<RuntimeConfig>,
    state: Option<RuntimeState>,
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

    pub fn config(
        mut self,
        config: RuntimeConfig,
    ) -> Self {
        self.config = Some(config);
        self
    }

    pub fn state(
        mut self,
        state: RuntimeState,
    ) -> Self {
        self.state = Some(state);
        self
    }

    pub fn event_bus(
        mut self,
        event_bus: EventBus<RuntimeEvent>,
    ) -> Self {
        self.event_bus = Some(event_bus);
        self
    }

    pub fn provider_mgr(
        mut self,
        provider_mgr: ProviderManager,
    ) -> Self {
        self.provider_mgr = Some(provider_mgr);
        self
    }

    pub fn build(
        self,
    ) -> Result<Runtime> {
        let provider_mgr = match self.provider_mgr {
            Some(mgr) => mgr,
            None => {
                let config = self.config
                    .ok_or(RuntimeError::ConfigError(
                        "missing config".into()
                    ))?;
                ProviderManager::new(
                    config.providers()
                )?
            }
        };

        let state = self.state.unwrap_or_default();
        let event_bus = self.event_bus.unwrap_or_default();

        Ok(Runtime::new(state, event_bus, provider_mgr))
    }
}