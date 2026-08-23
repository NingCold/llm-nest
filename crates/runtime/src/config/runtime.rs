use std::collections::HashMap;

use ai_client::config::{ProviderConfig, ProviderId};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RuntimeConfig {
    pub providers: HashMap<ProviderId, ProviderConfig>,
}

impl RuntimeConfig {
    pub fn providers(&self) -> &HashMap<ProviderId, ProviderConfig> {
        &self.providers
    }
}
