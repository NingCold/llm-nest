use crate::error::Result;
use std::{collections::HashMap, sync::Arc};

use common::config::{ProviderConfig, ProviderId};

use crate::{factory::ProviderFactory, traits::Provider};

#[derive(Debug, Clone)]
pub struct ProviderManager {
    
    providers:
        HashMap<ProviderId, Arc<dyn Provider>>,
}

impl ProviderManager {
    pub fn new(
        configs: &HashMap<ProviderId, ProviderConfig>
    ) -> Result<Self> {
        let mut providers =
            HashMap::with_capacity(configs.len());

        for (id, config) in configs {
            let provider = ProviderFactory::create(config.clone())?;
            providers.insert(id.clone(), provider);
        }

        Ok(Self {
            providers
        })
    }

    pub fn get(
        &self,
        id: &ProviderId,
    ) -> Option<Arc<dyn Provider>> {
        self.providers.get(id).cloned()
    }
}