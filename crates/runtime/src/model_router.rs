use std::sync::Arc;

use llm::ModelSelection;
use provider::traits::Provider;

use crate::error::{Result, RuntimeError};
use crate::provider_manager::ProviderManager;

#[derive(Debug)]
pub struct ModelRouter {
    manager: Arc<ProviderManager>,
}

impl ModelRouter {
    pub fn new(manager: Arc<ProviderManager>) -> Self {
        Self { manager }
    }

    pub async fn route(&self, selection: &ModelSelection) -> Result<Arc<dyn Provider>> {
        let pid = common::config::ProviderId::new(&selection.provider);
        self.manager
            .get(&pid)
            .await
            .ok_or_else(|| RuntimeError::ProviderNotFound(pid))
    }
}