//! Public Pi catalog metadata. Never imports credentials, endpoints or executable code.
use std::collections::{BTreeMap, HashMap};
use std::time::Duration;

use futures_util::{StreamExt, stream};
use serde::{Deserialize, Serialize};

use crate::catalog::{builtin_model, builtin_provider};
use crate::config::{ModelConfig, ModelId, Protocol, ProviderConfig, ProviderId};
use crate::error::{AiError, Result};

pub const SOURCE: &str = "https://pi.dev";
pub const MAX_BYTES: usize = 4 * 1024 * 1024;
// Explicit mapping: regional endpoints and coding subscriptions are not interchangeable.
pub const SOURCES: &[(&str, &str, &str)] = &[
    ("anthropic", "anthropic", "anthropic-messages"),
    ("openai", "openai", "openai-responses"),
    ("gemini", "google", "google-generative-ai"),
    ("deepseek", "deepseek", "openai-completions"),
    ("kimi", "moonshotai-cn", "openai-completions"),
    ("mimo", "xiaomi", "openai-completions"),
    ("minimax", "minimax-cn", "openai-completions"),
    ("xai", "xai", "openai-responses"),
    ("openrouter", "openrouter", "openai-completions"),
    ("opencode-zen", "opencode", "openai-completions"),
    ("opencode-go", "opencode-go", "openai-completions"),
];

/// Only descriptive metadata crosses the remote/local boundary. Protocol and
/// reasoning behavior come from local adapters/config, never a downloaded URL.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CatalogModel {
    pub id: String,
    pub display_name: String,
    pub context_window: u32,
    pub max_tokens: u32,
    /// Only OpenAI/xAI's same-endpoint Responses route can be selected by the importer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub protocol: Option<Protocol>,
}

pub type CatalogModels = BTreeMap<String, BTreeMap<String, CatalogModel>>;

fn invalid(message: impl Into<String>) -> AiError {
    AiError::ConfigError(message.into())
}

fn valid_text(value: &str) -> bool {
    !value.trim().is_empty() && value.len() <= 256 && !value.chars().any(char::is_control)
}

pub fn validate(models: &CatalogModels) -> Result<()> {
    for (provider, entries) in models {
        if !SOURCES.iter().any(|(id, _, _)| id == provider) || entries.len() > 10_000 {
            return Err(invalid(format!("无效的目录提供商：{provider}")));
        }
        for (id, model) in entries {
            if id != &model.id
                || !valid_text(id)
                || !valid_text(&model.display_name)
                || model.context_window == 0
                || model.max_tokens == 0
                || model.context_window > 100_000_000
                || model.max_tokens > 100_000_000
                || model.protocol.is_some_and(|p| {
                    p != Protocol::OpenAIResponses || !matches!(provider.as_str(), "openai" | "xai")
                })
            {
                return Err(invalid(format!("无效的模型目录条目：{provider}/{id}")));
            }
        }
    }
    Ok(())
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PiModel {
    id: String,
    name: String,
    provider: String,
    api: String,
    context_window: u32,
    max_tokens: u32,
    #[serde(default)]
    compat: BTreeMap<String, serde_json::Value>,
}

/// Existing builtin models can receive descriptive updates even when Pi uses
/// another transport. New entries must use our existing provider transport.
fn parse(
    provider: &str,
    source: &str,
    api: &str,
    body: &[u8],
) -> Result<(BTreeMap<String, CatalogModel>, usize)> {
    let entries: BTreeMap<String, PiModel> = serde_json::from_slice(body)
        .map_err(|_| invalid(format!("{provider}：模型目录格式错误")))?;
    if entries.is_empty() || entries.len() > 10_000 {
        return Err(invalid(format!("{provider}：模型目录为空或过大")));
    }
    let mut result = BTreeMap::new();
    let mut skipped = 0;
    for (key, model) in entries {
        if key != model.id || model.provider != source {
            return Err(invalid(format!("{provider}：模型 ID 或提供商不匹配")));
        }
        let known = builtin_model(provider, &model.id).is_some();
        let responses = matches!(provider, "openai" | "xai") && model.api == "openai-responses";
        let transport_matches = model.api == api
            || (matches!(provider, "openai" | "xai") && model.api == "openai-completions");
        // These flags only describe optional features that we don't request.
        // Parameter remapping/adaptive thinking and unknown compatibility requirements
        // need a local adapter change before newly discovered models can be enabled.
        let compatible = model.compat.iter().all(|(key, value)| match key.as_str() {
            "supportsStore"
            | "supportsLongCacheRetention"
            | "supportsStrictTools"
            | "supportsStrictMode"
            | "supportsToolSearch"
            | "supportsAdditionalTools"
            | "sendSessionAffinityHeaders" => value.is_boolean(),
            "maxTokensField" => value.as_str() == Some("max_tokens"),
            "supportsDeveloperRole" | "supportsMidConvoSystemMessage" => {
                value.as_bool() == Some(true)
            }
            "requiresReasoningContentOnAssistantMessages" => value.as_bool() == Some(false),
            _ => false,
        });
        if !known && (!transport_matches || !compatible) {
            skipped += 1;
            continue;
        }
        result.insert(
            model.id.clone(),
            CatalogModel {
                id: model.id,
                display_name: model.name,
                context_window: model.context_window,
                max_tokens: model.max_tokens,
                protocol: (!known && responses).then_some(Protocol::OpenAIResponses),
            },
        );
    }
    validate(&BTreeMap::from([(provider.to_string(), result.clone())]))?;
    Ok((result, skipped))
}

#[derive(Default)]
pub struct Download {
    pub models: CatalogModels,
    pub warnings: Vec<String>,
    pub skipped: usize,
}

async fn download_provider(
    client: reqwest::Client,
    provider: &'static str,
    source: &'static str,
    api: &'static str,
) -> (
    &'static str,
    Result<(BTreeMap<String, CatalogModel>, usize)>,
) {
    let fetched = async {
        let mut response = client
            .get(format!("{SOURCE}/api/models/providers/{source}"))
            .send()
            .await?
            .error_for_status()?;
        if response
            .content_length()
            .is_some_and(|n| n > MAX_BYTES as u64)
        {
            return Err(invalid("模型目录响应过大"));
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await? {
            if bytes.len() + chunk.len() > MAX_BYTES {
                return Err(invalid("模型目录响应过大"));
            }
            bytes.extend_from_slice(&chunk);
        }
        parse(provider, source, api, &bytes)
    }
    .await;
    (provider, fetched)
}

/// Bounded parallel public GETs, with no access to provider API keys or headers.
pub async fn download() -> Result<Download> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .redirect(reqwest::redirect::Policy::none())
        .user_agent("LLM-Nest/catalog-sync")
        .build()?;
    let requests = SOURCES
        .iter()
        .map(|&(provider, source, api)| download_provider(client.clone(), provider, source, api))
        .collect::<Vec<_>>();
    let results = stream::iter(requests)
        .buffer_unordered(4)
        .collect::<Vec<_>>()
        .await;
    let mut result = Download::default();
    for (provider, fetched) in results {
        match fetched {
            Ok((models, skipped)) if !models.is_empty() => {
                result.models.insert(provider.into(), models);
                result.skipped += skipped;
            }
            Ok((_, skipped)) => {
                result.skipped += skipped;
                result
                    .warnings
                    .push(format!("{provider}：未找到兼容条目，保留原目录"));
            }
            Err(error) => result
                .warnings
                .push(format!("{provider}：{error}；保留原目录")),
        }
    }
    result.warnings.sort();
    if result.models.is_empty() {
        return Err(invalid(format!(
            "同步失败，目录未更改。{}",
            result.warnings.join("；")
        )));
    }
    Ok(result)
}

/// User fields > synced descriptive metadata > builtins. No providers are added.
pub fn apply(
    configs: &HashMap<ProviderId, ProviderConfig>,
    catalog: &CatalogModels,
) -> HashMap<ProviderId, ProviderConfig> {
    let mut result = configs.clone();
    for (id, cfg) in &mut result {
        let Some(models) = catalog.get(&id.0) else {
            continue;
        };
        let Some(builtin) = builtin_provider(&id.0) else {
            continue;
        };
        // A route repurposed to another protocol must not gain foreign models.
        if cfg.protocol.is_some_and(|p| p != builtin.protocol) {
            continue;
        }
        for model in models.values() {
            let mut configured = false;
            for existing in cfg.models.values_mut().filter(|m| m.model == model.id) {
                configured = true;
                existing
                    .display_name
                    .get_or_insert_with(|| model.display_name.clone());
                existing.context_window.get_or_insert(model.context_window);
                existing.max_tokens.get_or_insert(model.max_tokens);
                if existing.protocol.is_none() {
                    existing.protocol = model.protocol;
                }
            }
            if !configured && !cfg.models.contains_key(&ModelId::new(&model.id)) {
                cfg.models.insert(
                    ModelId::new(&model.id),
                    ModelConfig {
                        model: model.id.clone(),
                        display_name: Some(model.display_name.clone()),
                        context_window: Some(model.context_window),
                        max_tokens: Some(model.max_tokens),
                        reasoning: None,
                        protocol: model
                            .protocol
                            .or_else(|| builtin_model(&id.0, &model.id).and_then(|m| m.protocol)),
                    },
                );
            }
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ApiKey, ModelRouter};

    #[test]
    fn only_supported_protocols_and_compatibility_enter_cache() {
        let body = br#"{"new":{"id":"new","name":"New","provider":"openai","api":"openai-responses","contextWindow":1000,"maxTokens":100},"custom":{"id":"custom","name":"Custom","provider":"openai","api":"openai-completions","contextWindow":1000,"maxTokens":100,"compat":{"maxTokensField":"max_completion_tokens"}}}"#;
        // Optional strict-mode support doesn't require enabling it in our request.
        let mut entries: serde_json::Value = serde_json::from_slice(body).unwrap();
        entries["new"]["compat"] = serde_json::json!({"supportsStrictMode": true});
        let (models, skipped) = parse(
            "openai",
            "openai",
            "openai-responses",
            &serde_json::to_vec(&entries).unwrap(),
        )
        .unwrap();
        assert_eq!(models["new"].protocol, Some(Protocol::OpenAIResponses));
        assert_eq!(skipped, 1);
        let mut cached = BTreeMap::from([("openai".into(), models)]);
        cached
            .get_mut("openai")
            .unwrap()
            .get_mut("new")
            .unwrap()
            .protocol = Some(Protocol::Anthropic);
        assert!(validate(&cached).is_err());
    }

    #[test]
    fn validates_and_filters_foreign_models() {
        let body = br#"{"new":{"id":"new","name":"New","provider":"google","api":"google-generative-ai","contextWindow":1000,"maxTokens":100},"other":{"id":"other","name":"Other","provider":"google","api":"google-vertex","contextWindow":1000,"maxTokens":100}}"#;
        let (models, skipped) = parse("gemini", "google", "google-generative-ai", body).unwrap();
        assert_eq!(models.len(), 1);
        assert_eq!(skipped, 1);
        assert!(parse("gemini", "anthropic", "google-generative-ai", body).is_err());
        assert!(parse("gemini", "google", "google-generative-ai", b"{}").is_err());
        let bad = String::from_utf8(body.to_vec())
            .unwrap()
            .replace("\"maxTokens\":100", "\"maxTokens\":0");
        assert!(parse("gemini", "google", "google-generative-ai", bad.as_bytes()).is_err());
    }

    #[test]
    fn overlay_preserves_user_fields_aliases_and_defaults() {
        let mut cfg = ProviderConfig {
            protocol: None,
            api_key: ApiKey::Direct("test".into()),
            base_url: None,
            models: HashMap::new(),
            default_model: Some("alias".into()),
            headers: HashMap::new(),
            timeout_ms: None,
        };
        cfg.models.insert(
            ModelId::new("alias"),
            ModelConfig {
                model: "ecnu-max".into(),
                display_name: Some("Mine".into()),
                context_window: Some(777),
                max_tokens: None,
                reasoning: None,
                protocol: None,
            },
        );
        let input = HashMap::from([(ProviderId::new("chatecnu"), cfg)]);
        let model = CatalogModel {
            id: "ecnu-max".into(),
            display_name: "Remote".into(),
            context_window: 999,
            max_tokens: 88,
            protocol: None,
        };
        let overlay = BTreeMap::from([(
            "chatecnu".into(),
            BTreeMap::from([("ecnu-max".into(), model)]),
        )]);
        let merged = apply(&input, &overlay);
        let model = &merged[&ProviderId::new("chatecnu")].models[&ModelId::new("alias")];
        assert_eq!(model.display_name.as_deref(), Some("Mine"));
        assert_eq!(model.context_window, Some(777));
        assert_eq!(model.max_tokens, Some(88));
        assert!(
            input[&ProviderId::new("chatecnu")].models[&ModelId::new("alias")]
                .max_tokens
                .is_none()
        );
        assert_eq!(
            ModelRouter::new(&merged)
                .unwrap()
                .default_selection()
                .unwrap()
                .model,
            "ecnu-max"
        );
    }
}
