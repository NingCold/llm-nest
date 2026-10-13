use std::path::PathBuf;
use std::sync::Arc;

use ai_client::AiClient;
use ai_client::ModelInfo;
use ai_client::ModelSelection;
use ai_client::ResolvedSelection;
use tokio::sync::RwLock;

use crate::error::{Result, RuntimeError};
use crate::feature::{Feature, FeatureContext, FeatureRegistry};

#[derive(Clone)]
pub struct Runtime {
    pub(crate) config_updates: Arc<tokio::sync::Mutex<()>>,
    pub(crate) catalog_sync: Arc<tokio::sync::Mutex<()>>,
    pub(crate) catalog_state: Arc<RwLock<crate::catalog::CatalogState>>,
    pub(crate) catalog_path: Option<PathBuf>,
    pub(crate) llm: Arc<AiClient>,
    features: Arc<RwLock<FeatureRegistry>>,
    pub(crate) config_path: Option<PathBuf>,
    pub(crate) social: Arc<crate::feature::SocialResources>,
}

impl Runtime {
    /// Construct the runtime with explicit social resources (also used by deterministic tests).
    pub fn with_resources(
        config: &crate::config::RuntimeConfig,
        resources: crate::feature::SocialResources,
    ) -> Result<Self> {
        Ok(Self {
            config_updates: Arc::new(tokio::sync::Mutex::new(())),
            catalog_sync: Arc::new(tokio::sync::Mutex::new(())),
            catalog_state: Arc::new(RwLock::new(crate::catalog::CatalogState::default())),
            catalog_path: None,
            llm: Arc::new(AiClient::from_config(config.providers())?),
            features: Arc::new(RwLock::new(FeatureRegistry::new())),
            config_path: None,
            social: Arc::new(resources),
        })
    }

    /// Open the social database. Old JSON is only read by explicit import.
    pub async fn open(
        config_path: impl AsRef<std::path::Path>,
        data_dir: impl Into<PathBuf>,
    ) -> Result<Self> {
        let config = crate::config::ConfigLoader::load(&config_path)?;
        let guard = storage::DataDirectoryGuard::acquire(data_dir)?;
        let catalog_path = crate::catalog::cache_path(guard.dir());
        let catalog = crate::catalog::CatalogState::load(Some(&catalog_path));
        let llm = Arc::new(catalog.client(&config)?);
        let clock: Arc<dyn social::Clock> = Arc::new(social::SystemClock);
        let store = storage::social::SqliteSocialStore::open(guard, clock.clone()).await?;
        Ok(Self {
            config_updates: Arc::new(tokio::sync::Mutex::new(())),
            catalog_sync: Arc::new(tokio::sync::Mutex::new(())),
            catalog_state: Arc::new(RwLock::new(catalog)),
            catalog_path: Some(catalog_path),
            llm,
            features: Arc::new(RwLock::new(FeatureRegistry::new())),
            config_path: Some(config_path.as_ref().to_path_buf()),
            social: Arc::new(crate::feature::SocialResources {
                store: Arc::new(store),
                clock,
            }),
        })
    }

    pub fn llm_client(&self) -> Arc<AiClient> {
        self.llm.clone()
    }

    /// The default `ModelSelection` across providers, resolved through the
    /// model router (config `default_model` > builtin default > first model).
    pub async fn default_model(&self) -> Option<ModelSelection> {
        self.llm.default_selection().await
    }

    /// Every effective model across providers, with capabilities and display
    /// names, in deterministic order.
    pub async fn list_models(&self) -> Vec<ModelInfo> {
        self.llm.list_models().await
    }

    /// Re-apply the configuration document at `path` to a running runtime.
    ///
    /// The whole next configuration is loaded and validated before anything
    /// swaps; a refused document (parse error, unknown `default_model`, empty
    /// reasoning levels, unset key, invalid header, ...) returns the error and
    /// the current configuration keeps serving. In-flight requests are
    /// unaffected; the new configuration applies to the next request.
    pub async fn reload_config(&self, path: impl AsRef<std::path::Path>) -> Result<()> {
        let _update = self.config_updates.lock().await;
        let config = crate::config::ConfigLoader::load(path)?;
        self.llm.install(self.catalog_client(&config).await?).await;
        Ok(())
    }

    /// Fetch `provider`'s model list from its own `GET /models` endpoint,
    /// merge newly discovered models into the catalog, and **persist them
    /// back into the config document** (in-place TOML edit; existing entries
    /// and comments are untouched). Returns the newly added models (empty
    /// when nothing was new).
    ///
    /// If `provider` is not yet in the in-memory snapshot (e.g. it was just
    /// added to the config file and the watcher has not reloaded yet), the
    /// config document is reloaded first and the refresh retried once.
    pub async fn refresh_models(&self, provider: &str) -> Result<Vec<ai_client::WireModel>> {
        let _update = self.config_updates.lock().await;
        let candidate = if let Some(path) = &self.config_path {
            let config = crate::config::ConfigLoader::load(path)?;
            self.catalog_client(&config).await?
        } else {
            self.llm.snapshot().await
        };
        let added = candidate.refresh_models(provider).await?;
        if !added.is_empty() {
            if let Some(path) = &self.config_path {
                crate::config::persist::persist_new_models(path, provider, &added)?;
            }
            self.llm.install(candidate).await;
        }
        Ok(added)
    }

    /// Persist and hot-apply a provider written from the web settings
    /// (create or update). Only the form's fields (protocol / base_url /
    /// api_key / models) change; other document fields are preserved. Fails
    /// without touching anything when the runtime has no config document
    /// (in-memory runtime), and the whole next config is validated by
    /// `reload_config` before it swaps — a bad provider keeps the old one.
    pub async fn upsert_provider(
        &self,
        draft: &crate::config::persist::ProviderDraft,
    ) -> Result<()> {
        let _update = self.config_updates.lock().await;
        let path = self.config_path.clone().ok_or_else(|| {
            RuntimeError::ConfigError("no config document to persist to (in-memory runtime)".into())
        })?;
        let text = std::fs::read_to_string(&path)?;
        let next = crate::config::persist::render_provider(&text, draft)?;
        let config: crate::config::runtime::RuntimeConfig = toml::from_str(&next)?;
        crate::config::ConfigLoader::validate(&config)?;
        let candidate = self.catalog_client(&config).await?;
        crate::config::persist::atomic_write(&path, &next)?;
        self.llm.install(candidate).await;
        Ok(())
    }

    /// Remove a provider from the config document and hot-apply the change.
    pub async fn remove_provider(&self, id: &str) -> Result<()> {
        let _update = self.config_updates.lock().await;
        let path = self.config_path.clone().ok_or_else(|| {
            RuntimeError::ConfigError("no config document to persist to (in-memory runtime)".into())
        })?;
        let text = std::fs::read_to_string(&path)?;
        let next = crate::config::persist::render_remove_provider(&text, id)?;
        let config: crate::config::runtime::RuntimeConfig = toml::from_str(&next)?;
        crate::config::ConfigLoader::validate(&config)?;
        let candidate = self.catalog_client(&config).await?;
        crate::config::persist::atomic_write(&path, &next)?;
        self.llm.install(candidate).await;
        Ok(())
    }

    /// Strictly validate a selection against the mounted model catalog before
    /// switching to it. Errors name the offending key and its candidates.
    pub async fn resolve_model(&self, selection: &ModelSelection) -> Result<ResolvedSelection> {
        match self.llm.resolve(selection).await? {
            Some(resolved) => Ok(resolved),
            None => Err(RuntimeError::ConfigError("no model catalog mounted".into())),
        }
    }

    pub fn context(&self) -> FeatureContext {
        FeatureContext {
            llm: self.llm.clone(),
            social: self.social.clone(),
        }
    }

    pub async fn register_feature(&self, feature: Arc<dyn Feature>) {
        let mut features = self.features.write().await;
        features.register(feature);
    }

    pub async fn initialize_features(&self) -> Result<()> {
        let ctx = self.context();
        let features = self.features.read().await;
        features.initialize_all(ctx).await
    }

    pub async fn shutdown_features(&self) -> Result<()> {
        let features = self.features.read().await;
        let features_result = features.shutdown_all().await;
        let storage_result = self
            .social
            .store
            .shutdown()
            .await
            .map_err(RuntimeError::from);
        features_result.and(storage_result)
    }

    pub async fn get_feature<T: Feature + 'static>(&self, id: &str) -> Option<Arc<T>> {
        self.features.read().await.get_by_id(id)
    }
}

impl std::fmt::Debug for Runtime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Runtime").finish()
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use ai_client::config::{ApiKey, ModelConfig, ModelId, Protocol, ProviderConfig, ProviderId};

    use super::*;
    use crate::config::RuntimeConfig;

    fn deepseek_config() -> RuntimeConfig {
        RuntimeConfig {
            providers: HashMap::from([(
                ProviderId::new("deepseek"),
                ProviderConfig {
                    protocol: Some(Protocol::OpenAIChat),
                    api_key: ApiKey::Direct("test-key".into()),
                    base_url: Some("https://api.deepseek.com".into()),
                    models: HashMap::from([
                        (
                            ModelId::new("chat"),
                            ModelConfig {
                                model: "deepseek-chat".into(),
                                display_name: None,
                                context_window: None,
                                max_tokens: None,
                                reasoning: None,
                                protocol: None,
                            },
                        ),
                        (
                            ModelId::new("reasoner"),
                            ModelConfig {
                                model: "deepseek-reasoner".into(),
                                display_name: None,
                                context_window: None,
                                max_tokens: None,
                                reasoning: None,
                                protocol: None,
                            },
                        ),
                    ]),
                    default_model: Some("chat".into()),
                    headers: HashMap::new(),
                    timeout_ms: None,
                },
            )]),
        }
    }

    #[tokio::test]
    async fn runtime_routes_default_model_and_catalog() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("llmn.toml");
        std::fs::write(&path, toml::to_string(&deepseek_config()).unwrap()).unwrap();
        let runtime = Runtime::open(&path, directory.path()).await.unwrap();

        // default model comes from the provider's default_model, not iteration order
        let default = runtime.default_model().await.expect("default model");
        assert_eq!(default.provider, "deepseek");
        assert_eq!(default.model, "deepseek-chat");

        let models = runtime.list_models().await;
        assert_eq!(models.len(), 2);
        assert!(models.iter().any(|m| m.spec.wire == "deepseek-chat"));

        // unknown model resolution fails with diagnostics naming it
        let err = runtime
            .resolve_model(&ModelSelection {
                provider: "deepseek".into(),
                model: "ghost".into(),
                reasoning_effort: None,
            })
            .await
            .unwrap_err();
        assert!(err.to_string().contains("deepseek/ghost"));
        assert!(err.to_string().contains("deepseek-chat"));
    }

    #[tokio::test]
    async fn reload_config_swaps_and_keeps_old_on_failure() {
        let dir = std::env::temp_dir().join(format!("llmn-reload-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("llmn.toml");
        let write = |text: &str| std::fs::write(&path, text).unwrap();

        write(
            r#"
[providers.deepseek]
protocol = "openai"
api_key = "test-key"
base_url = "https://api.deepseek.com"
default_model = "chat"
[providers.deepseek.models.chat]
model = "deepseek-chat"
"#,
        );
        let runtime = Runtime::open(&path, path.with_extension("data"))
            .await
            .expect("runtime builds");
        assert_eq!(
            runtime.default_model().await.unwrap().model,
            "deepseek-chat"
        );

        // a second model + new default hot-swap in
        write(
            r#"
[providers.deepseek]
protocol = "openai"
api_key = "test-key"
base_url = "https://api.deepseek.com"
default_model = "reasoner"
[providers.deepseek.models.chat]
model = "deepseek-chat"
[providers.deepseek.models.reasoner]
model = "deepseek-reasoner"
"#,
        );
        runtime.reload_config(&path).await.expect("reload succeeds");
        let default = runtime.default_model().await.unwrap();
        assert_eq!(default.model, "deepseek-reasoner");
        assert_eq!(runtime.list_models().await.len(), 2);

        // a broken document is refused; the previous configuration keeps serving
        write(
            r#"
[providers.deepseek]
protocol = "openai"
api_key = "test-key"
base_url = "https://api.deepseek.com"
default_model = "ghost"
[providers.deepseek.models.chat]
model = "deepseek-chat"
"#,
        );
        let err = runtime.reload_config(&path).await.unwrap_err();
        assert!(err.to_string().contains("default_model"));
        assert!(err.to_string().contains("ghost"));
        assert_eq!(
            runtime.default_model().await.unwrap().model,
            "deepseek-reasoner"
        );
        assert_eq!(runtime.list_models().await.len(), 2);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn refresh_auto_reloads_newly_added_provider() {
        let dir = std::env::temp_dir().join(format!("llmn-refresh-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("llmn.toml");
        let write = |text: &str| std::fs::write(&path, text).unwrap();

        write(
            r#"
[providers.chatecnu]
protocol = "openai"
api_key = "k"
base_url = "https://example.invalid/v1"
[providers.chatecnu.models.m]
model = "m"
"#,
        );
        let runtime = Runtime::open(&path, path.with_extension("data"))
            .await
            .expect("runtime builds");

        // a brand-new provider lands in the config file while the runtime is
        // running (no watcher in this test): refresh of a provider that is
        // not in the in-memory snapshot must auto-reload the document first.
        write(
            r#"
[providers.mine]
protocol = "openai"
api_key = "k"
base_url = "http://127.0.0.1:9/v1"
[providers.mine.models.m]
model = "m"
"#,
        );
        // After the auto-reload the provider exists, so the retry reaches the
        // network layer (unreachable port) instead of "provider not found".
        let err = runtime.refresh_models("mine").await.unwrap_err();
        match err {
            RuntimeError::AiError(ai_client::error::AiError::Reqwest(_)) => {}
            other => panic!("expected network error after auto-reload, got {other:?}"),
        }

        // a provider absent from the config entirely still errors as unknown
        let err = runtime.refresh_models("ghost").await.unwrap_err();
        assert!(matches!(
            err,
            RuntimeError::AiError(ai_client::error::AiError::ProviderNotFound(..))
        ));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn invalid_provider_edit_preserves_disk_and_runtime() {
        let path = std::env::temp_dir().join(format!("llmn-config-{}.toml", uuid::Uuid::new_v4()));
        let original = "[providers.deepseek]\napi_key = 'fixture'\n";
        std::fs::write(&path, original).unwrap();
        let rt = Runtime::open(&path, path.with_extension("data"))
            .await
            .unwrap();
        let before = rt.default_model().await.unwrap();
        let draft = crate::config::persist::ProviderDraft {
            id: "deepseek".into(),
            protocol: Some("unsupported".into()),
            base_url: None,
            api_key: None,
            models: None,
        };
        assert!(rt.upsert_provider(&draft).await.is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
        assert_eq!(rt.default_model().await.unwrap(), before);
        std::fs::remove_file(path).unwrap();
    }

    #[tokio::test]
    async fn removing_last_implicit_provider_leaves_loadable_config() {
        let path =
            std::env::temp_dir().join(format!("llmn-remove-last-{}.toml", uuid::Uuid::new_v4()));
        std::fs::write(
            &path,
            "# hand-written config\n[gui]\ntemperature = 0.4\ncurrentModel = { provider = 'deepseek', model = 'deepseek-chat' }\n[providers.deepseek]\napi_key = 'fixture'\n",
        )
        .unwrap();
        let rt = Runtime::open(&path, path.with_extension("data"))
            .await
            .unwrap();
        rt.remove_provider("deepseek").await.unwrap();
        assert!(rt.list_models().await.is_empty());
        rt.shutdown_features().await.unwrap();
        drop(rt);
        let restarted = Runtime::open(&path, path.with_extension("data"))
            .await
            .unwrap();
        assert!(restarted.default_model().await.is_none());
        assert!(
            std::fs::read_to_string(&path)
                .unwrap()
                .contains("temperature = 0.4")
        );
        assert!(
            std::fs::read_to_string(&path)
                .unwrap()
                .contains("# hand-written config")
        );
        std::fs::remove_file(path).unwrap();
    }
}
