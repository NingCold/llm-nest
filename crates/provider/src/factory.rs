use std::sync::Arc;

use crate::error::Result;
use common::config::{Protocol, ProviderConfig};
use crate::{protocols::openai::provider::OpenAIProvider, traits::Provider};

pub struct ProviderFactory;

impl ProviderFactory {
    pub fn create(
        config: ProviderConfig,
    ) -> Result<Arc<dyn Provider>> {
        match config.protocol {
            Protocol::OpenAI => Ok(Arc::new(
                OpenAIProvider::new(config)?
            ))
        }
    }
}