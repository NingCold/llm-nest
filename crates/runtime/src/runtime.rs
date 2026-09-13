use std::path::PathBuf;
use std::sync::Arc;

use ai_client::AiClient;
use ai_client::ModelInfo;
use ai_client::ModelSelection;
use ai_client::ResolvedSelection;
use common::{Message, SessionId};
use tokio::sync::RwLock;
use tokio::sync::broadcast;

use crate::command::Command;
use crate::error::{Result, RuntimeError};
use crate::event::RuntimeEvent;
use crate::event_bus::EventBus;
use crate::feature::{Feature, FeatureContext, FeatureRegistry};
use crate::session::Session;
use crate::session_manager::SessionManager;

#[derive(Clone)]
pub struct Runtime {
    config_updates: Arc<tokio::sync::Mutex<()>>,
    sessions: Arc<RwLock<SessionManager>>,
    llm: Arc<AiClient>,
    event_bus: EventBus<RuntimeEvent>,
    features: Arc<RwLock<FeatureRegistry>>,
    /// Tools available to features (chat agent loop). Pre-registered with
    /// built-ins; call [`Runtime::register_trusted_tool`] to add more.
    tools: Arc<tools::ToolRegistry>,
    /// Config document path, when the runtime was created via
    /// [`Runtime::from_config`]; used to auto-reload before a `/refresh` of a
    /// provider that is not yet in the in-memory snapshot.
    pub(crate) config_path: Option<PathBuf>,
}

impl Runtime {
    pub fn new(
        sessions: SessionManager,
        event_bus: EventBus<RuntimeEvent>,
        llm: Arc<AiClient>,
    ) -> Self {
        let sessions = Arc::new(RwLock::new(sessions));

        Self {
            config_updates: Arc::new(tokio::sync::Mutex::new(())),
            sessions,
            llm,
            event_bus,
            features: Arc::new(RwLock::new(FeatureRegistry::new())),
            tools: Arc::new(tools::ToolRegistry::with_builtins()),
            config_path: None,
        }
    }

    /// Explicitly trust an in-process extension. Built-in tools use isolated workers.
    pub fn register_trusted_tool(&self, tool: Arc<dyn tools::Tool>) {
        self.tools.register_trusted_in_process(tool);
    }

    pub fn llm_client(&self) -> Arc<AiClient> {
        self.llm.clone()
    }

    /// The default `ModelSelection` across providers, resolved through the
    /// model router (config `default_model` > builtin default > first model).
    pub async fn default_model(&self) -> Option<ModelSelection> {
        self.llm.default_selection().await
    }

    /// Settings are read from the shared config document; stale model selections
    /// fall back to the current catalog after a provider is removed.
    pub async fn gui_config(&self) -> Result<crate::config::GuiConfig> {
        let _update = self.config_updates.lock().await;
        let saved = match &self.config_path {
            Some(path) => crate::config::gui::read_gui(&crate::config::gui::read_document(path)?)?,
            None => None,
        };
        let default = self.default_model().await.unwrap_or(ModelSelection {
            provider: String::new(),
            model: String::new(),
            reasoning_effort: None,
        });
        let mut config = saved.unwrap_or(crate::config::GuiConfig {
            current_model: default.clone(),
            temperature: 0.7,
            max_tokens: None,
        });
        if self.resolve_model(&config.current_model).await.is_err() {
            config.current_model = default;
        }
        Ok(config)
    }

    pub async fn set_gui_config(&self, config: &crate::config::GuiConfig) -> Result<()> {
        let _update = self.config_updates.lock().await;
        config.validate()?;
        self.resolve_model(&config.current_model).await?;
        let path = self.config_path.as_ref().ok_or_else(|| {
            RuntimeError::ConfigError("当前 Runtime 未绑定配置文件，无法保存 GUI 设置".into())
        })?;
        let text =
            crate::config::gui::render_gui(&crate::config::gui::read_document(path)?, config)?;
        crate::config::persist::atomic_write(path, &text)
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
        self.llm.reload_config(config.providers()).await?;
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
            AiClient::from_config(config.providers())?
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
        let candidate = AiClient::from_config(config.providers())?;
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
        let candidate = AiClient::from_config(config.providers())?;
        crate::config::persist::atomic_write(&path, &next)?;
        self.llm.install(candidate).await;
        Ok(())
    }

    /// Strictly validate a selection against the mounted model catalog before
    /// switching to it. Errors name the offending key and its candidates.
    pub async fn resolve_model(&self, selection: &ModelSelection) -> Result<ResolvedSelection> {
        match self.llm.resolve(selection).await? {
            Some(resolved) => Ok(resolved),
            None => Err(RuntimeError::ConfigError(
                "no model catalog mounted; use Runtime::from_config".into(),
            )),
        }
    }

    /// Build and validate a selection from a frontend command target:
    /// `provider/model`, or a bare model name resolved across providers (a
    /// unique match only). Returns the validated selection, or an error naming
    /// the ambiguity or the candidate list.
    pub async fn select_model(&self, target: &str) -> Result<ModelSelection> {
        let selection = if let Some((provider, model)) = target.split_once('/') {
            if provider.trim().is_empty() || model.trim().is_empty() {
                return Err(RuntimeError::ConfigError(
                    "usage: /model <provider>/<model>".into(),
                ));
            }
            ModelSelection {
                provider: provider.trim().into(),
                model: model.trim().into(),
                reasoning_effort: None,
            }
        } else {
            let models = self.list_models().await;
            let matches: Vec<&ModelInfo> = models
                .iter()
                .filter(|mi| mi.spec.wire == target.trim())
                .collect();
            match matches.as_slice() {
                [single] => ModelSelection {
                    provider: single.provider.clone(),
                    model: single.spec.wire.clone(),
                    reasoning_effort: None,
                },
                [] => {
                    return Err(RuntimeError::ConfigError(format!(
                        "no model named '{target}'; use /models to list"
                    )));
                }
                _ => {
                    return Err(RuntimeError::ConfigError(format!(
                        "'{target}' is ambiguous across providers ({}); use /model <provider>/<model>",
                        matches
                            .iter()
                            .map(|m| m.provider.as_str())
                            .collect::<Vec<_>>()
                            .join(", ")
                    )));
                }
            }
        };
        self.resolve_model(&selection).await?;
        Ok(selection)
    }

    /// The effective model selection for a session: the session's remembered
    /// selection when it has one, otherwise the global default.
    pub async fn session_model(&self, session_id: &SessionId) -> Option<ModelSelection> {
        let remembered = {
            let sessions = self.sessions.read().await;
            sessions.get_model(session_id)
        };
        match remembered {
            Some(model) => Some(model),
            None => self.default_model().await,
        }
    }

    /// Remember a model selection on a session.
    pub async fn set_session_model(
        &self,
        session_id: &SessionId,
        selection: ModelSelection,
    ) -> Result<()> {
        self.sessions
            .write()
            .await
            .set_model(session_id, selection)?;
        Ok(())
    }

    pub fn context(&self) -> FeatureContext {
        FeatureContext {
            sessions: self.sessions.clone(),
            llm: self.llm.clone(),
            events: self.event_bus.clone(),
            tools: self.tools.clone(),
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<RuntimeEvent> {
        self.event_bus.subscribe()
    }

    pub fn emit(&self, event: RuntimeEvent) {
        self.event_bus.publish(event);
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
        features.shutdown_all().await
    }

    pub async fn get_feature<T: Feature + 'static>(&self, id: &str) -> Option<Arc<T>> {
        self.features.read().await.get_by_id(id)
    }

    // --- Session management ---

    pub async fn create_session(&self, title: Option<String>) -> Result<SessionId> {
        let id = self.sessions.write().await.create(title)?;
        self.event_bus
            .publish(RuntimeEvent::SessionCreated { session_id: id });
        Ok(id)
    }

    pub async fn delete_session(&self, session_id: SessionId) -> Result<()> {
        self.sessions.write().await.remove(&session_id)?;
        self.event_bus
            .publish(RuntimeEvent::SessionDeleted { session_id });
        Ok(())
    }

    pub async fn rename_session(&self, session_id: SessionId, title: String) -> Result<()> {
        self.sessions.write().await.set_title(&session_id, title)?;
        self.event_bus
            .publish(RuntimeEvent::SessionChanged { session_id });
        Ok(())
    }

    /// Version-checked feedback mutation by persistent message ID.
    pub async fn feedback_by_id(
        &self,
        session_id: common::SessionId,
        message_id: &str,
        revision: &str,
        feedback: Option<common::Feedback>,
    ) -> Result<()> {
        self.sessions
            .write()
            .await
            .feedback_by_id(&session_id, message_id, revision, feedback)
    }

    pub async fn get_session(&self, session_id: &SessionId) -> Option<Session> {
        self.sessions.read().await.get(session_id).cloned()
    }

    /// All session ids, most recently updated first.
    pub async fn list_sessions(&self) -> Vec<SessionId> {
        let sessions = self.sessions.read().await;
        let mut ids: Vec<(SessionId, chrono::DateTime<chrono::Utc>)> = sessions
            .iter()
            .map(|(id, s)| (*id, s.updated_at()))
            .collect();
        ids.sort_by_key(|(_, updated_at)| std::cmp::Reverse(*updated_at));
        ids.into_iter().map(|(id, _)| id).collect()
    }

    pub async fn push_message(&self, session_id: &SessionId, message: Message) -> Result<()> {
        self.sessions
            .write()
            .await
            .push_message(session_id, message)
    }

    pub async fn get_messages(&self, session_id: &SessionId) -> Vec<Message> {
        self.sessions.read().await.get_messages(session_id)
    }

    // --- Command execution ---

    pub async fn execute(&self, command: Command) -> Result<()> {
        match command {
            Command::CreateSession { title } => {
                self.create_session(title).await?;
            }
            Command::DeleteSession { session_id } => {
                self.delete_session(session_id).await?;
            }
            Command::RenameSession { session_id, title } => {
                self.rename_session(session_id, title).await?;
            }
            _ => {}
        }
        Ok(())
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
    use crate::builder::RuntimeBuilder;
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
    async fn builder_routes_default_model_and_catalog() {
        let runtime = RuntimeBuilder::new()
            .config(deepseek_config())
            .build()
            .expect("runtime builds");

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
    async fn select_model_accepts_provider_prefixed_and_bare_names() {
        let runtime = RuntimeBuilder::new()
            .config(deepseek_config())
            .build()
            .expect("runtime builds");

        let prefixed = runtime
            .select_model("deepseek/deepseek-reasoner")
            .await
            .unwrap();
        assert_eq!(prefixed.model, "deepseek-reasoner");

        // bare name resolves when unique across providers
        let bare = runtime.select_model("deepseek-chat").await.unwrap();
        assert_eq!(bare.model, "deepseek-chat");

        // unknown name errors instead of silently switching
        let err = runtime.select_model("ghost").await.unwrap_err();
        assert!(err.to_string().contains("ghost"));
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
        let runtime = Runtime::from_config(&path).expect("runtime builds");
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
        let runtime = Runtime::from_config(&path).expect("runtime builds");

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
    async fn session_remembers_model_and_falls_back_to_default() {
        let runtime = RuntimeBuilder::new()
            .config(deepseek_config())
            .build()
            .expect("runtime builds");

        let sid_a = runtime.create_session(None).await.unwrap();
        let sid_b = runtime.create_session(None).await.unwrap();

        // no remembered selection yet → global default
        assert_eq!(
            runtime.session_model(&sid_a).await.unwrap().model,
            "deepseek-chat"
        );

        // remember a per-session selection; the other session is unaffected
        let selected = ModelSelection {
            provider: "deepseek".into(),
            model: "deepseek-reasoner".into(),
            reasoning_effort: None,
        };
        runtime
            .set_session_model(&sid_a, selected.clone())
            .await
            .expect("set_session_model");
        assert_eq!(
            runtime.session_model(&sid_a).await.unwrap().model,
            "deepseek-reasoner"
        );
        assert_eq!(
            runtime.session_model(&sid_b).await.unwrap().model,
            "deepseek-chat"
        );

        // unknown session errors
        let ghost = common::SessionId::new();
        assert!(
            runtime
                .set_session_model(&ghost, selected.clone())
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn persistence_survives_runtime_restart() {
        let dir = std::env::temp_dir().join(format!("llmn-persist-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);

        // "first process": create, chat, rename, remember a model
        {
            let runtime = RuntimeBuilder::new()
                .config(deepseek_config())
                .storage_dir(&dir)
                .build()
                .expect("runtime builds");
            let sid = runtime
                .create_session(Some("persisted".into()))
                .await
                .expect("create");
            runtime
                .push_message(&sid, common::Message::user("hello"))
                .await
                .expect("push user");
            runtime
                .push_message(&sid, common::Message::assistant("hi there"))
                .await
                .expect("push assistant");
            let selected = ModelSelection {
                provider: "deepseek".into(),
                model: "deepseek-reasoner".into(),
                reasoning_effort: None,
            };
            runtime
                .set_session_model(&sid, selected)
                .await
                .expect("set model");
            runtime
                .rename_session(sid, "renamed".into())
                .await
                .expect("rename");
        } // runtime dropped, like a process exit

        // "second process": everything is reloaded from the store
        let runtime = RuntimeBuilder::new()
            .config(deepseek_config())
            .storage_dir(&dir)
            .build()
            .expect("runtime rebuilds");
        let sessions = runtime.list_sessions().await;
        assert_eq!(sessions.len(), 1);
        let s = runtime.get_session(&sessions[0]).await.expect("session");
        assert_eq!(s.title(), Some("renamed"));
        assert_eq!(s.messages().len(), 2);
        assert_eq!(s.messages()[0].text(), "hello");
        assert_eq!(s.messages()[1].text(), "hi there");
        assert_eq!(
            runtime.session_model(&sessions[0]).await.unwrap().model,
            "deepseek-reasoner"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn persistence_keeps_reasoning_and_tool_parts() {
        use common::{ContentPart, Role, ToolCall};

        let dir = std::env::temp_dir().join(format!("llmn-persist-rt-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);

        {
            let runtime = RuntimeBuilder::new()
                .config(deepseek_config())
                .storage_dir(&dir)
                .build()
                .expect("runtime builds");
            let sid = runtime.create_session(None).await.expect("create");
            // reasoning persists as part of the assistant message
            runtime
                .push_message(
                    &sid,
                    common::Message::assistant_with_reasoning("answer", "think think"),
                )
                .await
                .expect("push with reasoning");
            // tool call/result parts persist as part of the content
            runtime
                .push_message(
                    &sid,
                    common::Message {
                        role: Role::Assistant,
                        content: vec![
                            ContentPart::Text("calling".into()),
                            ContentPart::ToolCall(ToolCall {
                                id: "call_1".into(),
                                name: "web_search".into(),
                                arguments: r#"{"query":"rust"}"#.into(),
                                thought_signature: None,
                            }),
                        ],
                        reasoning: None,
                        created_at: None,
                        thinking_ms: None,
                        usage: None,
                        timings: None,
                        feedback: None,
                        interruption: None,
                        id: Some(common::MessageId::new()),
                    },
                )
                .await
                .expect("push with tool call");
        }

        let runtime = RuntimeBuilder::new()
            .config(deepseek_config())
            .storage_dir(&dir)
            .build()
            .expect("runtime rebuilds");
        let s = runtime
            .get_session(&runtime.list_sessions().await[0])
            .await
            .expect("session");
        assert_eq!(s.messages().len(), 2);
        assert_eq!(s.messages()[0].reasoning(), Some("think think"));
        assert_eq!(s.messages()[0].text(), "answer");
        assert!(matches!(
            s.messages()[1].content[1],
            ContentPart::ToolCall(ref tc) if tc.name == "web_search"
        ));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn feedback_persists_across_restart_and_clears() {
        let dir =
            std::env::temp_dir().join(format!("llmn-persist-feedback-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);

        {
            let runtime = RuntimeBuilder::new()
                .config(deepseek_config())
                .storage_dir(&dir)
                .build()
                .expect("runtime builds");
            let sid = runtime.create_session(None).await.expect("create session");
            runtime
                .push_message(&sid, common::Message::user("hi"))
                .await
                .expect("push user");
            runtime
                .push_message(&sid, common::Message::assistant("yo"))
                .await
                .expect("push assistant");
            let s = runtime.get_session(&sid).await.unwrap();
            runtime
                .feedback_by_id(
                    sid,
                    &s.messages()[1].id.unwrap().to_string(),
                    &s.updated_at().to_rfc3339(),
                    Some(common::Feedback::Up),
                )
                .await
                .expect("set feedback");
            let s = runtime.get_session(&sid).await.expect("session");
            assert_eq!(s.messages()[1].feedback, Some(common::Feedback::Up));
        };

        // restart: feedback survives
        {
            let runtime = RuntimeBuilder::new()
                .config(deepseek_config())
                .storage_dir(&dir)
                .build()
                .expect("runtime builds");
            let sid = runtime.list_sessions().await[0];
            let s = runtime.get_session(&sid).await.expect("session");
            assert_eq!(s.messages()[1].feedback, Some(common::Feedback::Up));

            // clear it
            runtime
                .feedback_by_id(
                    sid,
                    &s.messages()[1].id.unwrap().to_string(),
                    &s.updated_at().to_rfc3339(),
                    None,
                )
                .await
                .expect("clear feedback");
            let s = runtime.get_session(&sid).await.expect("session");
            assert_eq!(s.messages()[1].feedback, None);

            // out-of-range index errors
            let err = runtime
                .feedback_by_id(
                    sid,
                    "deleted-id",
                    &s.updated_at().to_rfc3339(),
                    Some(common::Feedback::Down),
                )
                .await
                .unwrap_err();
            assert!(err.to_string().contains("no longer exists"));
        }

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn persistence_delete_removes_session_from_store() {
        let dir = std::env::temp_dir().join(format!("llmn-persist-delete-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);

        {
            let runtime = RuntimeBuilder::new()
                .config(deepseek_config())
                .storage_dir(&dir)
                .build()
                .expect("runtime builds");
            let sid = runtime.create_session(None).await.expect("create");
            runtime
                .push_message(&sid, common::Message::user("a"))
                .await
                .expect("push");
            runtime.delete_session(sid).await.expect("delete");
        }

        let runtime = RuntimeBuilder::new()
            .config(deepseek_config())
            .storage_dir(&dir)
            .build()
            .expect("runtime rebuilds");
        assert!(runtime.list_sessions().await.is_empty());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn persistence_failure_fails_create_and_leaves_memory_unchanged() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        // Real store whose saves fail after the first one (create ok, then disk full).
        struct FlakyStore {
            inner: storage::FileSessionStore,
            saves: AtomicUsize,
        }
        impl storage::SessionStore for FlakyStore {
            fn save_session(&self, record: &storage::SessionRecord) -> storage::Result<()> {
                if self
                    .saves
                    .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| {
                        (n > 0).then(|| n - 1)
                    })
                    .is_err()
                {
                    return Err(storage::StorageError::Io(std::io::Error::other(
                        "disk full",
                    )));
                }
                self.inner.save_session(record)
            }
            fn load_sessions(&self) -> storage::Result<Vec<storage::SessionRecord>> {
                self.inner.load_sessions()
            }
            fn delete_session(&self, id: &common::SessionId) -> storage::Result<()> {
                self.inner.delete_session(id)
            }
        }

        let dir = std::env::temp_dir().join(format!("llmn-persist-fail-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let store = std::sync::Arc::new(FlakyStore {
            inner: storage::FileSessionStore::new(&dir).expect("store"),
            saves: AtomicUsize::new(1),
        });

        let mut manager = SessionManager::from_store(store).expect("load");
        let sid = manager.create(Some("t".into())).expect("first save ok");
        assert_eq!(manager.get_messages(&sid).len(), 0);

        // A failed write surfaces the error and leaves the session unchanged.
        let err = manager
            .push_message(&sid, common::Message::user("boom"))
            .expect_err("second save fails");
        assert!(err.to_string().contains("disk full"));
        assert_eq!(manager.get_messages(&sid).len(), 0);
        assert_eq!(manager.get(&sid).unwrap().title(), Some("t"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn list_sessions_orders_most_recent_first() {
        let runtime = RuntimeBuilder::new()
            .config(deepseek_config())
            .build()
            .expect("runtime builds");

        let a = runtime.create_session(None).await.unwrap();
        let b = runtime.create_session(None).await.unwrap();
        assert_eq!(runtime.list_sessions().await, vec![b, a]);

        // touching the older session moves it to the front
        runtime
            .push_message(&a, common::Message::user("x"))
            .await
            .unwrap();
        assert_eq!(runtime.list_sessions().await, vec![a, b]);
    }
    #[tokio::test]
    async fn invalid_provider_edit_preserves_disk_and_runtime() {
        let path =
            std::env::temp_dir().join(format!("llmn-config-{}.toml", common::SessionId::new()));
        let original = "[providers.deepseek]\napi_key = 'fixture'\n";
        std::fs::write(&path, original).unwrap();
        let rt = Runtime::from_config(&path).unwrap();
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
    async fn empty_catalog_can_add_remove_and_restart_without_losing_history() {
        let dir =
            std::env::temp_dir().join(format!("llmn-onboarding-{}", common::SessionId::new()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("llmn.toml");
        std::fs::write(&path, "[providers]\n").unwrap();
        let session_id;
        {
            let rt = Runtime::from_config_persistent(&path, &dir).unwrap();
            assert!(rt.default_model().await.is_none());
            assert!(
                rt.gui_config()
                    .await
                    .unwrap()
                    .current_model
                    .model
                    .is_empty()
            );
            session_id = rt.create_session(Some("first run".into())).await.unwrap();
            let draft = crate::config::persist::ProviderDraft {
                id: "deepseek".into(),
                protocol: None,
                base_url: None,
                api_key: Some("fixture".into()),
                models: None,
            };
            rt.upsert_provider(&draft).await.unwrap();
            let selection = rt.default_model().await.unwrap();
            rt.remove_provider("deepseek").await.unwrap();
            assert!(rt.list_models().await.is_empty());
            assert!(rt.resolve_model(&selection).await.is_err());
        }
        {
            let rt = Runtime::from_config_persistent(&path, &dir).unwrap();
            assert!(rt.default_model().await.is_none());
            assert_eq!(rt.list_sessions().await, vec![session_id]);
        }
        std::fs::remove_dir_all(dir).unwrap();
    }
}
