use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::RwLock;

use crate::ai_provider::AiProvider;
use crate::catalog::effective_base_url;
use crate::config::{ModelConfig, ModelId, Protocol, ProviderConfig, ProviderId};
use crate::error::{AiError, Result};
use crate::protocols::anthropic::provider::AnthropicProvider;
use crate::protocols::gemini::provider::GeminiProvider;
use crate::protocols::openai::provider::OpenAIProvider;
use crate::protocols::openai_responses::provider::OpenAIResponsesProvider;
use crate::request::{ChatRequest, ModelSelection};
use crate::response::ProviderResponse;
use crate::router::{ModelInfo, ModelRouter, ResolvedSelection};
use crate::stream::ChatStream;

/// Catalog-backed adapter lookup key: provider route + effective protocol.
type RouteKey = (ProviderId, Protocol);
/// Catalog-backed adapter table.
type RouteTable = HashMap<RouteKey, Arc<dyn AiProvider>>;

#[derive(Clone)]
pub struct AiClient {
    /// Catalog-backed routes keyed by (provider id, effective protocol).
    /// Built by [`AiClient::from_config`] / [`AiClient::reload_config`]: a
    /// provider whose models override the default protocol gets one adapter
    /// per protocol in use (all sharing the route's key/base_url/headers).
    routes: Arc<RwLock<RouteTable>>,
    /// Legacy/manual routes keyed by provider id ([`AiClient::register`]),
    /// used only while no catalog is mounted.
    legacy: Arc<RwLock<HashMap<ProviderId, Arc<dyn AiProvider>>>>,
    catalog: Arc<RwLock<Option<ModelRouter>>>,
    /// Source configuration snapshot; the catalog/routes are rebuilt from it
    /// on reload and on `/models` refresh.
    configs: Arc<RwLock<HashMap<ProviderId, ProviderConfig>>>,
}

impl AiClient {
    pub fn new() -> Self {
        Self {
            routes: Arc::new(RwLock::new(HashMap::new())),
            legacy: Arc::new(RwLock::new(HashMap::new())),
            catalog: Arc::new(RwLock::new(None)),
            configs: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Register a provider route directly. Manual registrations have no model
    /// catalog: routing falls back to legacy provider-only behavior (model and
    /// reasoning pass through unvalidated). Use [`AiClient::from_config`] for
    /// full routing.
    pub fn register(&self, id: ProviderId, provider: Arc<dyn AiProvider>) {
        let mut legacy = self.legacy.blocking_write();
        legacy.insert(id, provider);
    }

    /// OpenAI-compatible endpoint (legacy quickstart, no model catalog).
    pub fn openai_compat(name: impl Into<ProviderId>, base_url: &str, api_key: &str) -> Self {
        let client = Self::new();
        let id = name.into();
        let provider = OpenAIProvider::new(id.0.clone(), base_url, api_key.to_string())
            .expect("failed to build OpenAI-compatible provider");
        client.register(id, Arc::new(provider));
        client
    }

    pub fn deepseek(api_key: &str) -> Self {
        Self::openai_compat(
            ProviderId::new("deepseek"),
            "https://api.deepseek.com",
            api_key,
        )
    }

    pub fn anthropic(api_key: &str) -> Self {
        let client = Self::new();
        let provider = AnthropicProvider::new(
            "anthropic",
            "https://api.anthropic.com/v1",
            api_key.to_string(),
        )
        .expect("failed to build Anthropic provider");
        client.register(ProviderId::new("anthropic"), Arc::new(provider));
        client
    }

    /// Build the client from provider configuration: resolves the merged model
    /// catalog first (configuration errors fail here, naming the offending
    /// provider/default_model/level), then constructs the provider routes —
    /// one adapter per protocol in use per provider.
    pub fn from_config(configs: &HashMap<ProviderId, ProviderConfig>) -> Result<Self> {
        let router = ModelRouter::new(configs)?;
        let routes = build_routes(&router, configs)?;
        Ok(Self {
            routes: Arc::new(RwLock::new(routes)),
            legacy: Arc::new(RwLock::new(HashMap::new())),
            catalog: Arc::new(RwLock::new(Some(router))),
            configs: Arc::new(RwLock::new(configs.clone())),
        })
    }

    /// Atomically swap configuration snapshot and provider routes. A request in
    /// flight keeps the snapshot it resolved under; the next request sees the
    /// new one.
    pub async fn reload_config(&self, configs: &HashMap<ProviderId, ProviderConfig>) -> Result<()> {
        let router = ModelRouter::new(configs)?;
        let routes = build_routes(&router, configs)?;
        let mut catalog = self.catalog.write().await;
        let mut current = self.routes.write().await;
        let mut snapshot = self.configs.write().await;
        *catalog = Some(router);
        *current = routes;
        *snapshot = configs.clone();
        Ok(())
    }

    /// Refresh one provider's model list from its own `GET /models` endpoint:
    /// newly discovered wire ids are merged into the in-memory configuration
    /// (existing entries — builtin or configured — are kept as-is) and the
    /// catalog is rebuilt. Returns the newly added models (empty when nothing
    /// was new), which callers may persist back to the config document.
    pub async fn refresh_models(
        &self,
        provider: &str,
    ) -> Result<Vec<crate::ai_provider::WireModel>> {
        // Resolve the provider's default protocol and its adapter.
        let adapter = {
            let catalog = self.catalog.read().await;
            let router = catalog.as_ref().ok_or_else(|| {
                AiError::ConfigError("no model catalog mounted; refresh needs a config".into())
            })?;
            let protocols = router.provider_protocols(provider).ok_or_else(|| {
                AiError::ProviderNotFound(provider.into(), router.provider_ids().join(", "))
            })?;
            let protocol = protocols
                .first()
                .copied()
                .expect("a provider route always has its default protocol");
            let routes = self.routes.read().await;
            routes
                .get(&(ProviderId::new(provider), protocol))
                .cloned()
                .ok_or_else(|| {
                    AiError::ConfigError(format!(
                        "provider '{provider}': no adapter for protocol {protocol:?}"
                    ))
                })?
        };
        let fetched = adapter.list_models().await?;

        // Merge new wire ids into the configuration snapshot (by wire name).
        let added = {
            let mut configs = self.configs.write().await;
            let Some(cfg) = configs.get_mut(&ProviderId::new(provider)) else {
                return Err(AiError::ProviderNotFound(provider.into(), String::new()));
            };
            let mut added = Vec::new();
            for m in fetched {
                let known = cfg.models.values().any(|existing| existing.model == m.id);
                if !known {
                    cfg.models.insert(
                        ModelId::new(m.id.clone()),
                        ModelConfig {
                            model: m.id.clone(),
                            display_name: m.display_name.clone(),
                            context_window: m.context_window,
                            max_tokens: m.max_tokens,
                            reasoning: None,
                            protocol: None,
                        },
                    );
                    added.push(m);
                }
            }
            added
        };

        if !added.is_empty() {
            // Clone the snapshot before rebuilding: reload_config takes the
            // configs write lock, which must not run under our read guard.
            let snapshot = self.configs.read().await.clone();
            self.reload_config(&snapshot).await?;
        }
        Ok(added)
    }

    /// Every effective model across providers (empty when no catalog is
    /// mounted, i.e. legacy quickstart clients).
    pub async fn list_models(&self) -> Vec<ModelInfo> {
        let catalog = self.catalog.read().await;
        match catalog.as_ref() {
            Some(router) => router.list_models(),
            None => Vec::new(),
        }
    }

    /// The default selection across providers (empty when no catalog is
    /// mounted).
    pub async fn default_selection(&self) -> Option<ModelSelection> {
        let catalog = self.catalog.read().await;
        catalog.as_ref().and_then(|r| r.default_selection())
    }

    /// Strictly resolve a selection against the mounted catalog. Errors name
    /// the offending key and candidates; on the legacy path (no catalog) the
    /// selection passes through with no capabilities.
    pub async fn resolve(&self, selection: &ModelSelection) -> Result<Option<ResolvedSelection>> {
        let catalog = self.catalog.read().await;
        match catalog.as_ref() {
            Some(router) => Ok(Some(router.resolve(selection)?)),
            None => Ok(None),
        }
    }

    /// Resolve and fetch the provider route for a catalog-backed selection:
    /// the adapter speaking the model's effective protocol.
    pub async fn route_resolved(
        &self,
        resolved: &ResolvedSelection,
    ) -> Result<Arc<dyn AiProvider>> {
        let routes = self.routes.read().await;
        routes
            .get(&(
                ProviderId::new(&resolved.selection.provider),
                resolved.spec.protocol,
            ))
            .cloned()
            .ok_or_else(|| {
                AiError::ConfigError(format!(
                    "provider '{}': no adapter for protocol {:?}",
                    resolved.selection.provider, resolved.spec.protocol
                ))
            })
    }

    /// Fetch a legacy (manually registered) provider route by provider id.
    pub async fn route(&self, selection: &ModelSelection) -> Result<Arc<dyn AiProvider>> {
        let legacy = self.legacy.read().await;
        legacy
            .get(&ProviderId::new(&selection.provider))
            .cloned()
            .ok_or_else(|| {
                let available = legacy
                    .keys()
                    .map(|id| id.0.clone())
                    .collect::<Vec<_>>()
                    .join(", ");
                AiError::ProviderNotFound(selection.provider.clone(), available)
            })
    }

    /// Copy all routing state under the same read transaction. A run can keep
    /// this client across tool iterations even when the live config changes.
    pub async fn snapshot(&self) -> Self {
        let catalog = self.catalog.read().await;
        let routes = self.routes.read().await;
        let configs = self.configs.read().await;
        Self {
            catalog: Arc::new(RwLock::new(catalog.clone())),
            routes: Arc::new(RwLock::new(routes.clone())),
            configs: Arc::new(RwLock::new(configs.clone())),
            legacy: Arc::new(RwLock::new(self.legacy.read().await.clone())),
        }
    }

    /// Install a fully validated candidate without rebuilding it after saving.
    pub async fn install(&self, candidate: Self) {
        let mut catalog = self.catalog.write().await;
        let mut routes = self.routes.write().await;
        let mut configs = self.configs.write().await;
        *catalog = candidate.catalog.read().await.clone();
        *routes = candidate.routes.read().await.clone();
        *configs = candidate.configs.read().await.clone();
    }

    async fn prepare(&self, mut req: ChatRequest) -> Result<(Arc<dyn AiProvider>, ChatRequest)> {
        let catalog = self.catalog.read().await;
        let resolved = catalog
            .as_ref()
            .map(|r| r.resolve(&req.selection))
            .transpose()?;
        let provider = match &resolved {
            Some(r) => self.route_resolved(r).await?,
            None => self.route(&req.selection).await?,
        };
        req.resolved = resolved;
        Ok((provider, req))
    }

    pub async fn complete(&self, req: ChatRequest) -> Result<ProviderResponse> {
        let (provider, req) = self.prepare(req).await?;
        provider.complete(req).await
    }

    pub async fn complete_stream(&self, req: ChatRequest) -> Result<ChatStream> {
        let (provider, req) = self.prepare(req).await?;
        provider.complete_stream(req).await
    }
}

/// Build one adapter per protocol in use per provider, all sharing the
/// route's key/base_url/headers. Unimplemented protocols fail here (naming
/// the provider and protocol) instead of registering a stub that would panic
/// at request time.
fn build_routes(
    router: &ModelRouter,
    configs: &HashMap<ProviderId, ProviderConfig>,
) -> Result<RouteTable> {
    let mut routes = HashMap::new();
    for (id, cfg) in configs {
        let Some(protocols) = router.provider_protocols(&id.0) else {
            // Unreachable: the router was built from the same configs.
            continue;
        };
        for protocol in protocols {
            let adapter = build_provider(id, cfg, protocol)?;
            routes.insert((id.clone(), protocol), adapter);
        }
    }
    Ok(routes)
}

/// Protocol factory — the Rust counterpart of DSH's `PROTOCOLS` table. Maps a
/// wire protocol to the adapter implementing it, resolving the route's
/// credential and endpoint first (config override, else builtin directory).
fn build_provider(
    id: &ProviderId,
    cfg: &ProviderConfig,
    protocol: Protocol,
) -> Result<Arc<dyn AiProvider>> {
    let api_key = cfg
        .api_key
        .resolve()
        .map_err(|e| AiError::ConfigError(e.to_string()))?;
    let base_url = effective_base_url(&id.0, cfg)?;
    match protocol {
        Protocol::OpenAIChat | Protocol::OpenAI => Ok(Arc::new(OpenAIProvider::with_options(
            id.0.clone(),
            &base_url,
            api_key,
            &cfg.headers,
            cfg.timeout_ms,
        )?)),
        Protocol::OpenAIResponses => Ok(Arc::new(OpenAIResponsesProvider::with_options(
            id.0.clone(),
            &base_url,
            api_key,
            &cfg.headers,
            cfg.timeout_ms,
        )?)),
        Protocol::Anthropic => Ok(Arc::new(AnthropicProvider::with_options(
            id.0.clone(),
            &base_url,
            api_key,
            &cfg.headers,
            cfg.timeout_ms,
        )?)),
        Protocol::Gemini => Ok(Arc::new(GeminiProvider::with_options(
            id.0.clone(),
            &base_url,
            api_key,
            &cfg.headers,
            cfg.timeout_ms,
        )?)),
        // Ollama ships an OpenAI-compatible endpoint (`/v1/chat/completions`),
        // so the chat-completions adapter serves it directly; configure
        // `base_url = "http://localhost:11434/v1"`.
        Protocol::Ollama => Ok(Arc::new(OpenAIProvider::with_options(
            id.0.clone(),
            &base_url,
            api_key,
            &cfg.headers,
            cfg.timeout_ms,
        )?)),
    }
}

impl Default for AiClient {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{ApiKey, ModelConfig, ModelId};

    #[tokio::test]
    async fn routes_by_effective_protocol() {
        // gateway provider: default openai, one model overriding to responses
        let mut configs = HashMap::new();
        configs.insert(
            ProviderId::new("gateway"),
            ProviderConfig {
                protocol: Some(Protocol::OpenAI),
                api_key: ApiKey::Direct("k".into()),
                base_url: Some("https://gateway.example/v1".into()),
                models: {
                    let mut m = HashMap::new();
                    m.insert(
                        ModelId::new("chat"),
                        ModelConfig {
                            model: "chat-model".into(),
                            display_name: None,
                            context_window: None,
                            max_tokens: None,
                            reasoning: None,
                            protocol: None,
                        },
                    );
                    m.insert(
                        ModelId::new("resp"),
                        ModelConfig {
                            model: "resp-model".into(),
                            display_name: None,
                            context_window: None,
                            max_tokens: None,
                            reasoning: None,
                            protocol: Some(Protocol::OpenAIResponses),
                        },
                    );
                    m
                },
                default_model: None,
                headers: HashMap::new(),
                timeout_ms: None,
            },
        );
        let client = AiClient::from_config(&configs).unwrap();

        // inheriting model → chat-completions adapter
        let chat = client
            .resolve(&ModelSelection {
                provider: "gateway".into(),
                model: "chat-model".into(),
                reasoning_effort: None,
            })
            .await
            .unwrap()
            .unwrap();
        let adapter = client.route_resolved(&chat).await.unwrap();
        assert!(
            adapter
                .supported_protocols()
                .contains(&Protocol::OpenAIChat)
        );

        // overriding model → responses adapter
        let resp = client
            .resolve(&ModelSelection {
                provider: "gateway".into(),
                model: "resp-model".into(),
                reasoning_effort: None,
            })
            .await
            .unwrap()
            .unwrap();
        let adapter = client.route_resolved(&resp).await.unwrap();
        assert!(
            adapter
                .supported_protocols()
                .contains(&Protocol::OpenAIResponses)
        );
    }
}
