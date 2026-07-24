use std::collections::HashMap;
use std::sync::Arc;

use common::config::{ProviderConfig, ProviderId};
use provider::factory::ProviderFactory;
use provider::traits::Provider;
use tokio::sync::RwLock;

use crate::error::Result;

#[derive(Debug)]
pub struct ProviderManager {
    providers: RwLock<HashMap<ProviderId, Arc<dyn Provider>>>,
}

impl ProviderManager {
    pub fn new(configs: &HashMap<ProviderId, ProviderConfig>) -> Result<Self> {
        let mut providers = HashMap::with_capacity(configs.len());
        for (id, config) in configs {
            let provider = ProviderFactory::create(config.clone())?;
            providers.insert(id.clone(), provider);
        }
        Ok(Self {
            providers: RwLock::new(providers),
        })
    }

    pub async fn get(&self, id: &ProviderId) -> Option<Arc<dyn Provider>> {
        self.providers.read().await.get(id).cloned()
    }

    pub async fn register(&self, id: ProviderId, provider: Arc<dyn Provider>) {
        self.providers.write().await.insert(id, provider);
    }

    pub async fn remove(&self, id: &ProviderId) -> Option<Arc<dyn Provider>> {
        self.providers.write().await.remove(id)
    }
}