use std::collections::HashMap;

use common::config::{ProviderConfig, ProviderId};
use serde::{Deserialize, Serialize};


#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeConfig {
    pub providers: HashMap<ProviderId, ProviderConfig>,
}

impl RuntimeConfig {
    pub fn providers(
        &self,
    ) -> &HashMap<ProviderId, ProviderConfig> {
        &self.providers
    }
}