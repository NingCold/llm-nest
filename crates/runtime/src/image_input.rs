//! Updating one model's declared image support without rewriting its provider.
use ai_client::ModelSelection;
use serde::Deserialize;

use crate::{
    error::{Result, RuntimeError},
    runtime::Runtime,
};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ImageInputUpdate {
    pub provider: String,
    pub model: String,
    /// null removes the override and follows the directory again.
    pub image_input: Option<bool>,
}

impl Runtime {
    pub async fn set_model_image_input(&self, input: ImageInputUpdate) -> Result<()> {
        let _update = self.config_updates.lock().await;
        let path = self
            .config_path
            .as_ref()
            .ok_or_else(|| RuntimeError::ConfigError("没有可保存的配置文件".into()))?;
        let text = std::fs::read_to_string(path)?;
        let config = toml::from_str(&text)?;
        let candidate = self.catalog_client(&config).await?;
        let resolved = candidate
            .resolve(&ModelSelection {
                provider: input.provider.clone(),
                model: input.model,
                reasoning_effort: None,
            })
            .await?
            .ok_or_else(|| RuntimeError::ConfigError("尚未配置模型目录".into()))?;
        let next = crate::config::persist::render_image_input(
            &text,
            &input.provider,
            &resolved.spec.id,
            &resolved.spec.wire,
            input.image_input,
        )?;
        let config = toml::from_str(&next)?;
        crate::config::ConfigLoader::validate(&config)?;
        let candidate = self.catalog_client(&config).await?;
        crate::config::persist::atomic_write(path, &next)?;
        self.llm.install(candidate).await;
        Ok(())
    }
}
