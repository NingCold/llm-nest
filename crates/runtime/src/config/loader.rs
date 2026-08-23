use std::{fs, path::Path};

use ai_client::catalog::builtin_provider;

use crate::{
    config::runtime::RuntimeConfig,
    error::{Result, RuntimeError},
};

pub struct ConfigLoader;

impl ConfigLoader {
    pub fn load(path: impl AsRef<Path>) -> Result<RuntimeConfig> {
        let text = fs::read_to_string(path)?;
        let config = toml::from_str(&text)?;
        Self::validate(&config)?;
        Ok(config)
    }

    pub fn validate(config: &RuntimeConfig) -> Result<()> {
        if config.providers.is_empty() {
            return Err(RuntimeError::ConfigError(
                "at least one provider must be configured".into(),
            ));
        }

        for (id, provider) in &config.providers {
            // models/base_url/protocol may be omitted when the provider id
            // matches the builtin directory; otherwise they are required.
            let builtin = builtin_provider(&id.0);
            if provider.models.is_empty() && builtin.is_none_or(|b| b.models.is_empty()) {
                return Err(RuntimeError::ConfigError(format!(
                    "provider '{}' has no models (configured or builtin)",
                    id
                )));
            }

            let base_url_ok = provider
                .base_url
                .as_deref()
                .is_some_and(|u| !u.trim().is_empty());
            if !base_url_ok && builtin.is_none() {
                return Err(RuntimeError::ConfigError(format!(
                    "provider '{}' has empty base_url and no builtin directory entry",
                    id
                )));
            }

            for (model_id, model) in &provider.models {
                if model.model.trim().is_empty() {
                    return Err(RuntimeError::ConfigError(format!(
                        "model '{}' of provider '{}' has empty model name",
                        model_id, id
                    )));
                }
            }
        }
        Ok(())
    }
}
