//! Persisting `/refresh` discoveries and web-driven provider edits back into
//! the config document.
//!
//! All edits go through `toml_edit` **in place**: only the addressed fields
//! change; comments, formatting and every other section are untouched.

use std::path::Path;

use ai_client::WireModel;
use toml_edit::{DocumentMut, Item, Table, value};

use crate::error::{Result, RuntimeError};

/// A provider the web settings wrote: `protocol`/`base_url`/`api_key`/`models`
/// are the only fields the form manages; anything else in the document
/// (default_model, headers, timeout_ms, …) is preserved untouched, and a
/// `None` field keeps the existing value (create: stays absent → builtin
/// fallback).
pub struct ProviderDraft {
    /// `[providers.<id>]` table key (also the route id).
    pub id: String,
    /// Wire protocol of the route (`openai`, `openai_responses`, …);
    /// `None` keeps the existing field (builtin default on create).
    pub protocol: Option<String>,
    /// Endpoint override; `None` keeps/omits the field (builtin fallback).
    pub base_url: Option<String>,
    /// Direct key to store; `None` keeps the existing field (create: absent).
    pub api_key: Option<String>,
    /// Full intended model list (config key = wire id). `Some` replaces the
    /// whole `models` table; `None` leaves it untouched (builtin fallback).
    pub models: Option<Vec<ProviderModelDraft>>,
}

/// One model row of the provider form.
pub struct ProviderModelDraft {
    /// Config key — for this form always equal to the wire id.
    pub id: String,
    /// Wire model name sent to the API.
    pub wire: String,
    pub display_name: Option<String>,
}

/// Upsert `[providers.<id>]` (protocol / base_url / api_key) and optionally
/// replace its `models` table in the document at `path`. Creates the
/// `[providers]` table and the provider table when absent. Fields the form
/// does not manage are preserved. Fails when the document does not parse.
pub fn persist_provider(path: &Path, draft: &ProviderDraft) -> Result<()> {
    let text = std::fs::read_to_string(path)?;
    let mut doc: DocumentMut = text.parse().map_err(|e| {
        RuntimeError::ConfigError(format!("failed to parse config for persistence: {e}"))
    })?;
    let providers = match doc.get_mut("providers").and_then(Item::as_table_mut) {
        Some(table) => table,
        None => {
            doc.insert("providers", Item::Table(Table::new()));
            doc.get_mut("providers")
                .and_then(Item::as_table_mut)
                .ok_or_else(|| RuntimeError::ConfigError("no [providers] table in config".into()))?
        }
    };
    let provider_tbl = providers
        .entry(&draft.id)
        .or_insert(Item::Table(Table::new()))
        .as_table_mut()
        .ok_or_else(|| {
            RuntimeError::ConfigError(format!(
                "provider '{}' is an inline table and cannot be edited in place",
                draft.id
            ))
        })?;

    if let Some(protocol) = &draft.protocol {
        provider_tbl["protocol"] = value(protocol.clone());
    }
    if let Some(base_url) = &draft.base_url {
        provider_tbl["base_url"] = value(base_url.clone());
    }
    if let Some(key) = &draft.api_key {
        provider_tbl["api_key"] = value(key.clone());
    }
    if let Some(models) = &draft.models {
        let mut models_tbl = Table::new();
        for m in models {
            let mut entry = Table::new();
            entry["model"] = value(m.wire.clone());
            if let Some(name) = &m.display_name {
                entry["display_name"] = value(name.clone());
            }
            models_tbl.insert(&m.id, Item::Table(entry));
        }
        provider_tbl.insert("models", Item::Table(models_tbl));
    }

    std::fs::write(path, doc.to_string())?;
    Ok(())
}

/// Remove the `[providers.<id>]` table from the document at `path`. A no-op
/// when the provider is not present.
pub fn persist_remove_provider(path: &Path, id: &str) -> Result<()> {
    let text = std::fs::read_to_string(path)?;
    let mut doc: DocumentMut = text.parse().map_err(|e| {
        RuntimeError::ConfigError(format!("failed to parse config for persistence: {e}"))
    })?;
    let Some(providers) = doc.get_mut("providers").and_then(Item::as_table_mut) else {
        return Ok(());
    };
    if providers.contains_key(id) {
        providers.remove(id);
        std::fs::write(path, doc.to_string())?;
    }
    Ok(())
}

/// Append `models` into `[providers.<provider>]`'s `models` table of the
/// document at `path`. A no-op when `models` is empty. Fails when the
/// document has no `[providers]`/`[providers.<provider>]` table or when
/// `models` is an inline table (cannot be extended in place).
pub fn persist_new_models(path: &Path, provider: &str, models: &[WireModel]) -> Result<()> {
    if models.is_empty() {
        return Ok(());
    }
    let text = std::fs::read_to_string(path)?;
    let mut doc: DocumentMut = text.parse().map_err(|e| {
        RuntimeError::ConfigError(format!("failed to parse config for persistence: {e}"))
    })?;
    let providers = doc
        .get_mut("providers")
        .and_then(Item::as_table_mut)
        .ok_or_else(|| RuntimeError::ConfigError("no [providers] table in config".into()))?;
    let provider_tbl = providers
        .get_mut(provider)
        .and_then(Item::as_table_mut)
        .ok_or_else(|| {
            RuntimeError::ConfigError(format!(
                "provider '{provider}' has no [providers.{provider}] table in the config \
                 document; add it first"
            ))
        })?;
    let models_tbl = provider_tbl
        .entry("models")
        .or_insert(Item::Table(Table::new()))
        .as_table_mut()
        .ok_or_else(|| {
            RuntimeError::ConfigError(format!(
                "provider '{provider}': models is an inline table and cannot be extended in \
                 place; use section form [providers.{provider}.models.<key>]"
            ))
        })?;

    for m in models {
        if models_tbl.contains_key(&m.id) {
            continue;
        }
        let mut entry = Table::new();
        entry["model"] = value(m.id.clone());
        if let Some(name) = &m.display_name {
            entry["display_name"] = value(name.clone());
        }
        if let Some(ctx) = m.context_window {
            entry["context_window"] = value(ctx as i64);
        }
        if let Some(max) = m.max_tokens {
            entry["max_tokens"] = value(max as i64);
        }
        models_tbl.insert(&m.id, Item::Table(entry));
    }

    std::fs::write(path, doc.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn appends_models_and_keeps_comments() {
        let dir = std::env::temp_dir().join(format!("llmn-persist-append-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("llmn.toml");
        std::fs::write(
            &path,
            r#"# my config
[providers.chatecnu]
protocol = "openai"
api_key = "sk-keep-me"
base_url = "https://chat.ecnu.edu.cn/open/api/v1/"

[providers.chatecnu.models.ecnu-max]
model = "ecnu-max"
"#,
        )
        .unwrap();

        let models = vec![
            WireModel {
                id: "ecnu-image".into(),
                display_name: Some("ECNU Image".into()),
                context_window: None,
                max_tokens: Some(2048),
            },
            WireModel {
                id: "deepseek-ai/DeepSeek-V4-Flash".into(),
                display_name: None,
                context_window: Some(1000 * 1024),
                max_tokens: None,
            },
        ];
        persist_new_models(&path, "chatecnu", &models).unwrap();

        let text = std::fs::read_to_string(&path).unwrap();
        // comments and existing content preserved
        assert!(text.contains("# my config"));
        assert!(text.contains("api_key = \"sk-keep-me\""));
        assert!(text.contains("[providers.chatecnu.models.ecnu-max]"));
        // new entries appended
        assert!(text.contains("[providers.chatecnu.models.ecnu-image]"));
        assert!(text.contains("model = \"ecnu-image\""));
        assert!(text.contains("max_tokens = 2048"));
        // slash-containing wire id is quoted as a key
        assert!(text.contains("\"deepseek-ai/DeepSeek-V4-Flash\""));

        // the document still parses as a runtime config
        let parsed: crate::config::RuntimeConfig = toml::from_str(&text).unwrap();
        let cfg = parsed
            .providers()
            .get(&ai_client::config::ProviderId::new("chatecnu"))
            .unwrap();
        assert!(cfg.models.values().any(|m| m.model == "ecnu-image"));
        assert!(
            cfg.models
                .values()
                .any(|m| m.model == "deepseek-ai/DeepSeek-V4-Flash")
        );

        // second call is a no-op
        persist_new_models(&path, "chatecnu", &models).unwrap();
        let again = std::fs::read_to_string(&path).unwrap();
        assert_eq!(text, again);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_provider_table_fails() {
        let dir = std::env::temp_dir().join(format!("llmn-persist-missing-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("llmn.toml");
        std::fs::write(&path, "[providers.other]\napi_key = \"k\"\n").unwrap();

        let err = persist_new_models(
            &path,
            "ghost",
            &[WireModel {
                id: "m".into(),
                display_name: None,
                context_window: None,
                max_tokens: None,
            }],
        )
        .unwrap_err();
        assert!(err.to_string().contains("ghost"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn persists_new_custom_provider_and_keeps_rest() {
        let dir =
            std::env::temp_dir().join(format!("llmn-persist-provider-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("llmn.toml");
        std::fs::write(
            &path,
            r#"# my config
[providers.chatecnu]
protocol = "openai"
api_key = "sk-keep-me"

[providers.chatecnu.models.ecnu-max]
model = "ecnu-max"
"#,
        )
        .unwrap();

        persist_provider(
            &path,
            &ProviderDraft {
                id: "my-gateway".into(),
                protocol: Some("openai".into()),
                base_url: Some("https://gw.example/v1".into()),
                api_key: Some("sk-custom".into()),
                models: Some(vec![
                    ProviderModelDraft {
                        id: "gpt-x".into(),
                        wire: "gpt-x".into(),
                        display_name: Some("GPT-X".into()),
                    },
                    ProviderModelDraft {
                        id: "deepseek-ai/DeepSeek-V4-Flash".into(),
                        wire: "deepseek-ai/DeepSeek-V4-Flash".into(),
                        display_name: None,
                    },
                ]),
            },
        )
        .unwrap();

        let text = std::fs::read_to_string(&path).unwrap();
        // existing provider + comments untouched
        assert!(text.contains("# my config"));
        assert!(text.contains("api_key = \"sk-keep-me\""));
        // new provider section with all fields
        assert!(text.contains("[providers.my-gateway]"));
        assert!(text.contains("protocol = \"openai\""));
        assert!(text.contains("base_url = \"https://gw.example/v1\""));
        assert!(text.contains("api_key = \"sk-custom\""));
        assert!(text.contains("[providers.my-gateway.models.gpt-x]"));
        assert!(text.contains("display_name = \"GPT-X\""));
        // slash id quoted
        assert!(text.contains("\"deepseek-ai/DeepSeek-V4-Flash\""));

        // parses as a runtime config with the new provider
        let parsed: crate::config::RuntimeConfig = toml::from_str(&text).unwrap();
        let cfg = parsed
            .providers()
            .get(&ai_client::config::ProviderId::new("my-gateway"))
            .unwrap();
        assert_eq!(cfg.models.len(), 2);
        assert!(cfg.models.values().any(|m| m.model == "gpt-x"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn updates_provider_in_place_preserving_other_fields() {
        let dir = std::env::temp_dir().join(format!("llmn-persist-update-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("llmn.toml");
        std::fs::write(
            &path,
            r#"[providers.gw]
protocol = "anthropic"
base_url = "https://old.example"
timeout_ms = 30000
"#,
        )
        .unwrap();

        // blank key + blank base_url + no models: only protocol changes,
        // timeout_ms/base_url stay.
        persist_provider(
            &path,
            &ProviderDraft {
                id: "gw".into(),
                protocol: Some("openai".into()),
                base_url: None,
                api_key: None,
                models: None,
            },
        )
        .unwrap();

        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("protocol = \"openai\""));
        assert!(text.contains("base_url = \"https://old.example\""));
        assert!(text.contains("timeout_ms = 30000"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn removes_provider_and_is_noop_when_absent() {
        let dir = std::env::temp_dir().join(format!("llmn-persist-remove-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("llmn.toml");
        std::fs::write(
            &path,
            r#"[providers.keep]
protocol = "openai"
api_key = "k"

[providers.gone]
protocol = "anthropic"
"#,
        )
        .unwrap();

        persist_remove_provider(&path, "gone").unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(!text.contains("providers.gone"));
        assert!(text.contains("providers.keep"));

        // absent id: no-op, document unchanged
        let before = std::fs::read_to_string(&path).unwrap();
        persist_remove_provider(&path, "ghost").unwrap();
        assert_eq!(before, std::fs::read_to_string(&path).unwrap());

        let _ = std::fs::remove_dir_all(&dir);
    }
}
