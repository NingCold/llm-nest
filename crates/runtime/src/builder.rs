use std::path::PathBuf;
use std::sync::Arc;

use ai_client::AiClient;
use storage::{FileSessionStore, SessionStore};

use crate::{
    config::RuntimeConfig, error::Result, event::RuntimeEvent, event_bus::EventBus,
    runtime::Runtime, session_manager::SessionManager,
};

pub struct RuntimeBuilder {
    config: Option<RuntimeConfig>,
    state: Option<SessionManager>,
    event_bus: Option<EventBus<RuntimeEvent>>,
    storage_dir: Option<PathBuf>,
}

impl RuntimeBuilder {
    pub fn new() -> Self {
        Self {
            config: None,
            state: None,
            event_bus: None,
            storage_dir: None,
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

    /// Persist sessions to `dir` (one JSON file per session): the directory
    /// is loaded at build time and every session mutation is written through.
    /// Mutually exclusive with [`RuntimeBuilder::state`].
    pub fn storage_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.storage_dir = Some(dir.into());
        self
    }

    pub fn build(self) -> Result<Runtime> {
        let config = self.config.unwrap_or_default();

        // AiClient::from_config builds and validates the model router first:
        // configuration errors (unknown default_model, empty reasoning levels,
        // no models) fail here, at startup, naming the offending provider.
        let llm = Arc::new(AiClient::from_config(config.providers())?);

        let state = match (self.state, self.storage_dir) {
            (Some(_), Some(_)) => {
                return Err(crate::error::RuntimeError::ConfigError(
                    "cannot combine RuntimeBuilder::state with storage_dir".into(),
                ));
            }
            (Some(state), None) => state,
            (None, Some(dir)) => {
                let store: Arc<dyn SessionStore> = Arc::new(FileSessionStore::new(dir)?);
                SessionManager::from_store(store)?
            }
            (None, None) => SessionManager::new(),
        };
        let event_bus = self.event_bus.unwrap_or_default();

        Ok(Runtime::new(state, event_bus, llm))
    }
}

impl Default for RuntimeBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl Runtime {
    pub fn from_config(path: impl AsRef<std::path::Path>) -> Result<Self> {
        let config = crate::config::ConfigLoader::load(&path)?;
        let mut runtime = RuntimeBuilder::new().config(config).build()?;
        runtime.config_path = Some(path.as_ref().to_path_buf());
        Ok(runtime)
    }

    /// Like [`Runtime::from_config`], with sessions persisted to `storage_dir`:
    /// existing sessions are loaded at startup and every session mutation is
    /// written through to the store.
    pub fn from_config_persistent(
        path: impl AsRef<std::path::Path>,
        storage_dir: impl Into<PathBuf>,
    ) -> Result<Self> {
        let config = crate::config::ConfigLoader::load(&path)?;
        let mut runtime = RuntimeBuilder::new()
            .config(config)
            .storage_dir(storage_dir)
            .build()?;
        runtime.config_path = Some(path.as_ref().to_path_buf());
        Ok(runtime)
    }

    pub fn builder() -> RuntimeBuilder {
        RuntimeBuilder::new()
    }
}
