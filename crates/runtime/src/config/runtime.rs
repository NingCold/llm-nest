use std::collections::HashMap;

use ai_client::config::{ProviderConfig, ProviderId};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RuntimeConfig {
    #[serde(default)]
    pub providers: HashMap<ProviderId, ProviderConfig>,
}

impl RuntimeConfig {
    pub fn providers(&self) -> &HashMap<ProviderId, ProviderConfig> {
        &self.providers
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_and_comment_only_configs_are_valid_first_run_states() {
        for text in ["", "# LLM Nest desktop settings\n", "[providers]\n"] {
            let config: RuntimeConfig = toml::from_str(text).unwrap();
            assert!(config.providers.is_empty());
            crate::config::loader::ConfigLoader::validate(&config).unwrap();
        }
        assert!(toml::from_str::<RuntimeConfig>("providers = 42").is_err());
        assert!(toml::from_str::<RuntimeConfig>("[providers").is_err());
    }
}
