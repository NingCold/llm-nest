//! Model routing: catalog merge, per-provider defaults, and strict resolution
//! with actionable diagnostics.
//!
//! Mirrors the Pi/DSH split in reduced form: this is the *selection* layer
//! (which provider, which model, which reasoning effort), kept deliberately
//! separate from request transport. [`ModelRouter`] is a detached snapshot of
//! the provider configuration, built once per configuration (no live mutation
//! mid-request), so a request freezes its resolved selection before its first
//! await — switching models takes effect on the next request, never inside
//! one in flight.
//!
//! Resolution is strict: an unknown provider or model fails naming the
//! candidates. Pi-style fuzzy matching (aliases, dated versions,
//! case-insensitive fallback, `model:thinking-level` suffixes) is deliberately
//! not implemented; the API shapes below leave room for it.

use std::collections::{BTreeMap, HashMap};

use crate::catalog::{
    builtin_default_model, builtin_model, builtin_provider, effective_base_url, effective_protocol,
    to_reasoning_capability,
};
use crate::config::{ModelConfig, Protocol, ProviderConfig, ProviderId};
use crate::error::{AiError, Result};
use crate::reasoning::ReasoningCapability;
use crate::request::ModelSelection;

/// One model in the effective catalog: capabilities merged from the builtin
/// table and the configuration (configuration wins field by field).
#[derive(Debug, Clone)]
pub struct ModelSpec {
    /// Configuration key of the model (the `models.<key>` in `llmn.toml`).
    pub id: String,
    /// The model name sent on the wire.
    pub wire: String,
    /// Name shown by frontends.
    pub display_name: String,
    /// Context capacity; informational, never enforced on requests.
    pub context_window: Option<u32>,
    /// Output capability; informational, never enforced on requests.
    pub max_tokens: Option<u32>,
    /// Effective wire protocol for this model: the model-level override when
    /// declared, otherwise the provider's default protocol.
    pub protocol: Protocol,
    /// Declared reasoning support; `None` means the model takes no effort field.
    pub reasoning: Option<ReasoningCapability>,
}

/// One entry of [`ModelRouter::list_models`].
#[derive(Debug, Clone)]
pub struct ModelInfo {
    pub provider: String,
    pub spec: ModelSpec,
}

/// Successful routing of a selection: the selection plus the resolved model
/// capabilities, captured before the request is dispatched.
#[derive(Debug, Clone)]
pub struct ResolvedSelection {
    pub selection: ModelSelection,
    pub spec: ModelSpec,
}

/// Effective model catalog of one provider route.
#[derive(Debug, Clone)]
pub struct ProviderCatalog {
    /// Wire protocol of the route (provider default; models may override).
    pub protocol: Protocol,
    /// API endpoint of the route.
    pub base_url: String,
    /// Default model for this provider, given as wire name or config key.
    pub default_model: String,
    /// Models keyed by configuration key, in deterministic (BTree) order.
    pub models: BTreeMap<String, ModelSpec>,
}

impl ProviderCatalog {
    /// Find a model by wire name or configuration key.
    pub fn find(&self, model: &str) -> Option<&ModelSpec> {
        self.models
            .values()
            .find(|s| s.wire == model || s.id == model)
    }

    fn available_names(&self) -> Vec<String> {
        self.models
            .values()
            .flat_map(|s| [s.wire.clone(), s.id.clone()])
            .collect()
    }
}

/// Model router owning the merged catalog for every configured provider.
/// Immutable after construction; rebuild via [`ModelRouter::new`] for a new
/// configuration snapshot.
#[derive(Debug, Clone)]
pub struct ModelRouter {
    providers: BTreeMap<String, ProviderCatalog>,
}

impl ModelRouter {
    /// Build the merged catalog from provider configuration. Fails fast on
    /// configuration the router cannot serve:
    /// - a provider with no models (configured or builtin);
    /// - a provider with neither configured nor builtin `protocol`/`base_url`;
    /// - a `default_model` naming a model the provider does not have;
    /// - a reasoning capability with an empty level list.
    pub fn new(configs: &HashMap<ProviderId, ProviderConfig>) -> Result<Self> {
        let mut providers = BTreeMap::new();
        for (pid, pcfg) in configs {
            let provider = pid.0.clone();
            let protocol = effective_protocol(&provider, pcfg)?;
            let base_url = effective_base_url(&provider, pcfg)?;
            let mut models = BTreeMap::new();
            for (mid, mcfg) in &pcfg.models {
                let spec = Self::build_spec(&provider, mid.0.clone(), mcfg, protocol)?;
                models.insert(mid.0.clone(), spec);
            }
            // Fill the builtin model list: a config model with the same wire
            // id wins (field-by-field); every other builtin model is appended.
            if let Some(builtin) = builtin_provider(&provider) {
                for bm in builtin.models {
                    if !models.values().any(|s| s.wire == bm.id) {
                        models.insert(bm.id.to_string(), Self::builtin_spec(bm, protocol));
                    }
                }
            }
            if models.is_empty() {
                return Err(AiError::ConfigError(format!(
                    "provider '{provider}' has no models (configured or builtin)"
                )));
            }
            if let Some(configured) = &pcfg.default_model {
                let known = models
                    .values()
                    .any(|s| s.wire == *configured || s.id == *configured);
                if !known {
                    return Err(AiError::ConfigError(format!(
                        "provider '{provider}' default_model '{configured}' is not a model of this provider"
                    )));
                }
            }
            let default_model = pcfg
                .default_model
                .clone()
                .or_else(|| {
                    builtin_default_model(&provider).and_then(|d| {
                        models
                            .values()
                            .any(|s| s.wire == d || s.id == d)
                            .then(|| d.to_string())
                    })
                })
                .unwrap_or_else(|| {
                    // Deterministic: the first model in BTree order.
                    models
                        .values()
                        .next()
                        .expect("models is non-empty")
                        .wire
                        .clone()
                });
            providers.insert(
                provider.clone(),
                ProviderCatalog {
                    protocol,
                    base_url,
                    default_model,
                    models,
                },
            );
        }
        Ok(Self { providers })
    }

    fn build_spec(
        provider: &str,
        id: String,
        mcfg: &ModelConfig,
        provider_protocol: Protocol,
    ) -> Result<ModelSpec> {
        if mcfg
            .reasoning
            .as_ref()
            .is_some_and(|reasoning| reasoning.levels.is_empty())
        {
            return Err(AiError::ConfigError(format!(
                "provider '{provider}' model '{id}' declares an empty reasoning level list"
            )));
        }
        let builtin = builtin_model(provider, &mcfg.model);
        Ok(ModelSpec {
            id,
            wire: mcfg.model.clone(),
            display_name: mcfg.display_name.clone().unwrap_or_else(|| {
                builtin
                    .map(|b| b.display_name.to_string())
                    .unwrap_or_else(|| mcfg.model.clone())
            }),
            context_window: mcfg
                .context_window
                .or_else(|| builtin.and_then(|b| b.context_window)),
            max_tokens: mcfg
                .max_tokens
                .or_else(|| builtin.and_then(|b| b.max_tokens)),
            protocol: mcfg.protocol.unwrap_or(provider_protocol),
            reasoning: mcfg
                .reasoning
                .clone()
                .or_else(|| builtin.and_then(|b| b.reasoning.map(|r| to_reasoning_capability(&r)))),
        })
    }

    /// A model taken straight from the builtin directory (not declared in
    /// config); inherits the provider protocol unless the builtin entry
    /// overrides it.
    fn builtin_spec(
        bm: &crate::catalog::BuiltinModelEntry,
        provider_protocol: Protocol,
    ) -> ModelSpec {
        ModelSpec {
            id: bm.id.to_string(),
            wire: bm.id.to_string(),
            display_name: bm.display_name.to_string(),
            context_window: bm.context_window,
            max_tokens: bm.max_tokens,
            protocol: bm.protocol.unwrap_or(provider_protocol),
            reasoning: bm.reasoning.map(|r| to_reasoning_capability(&r)),
        }
    }

    /// Provider route keys, in deterministic order.
    pub fn provider_ids(&self) -> Vec<String> {
        self.providers.keys().cloned().collect()
    }

    /// Every effective model across providers, in deterministic order.
    pub fn list_models(&self) -> Vec<ModelInfo> {
        self.providers
            .iter()
            .flat_map(|(provider, catalog)| {
                catalog.models.values().map(move |spec| ModelInfo {
                    provider: provider.clone(),
                    spec: spec.clone(),
                })
            })
            .collect()
    }

    /// Effective protocols in use by one provider route: its default protocol
    /// plus every model-level override. `None` when the provider is unknown.
    pub fn provider_protocols(&self, provider: &str) -> Option<Vec<Protocol>> {
        let catalog = self.providers.get(provider)?;
        let mut protocols = vec![catalog.protocol];
        for spec in catalog.models.values() {
            if !protocols.contains(&spec.protocol) {
                protocols.push(spec.protocol);
            }
        }
        Some(protocols)
    }

    /// Resolve a default selection: the first provider's default model.
    pub fn default_selection(&self) -> Option<ModelSelection> {
        let (provider, catalog) = self.providers.iter().next()?;
        let spec = catalog.find(&catalog.default_model)?;
        Some(ModelSelection {
            provider: provider.clone(),
            model: spec.wire.clone(),
            reasoning_effort: None,
        })
    }

    /// Strictly resolve a selection into provider + model capabilities.
    ///
    /// Errors name the offending key and the candidates:
    /// - unknown provider lists the configured providers;
    /// - unknown model lists every wire and config-key name of that provider;
    /// - an unsupported reasoning effort lists the model's declared levels.
    ///   `Off` is always accepted (it is a no-op for formats that cannot
    ///   express it).
    pub fn resolve(&self, selection: &ModelSelection) -> Result<ResolvedSelection> {
        let catalog = self.providers.get(&selection.provider).ok_or_else(|| {
            AiError::ProviderNotFound(selection.provider.clone(), self.provider_ids().join(", "))
        })?;
        let spec = catalog.find(&selection.model).ok_or_else(|| {
            AiError::ModelNotFound(
                selection.provider.clone(),
                selection.model.clone(),
                catalog.available_names().join(", "),
            )
        })?;
        match selection.reasoning_effort {
            Some(effort) if effort != crate::reasoning::ReasoningEffort::Off => {
                let supported = spec
                    .reasoning
                    .as_ref()
                    .is_some_and(|cap| cap.levels.contains(&effort));
                if !supported {
                    let available = spec
                        .reasoning
                        .as_ref()
                        .map(|cap| {
                            cap.levels
                                .iter()
                                .map(|l| l.as_wire().to_string())
                                .collect::<Vec<_>>()
                                .join(", ")
                        })
                        .unwrap_or_else(|| "none (model does not support reasoning)".into());
                    return Err(AiError::ReasoningNotSupported {
                        provider: selection.provider.clone(),
                        model: format!("{}/{}", selection.provider, spec.wire),
                        effort,
                        supported: available,
                    });
                }
            }
            _ => {}
        }
        Ok(ResolvedSelection {
            selection: selection.clone(),
            spec: spec.clone(),
        })
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;
    use crate::config::{ApiKey, ModelConfig, ModelId, Protocol};
    use crate::reasoning::{ReasoningCapability, ReasoningEffort, ReasoningFormat};

    fn config(base_url: &str, models: Vec<(String, ModelConfig)>) -> ProviderConfig {
        ProviderConfig {
            protocol: Some(Protocol::OpenAIChat),
            api_key: ApiKey::Direct("test-key".into()),
            base_url: Some(base_url.into()),
            models: models
                .into_iter()
                .map(|(k, v)| (ModelId::new(k), v))
                .collect(),
            default_model: None,
            headers: HashMap::new(),
            timeout_ms: None,
        }
    }

    fn model(wire: &str) -> ModelConfig {
        ModelConfig {
            model: wire.into(),
            display_name: None,
            context_window: None,
            max_tokens: None,
            reasoning: None,
            protocol: None,
        }
    }

    fn reasoning_model(wire: &str, levels: Vec<ReasoningEffort>) -> ModelConfig {
        ModelConfig {
            model: wire.into(),
            display_name: None,
            context_window: None,
            max_tokens: None,
            reasoning: Some(ReasoningCapability {
                levels,
                format: ReasoningFormat::OpenAIEffort,
                budget_tokens: None,
            }),
            protocol: None,
        }
    }

    fn router(configs: &HashMap<ProviderId, ProviderConfig>) -> ModelRouter {
        ModelRouter::new(configs).expect("router builds")
    }

    #[test]
    fn merges_builtin_capabilities() {
        let mut configs = HashMap::new();
        configs.insert(
            ProviderId::new("deepseek"),
            config(
                "https://api.deepseek.com",
                vec![
                    ("chat".into(), model("deepseek-chat")),
                    (
                        "custom".into(),
                        ModelConfig {
                            model: "my-model".into(),
                            display_name: Some("My Model".into()),
                            context_window: None,
                            max_tokens: Some(4096),
                            reasoning: None,
                            protocol: None,
                        },
                    ),
                ],
            ),
        );
        let r = router(&configs);
        let models = r.list_models();
        // config models + the builtin list appended (deepseek-reasoner)
        assert_eq!(models.len(), 3);
        let chat = models
            .iter()
            .find(|m| m.spec.wire == "deepseek-chat")
            .unwrap();
        // builtin fills the gap
        assert_eq!(chat.spec.context_window, Some(64 * 1024));
        assert_eq!(chat.spec.max_tokens, Some(8192));
        // builtin display name fills the gap too
        assert_eq!(chat.spec.display_name, "DeepSeek Chat");
        // model inherits the provider protocol
        assert_eq!(chat.spec.protocol, Protocol::OpenAIChat);
        // config values win over builtin
        let custom = models.iter().find(|m| m.spec.wire == "my-model").unwrap();
        assert_eq!(custom.spec.display_name, "My Model");
        assert_eq!(custom.spec.max_tokens, Some(4096));
        assert_eq!(custom.spec.context_window, None);
        // builtin-only model is appended with its capabilities
        let reasoner = models
            .iter()
            .find(|m| m.spec.wire == "deepseek-reasoner")
            .unwrap();
        assert_eq!(reasoner.spec.display_name, "DeepSeek Reasoner");
        assert_eq!(
            reasoner.spec.reasoning.as_ref().unwrap().format,
            ReasoningFormat::DeepSeekThinking
        );
    }

    #[test]
    fn default_model_priority() {
        let mut configs = HashMap::new();
        // explicit default_model wins
        configs.insert(
            ProviderId::new("deepseek"),
            ProviderConfig {
                default_model: Some("reasoner".into()),
                ..config(
                    "url",
                    vec![
                        ("chat".into(), model("deepseek-chat")),
                        ("reasoner".into(), model("deepseek-reasoner")),
                    ],
                )
            },
        );
        let r = router(&configs);
        let sel = r.default_selection().unwrap();
        assert_eq!(sel.provider, "deepseek");
        assert_eq!(sel.model, "deepseek-reasoner");

        // builtin default applies when configured and not overridden
        let mut configs = HashMap::new();
        configs.insert(
            ProviderId::new("deepseek"),
            config(
                "url",
                vec![
                    ("a".into(), model("deepseek-chat")),
                    ("b".into(), model("deepseek-reasoner")),
                ],
            ),
        );
        let r = router(&configs);
        assert_eq!(r.default_selection().unwrap().model, "deepseek-chat");

        // builtin default ignored when the provider does not ship it; first BTree entry wins
        let mut configs = HashMap::new();
        configs.insert(
            ProviderId::new("custom-gateway"),
            config("url", vec![("z".into(), model("zz-model"))]),
        );
        let r = router(&configs);
        assert_eq!(r.default_selection().unwrap().model, "zz-model");
    }

    #[test]
    fn resolves_by_wire_or_key() {
        let mut configs = HashMap::new();
        configs.insert(
            ProviderId::new("openai"),
            config("url", vec![("alias".into(), model("gpt-4o"))]),
        );
        let r = router(&configs);
        let by_wire = r
            .resolve(&ModelSelection {
                provider: "openai".into(),
                model: "gpt-4o".into(),
                reasoning_effort: None,
            })
            .unwrap();
        assert_eq!(by_wire.spec.id, "alias");
        let by_key = r
            .resolve(&ModelSelection {
                provider: "openai".into(),
                model: "alias".into(),
                reasoning_effort: None,
            })
            .unwrap();
        assert_eq!(by_key.spec.wire, "gpt-4o");
    }

    #[test]
    fn unknown_provider_lists_candidates() {
        let mut configs = HashMap::new();
        configs.insert(
            ProviderId::new("openai"),
            config("url", vec![("a".into(), model("gpt-4o"))]),
        );
        let r = router(&configs);
        let err = r
            .resolve(&ModelSelection {
                provider: "nope".into(),
                model: "gpt-4o".into(),
                reasoning_effort: None,
            })
            .unwrap_err();
        match err {
            AiError::ProviderNotFound(provider, available) => {
                assert_eq!(provider, "nope");
                assert!(available.contains("openai"));
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn unknown_model_lists_candidates() {
        let mut configs = HashMap::new();
        configs.insert(
            ProviderId::new("openai"),
            config("url", vec![("a".into(), model("gpt-4o"))]),
        );
        let r = router(&configs);
        // gpt-5 is appended by the builtin directory now; use a truly unknown id
        let err = r
            .resolve(&ModelSelection {
                provider: "openai".into(),
                model: "gpt-99".into(),
                reasoning_effort: None,
            })
            .unwrap_err();
        match err {
            AiError::ModelNotFound(provider, model, available) => {
                assert_eq!(provider, "openai");
                assert_eq!(model, "gpt-99");
                assert!(available.contains("gpt-4o"));
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn reasoning_effort_validation() {
        let mut configs = HashMap::new();
        configs.insert(
            ProviderId::new("openai"),
            config(
                "url",
                vec![
                    (
                        "reasoning".into(),
                        reasoning_model(
                            "gpt-5",
                            vec![
                                ReasoningEffort::Low,
                                ReasoningEffort::High,
                                ReasoningEffort::Max,
                            ],
                        ),
                    ),
                    ("plain".into(), model("gpt-4o")),
                ],
            ),
        );
        let r = router(&configs);

        // supported level passes
        r.resolve(&ModelSelection {
            provider: "openai".into(),
            model: "gpt-5".into(),
            reasoning_effort: Some(ReasoningEffort::High),
        })
        .unwrap();
        r.resolve(&ModelSelection {
            provider: "openai".into(),
            model: "gpt-5".into(),
            reasoning_effort: Some(ReasoningEffort::Max),
        })
        .unwrap();

        // unsupported level names the supported set
        let err = r
            .resolve(&ModelSelection {
                provider: "openai".into(),
                model: "gpt-5".into(),
                reasoning_effort: Some(ReasoningEffort::Medium),
            })
            .unwrap_err();
        match err {
            AiError::ReasoningNotSupported {
                effort, supported, ..
            } => {
                assert_eq!(effort, ReasoningEffort::Medium);
                assert!(supported.contains("low") && supported.contains("high"));
                assert!(supported.contains("max"));
            }
            other => panic!("unexpected error: {other:?}"),
        }

        // model without reasoning support rejects a real effort
        let err = r
            .resolve(&ModelSelection {
                provider: "openai".into(),
                model: "gpt-4o".into(),
                reasoning_effort: Some(ReasoningEffort::Low),
            })
            .unwrap_err();
        assert!(matches!(err, AiError::ReasoningNotSupported { .. }));

        // Off is always accepted, even on a non-reasoning model
        r.resolve(&ModelSelection {
            provider: "openai".into(),
            model: "gpt-4o".into(),
            reasoning_effort: Some(ReasoningEffort::Off),
        })
        .unwrap();
    }

    #[test]
    fn rejects_empty_reasoning_levels() {
        let mut configs = HashMap::new();
        configs.insert(
            ProviderId::new("openai"),
            config("url", vec![("a".into(), reasoning_model("gpt-5", vec![]))]),
        );
        let err = ModelRouter::new(&configs).unwrap_err();
        assert!(matches!(err, AiError::ConfigError(_)));
        assert!(err.to_string().contains("empty reasoning level list"));
    }

    #[test]
    fn rejects_unknown_default_model() {
        let mut configs = HashMap::new();
        configs.insert(
            ProviderId::new("openai"),
            ProviderConfig {
                default_model: Some("ghost".into()),
                ..config("url", vec![("a".into(), model("gpt-4o"))])
            },
        );
        let err = ModelRouter::new(&configs).unwrap_err();
        assert!(matches!(err, AiError::ConfigError(_)));
        assert!(err.to_string().contains("default_model"));
        assert!(err.to_string().contains("ghost"));
    }

    #[test]
    fn model_protocol_override() {
        let mut configs = HashMap::new();
        configs.insert(
            ProviderId::new("gateway"),
            ProviderConfig {
                protocol: Some(Protocol::OpenAI),
                api_key: ApiKey::Direct("k".into()),
                base_url: Some("https://gateway.example/v1".into()),
                models: {
                    let mut m = HashMap::new();
                    m.insert(ModelId::new("chat-model"), model("chat-model"));
                    m.insert(
                        ModelId::new("resp-model"),
                        ModelConfig {
                            protocol: Some(Protocol::OpenAIResponses),
                            ..model("resp-model")
                        },
                    );
                    m
                },
                default_model: None,
                headers: HashMap::new(),
                timeout_ms: None,
            },
        );
        let r = router(&configs);
        // inheriting model uses the provider protocol
        let chat = r
            .resolve(&ModelSelection {
                provider: "gateway".into(),
                model: "chat-model".into(),
                reasoning_effort: None,
            })
            .unwrap();
        assert_eq!(chat.spec.protocol, Protocol::OpenAI);
        // overriding model uses its own protocol
        let resp = r
            .resolve(&ModelSelection {
                provider: "gateway".into(),
                model: "resp-model".into(),
                reasoning_effort: None,
            })
            .unwrap();
        assert_eq!(resp.spec.protocol, Protocol::OpenAIResponses);
        // the route serves both protocols
        assert_eq!(
            r.provider_protocols("gateway").unwrap(),
            vec![Protocol::OpenAI, Protocol::OpenAIResponses]
        );
    }

    #[test]
    fn minimal_config_falls_back_to_builtin() {
        // only an api key: protocol/base_url/models/default all from the
        // builtin directory
        let mut configs = HashMap::new();
        configs.insert(
            ProviderId::new("deepseek"),
            ProviderConfig {
                protocol: None,
                api_key: ApiKey::Direct("k".into()),
                base_url: None,
                models: HashMap::new(),
                default_model: None,
                headers: HashMap::new(),
                timeout_ms: None,
            },
        );
        let r = router(&configs);
        // builtin model list fills the catalog
        assert_eq!(r.list_models().len(), 2);
        assert_eq!(r.default_selection().unwrap().model, "deepseek-chat");
        // model inherits the builtin protocol
        let chat = r
            .resolve(&ModelSelection {
                provider: "deepseek".into(),
                model: "deepseek-chat".into(),
                reasoning_effort: None,
            })
            .unwrap();
        assert_eq!(chat.spec.protocol, Protocol::OpenAI);
    }

    #[test]
    fn unknown_provider_needs_explicit_protocol_and_base_url() {
        let mut configs = HashMap::new();
        configs.insert(
            ProviderId::new("custom-gateway"),
            ProviderConfig {
                protocol: None,
                api_key: ApiKey::Direct("k".into()),
                base_url: None,
                models: {
                    let mut m = HashMap::new();
                    m.insert(ModelId::new("m"), model("m"));
                    m
                },
                default_model: None,
                headers: HashMap::new(),
                timeout_ms: None,
            },
        );
        let err = ModelRouter::new(&configs).unwrap_err();
        assert!(matches!(err, AiError::ConfigError(_)));
        assert!(err.to_string().contains("custom-gateway"));
        assert!(err.to_string().contains("protocol"));
    }

    #[test]
    fn builtin_opencode_zen_serves_multiple_protocols() {
        // The builtin opencode-zen directory models map across four wire
        // protocols on one shared gateway; a minimal config (key only) gets
        // them all with per-model routing.
        let mut configs = HashMap::new();
        configs.insert(
            ProviderId::new("opencode-zen"),
            ProviderConfig {
                protocol: None,
                api_key: ApiKey::Direct("k".into()),
                base_url: None,
                models: HashMap::new(),
                default_model: None,
                headers: HashMap::new(),
                timeout_ms: None,
            },
        );
        let r = router(&configs);
        let protocols = r.provider_protocols("opencode-zen").unwrap();
        assert!(protocols.contains(&Protocol::OpenAI));
        assert!(protocols.contains(&Protocol::OpenAIResponses));
        assert!(protocols.contains(&Protocol::Anthropic));
        assert!(protocols.contains(&Protocol::Gemini));

        // each model resolves to its bound protocol
        let resp = r
            .resolve(&ModelSelection {
                provider: "opencode-zen".into(),
                model: "gpt-5.6-sol".into(),
                reasoning_effort: None,
            })
            .unwrap();
        assert_eq!(resp.spec.protocol, Protocol::OpenAIResponses);
        let anthropic = r
            .resolve(&ModelSelection {
                provider: "opencode-zen".into(),
                model: "claude-opus-5".into(),
                reasoning_effort: None,
            })
            .unwrap();
        assert_eq!(anthropic.spec.protocol, Protocol::Anthropic);
        let gemini = r
            .resolve(&ModelSelection {
                provider: "opencode-zen".into(),
                model: "gemini-3.7-flash".into(),
                reasoning_effort: None,
            })
            .unwrap();
        assert_eq!(gemini.spec.protocol, Protocol::Gemini);
        let chat = r
            .resolve(&ModelSelection {
                provider: "opencode-zen".into(),
                model: "deepseek-v4-pro".into(),
                reasoning_effort: None,
            })
            .unwrap();
        assert_eq!(chat.spec.protocol, Protocol::OpenAI);
    }
}
