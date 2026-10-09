use std::path::PathBuf;
use std::sync::Arc;

use storage::{DataDirectoryGuard, FileSessionStore, SessionStore};

use crate::{
    config::RuntimeConfig, error::Result, event::RuntimeEvent, event_bus::EventBus,
    runtime::Runtime, session_manager::SessionManager,
};

pub struct RuntimeBuilder {
    config: Option<RuntimeConfig>,
    state: Option<SessionManager>,
    event_bus: Option<EventBus<RuntimeEvent>>,
    storage_dir: Option<PathBuf>,
    catalog_path: Option<PathBuf>,
    social: Option<Arc<crate::feature::SocialResources>>,
}

impl RuntimeBuilder {
    pub fn new() -> Self {
        Self {
            config: None,
            state: None,
            event_bus: None,
            storage_dir: None,
            catalog_path: None,
            social: None,
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

    pub fn social_resources(mut self, resources: crate::feature::SocialResources) -> Self {
        self.social = Some(Arc::new(resources));
        self
    }

    pub fn build(self) -> Result<Runtime> {
        let config = self.config.unwrap_or_default();

        // AiClient::from_config builds and validates the model router first:
        // configuration errors (unknown default_model, empty reasoning levels,
        // no models) fail here, at startup, naming the offending provider.
        let catalog_path = self
            .catalog_path
            .or_else(|| self.storage_dir.as_deref().map(crate::catalog::cache_path));
        let catalog = crate::catalog::CatalogState::load(catalog_path.as_deref());
        let llm = Arc::new(catalog.client(&config)?);

        let mut directory_guard = None;
        let state = match (self.state, self.storage_dir) {
            (Some(_), Some(_)) => {
                return Err(crate::error::RuntimeError::ConfigError(
                    "cannot combine RuntimeBuilder::state with storage_dir".into(),
                ));
            }
            (Some(state), None) => state,
            (None, Some(dir)) => {
                let guard = DataDirectoryGuard::acquire(dir)?;
                let store: Arc<dyn SessionStore> =
                    Arc::new(FileSessionStore::from_guard(guard.clone()));
                directory_guard = Some(guard);
                SessionManager::from_store(store)?
            }
            (None, None) => SessionManager::new(),
        };
        let event_bus = self.event_bus.unwrap_or_default();

        let mut runtime = Runtime::new(state, event_bus, llm);
        runtime.catalog_path = catalog_path;
        runtime.catalog_state = Arc::new(tokio::sync::RwLock::new(catalog));
        runtime.directory_guard = directory_guard;
        runtime.social = self.social;
        Ok(runtime)
    }
}

impl Default for RuntimeBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl Runtime {
    /// GUI hosts explicitly activate SQLite; CLI/TUI retain their legacy path.
    pub async fn from_config_social_persistent(
        path: impl AsRef<std::path::Path>,
        storage_dir: impl Into<PathBuf>,
    ) -> Result<Self> {
        let mut runtime = Self::from_config_persistent(path, storage_dir)?;
        let guard = runtime
            .directory_guard
            .as_ref()
            .expect("persistent runtime owns its directory guard")
            .clone();
        let clock: Arc<dyn social::Clock> = Arc::new(social::SystemClock);
        let store = storage::social::SqliteSocialStore::open(guard, clock.clone()).await?;
        runtime.social = Some(Arc::new(crate::feature::SocialResources {
            store: Arc::new(store),
            clock,
        }));
        Ok(runtime)
    }

    pub fn from_config(path: impl AsRef<std::path::Path>) -> Result<Self> {
        let config = crate::config::ConfigLoader::load(&path)?;
        let mut builder = RuntimeBuilder::new().config(config);
        builder.catalog_path = path.as_ref().parent().map(crate::catalog::cache_path);
        let mut runtime = builder.build()?;
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
