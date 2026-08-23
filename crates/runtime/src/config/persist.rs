//! Persisting `/refresh` discoveries back into the config document.
//!
//! [`persist_new_models`] edits the TOML **in place** via `toml_edit`: only
//! the provider's `models` table gains entries for previously unknown wire
//! ids; comments, formatting and every other section are untouched.

use std::path::Path;

use ai_client::WireModel;
use toml_edit::{DocumentMut, Item, Table, value};

use crate::error::{Result, RuntimeError};

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
}
