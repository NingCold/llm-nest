use std::{fs, path::Path};

use crate::{config::runtime::RuntimeConfig, error::{Result, RuntimeError}};

pub struct ConfigLoader;

impl ConfigLoader {
    pub fn load(
        path: impl AsRef<Path>,
    ) -> Result<RuntimeConfig> {
        let text = fs::read_to_string(path)?;
        let config = toml::from_str(&text)?;
        Self::validate(&config)?;
        Ok(config)
    }

    pub fn validate(
        config: &RuntimeConfig,
    ) -> Result<()> {
        if config.providers.is_empty() {
            return Err(RuntimeError::ConfigError(
                "at least one provider must be configured".into(),
            ))
        }

        for (id, provider) in &config.providers {

            if provider.models.is_empty() {
                return Err(
                    RuntimeError::ConfigError(
                        format!(
                            "provider '{}' has no models",
                            id
                        ),
                    ),
                );
            }

            if provider.base_url.trim().is_empty() {
                return Err(
                    RuntimeError::ConfigError(
                        format!(
                            "provider '{}' has empty base_url",
                            id
                        ),
                    ),
                );
            }

            for (model_id, model) in &provider.models {
                if model.model.trim().is_empty() {
                    return Err(
                        RuntimeError::ConfigError(
                            format!(
                                "model '{}' of provider '{}' has empty model name",
                                model_id,
                                id
                            ),
                        ),
                    );
                }
            }
        }
        Ok(())
    }
}