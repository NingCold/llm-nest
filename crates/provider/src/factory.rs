use std::{collections::HashMap, sync::Arc};

use common::config::{
    ModelConfig, ModelId, Protocol, ProviderConfig,
};

use crate::{
    error::{ProviderError, Result},
    protocols::openai::provider::OpenAIProvider,
    traits::Provider,
};

#[derive(Debug, Clone)]
pub struct ResolvedProviderConfig {
    pub protocol: Protocol,
    pub api_key: String,
    pub base_url: String,
    pub models: HashMap<ModelId, ModelConfig>,
}

pub struct ProviderFactory;

impl ProviderFactory {
    pub fn create(
        config: ProviderConfig,
    ) -> Result<Arc<dyn Provider>> {
        let resolved = ResolvedProviderConfig {
            api_key: config
                .api_key
                .resolve()
                .map_err(|e| ProviderError::ConfigError(e.to_string()))?,
            protocol: config.protocol,
            base_url: config.base_url,
            models: config.models,
        };
        match resolved.protocol {
            Protocol::OpenAI => Ok(Arc::new(OpenAIProvider::new(resolved)?)),
        }
    }
}