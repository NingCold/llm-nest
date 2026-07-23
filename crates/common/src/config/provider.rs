use std::{collections::HashMap, fmt::{Display, Formatter, Result}};

use serde::{Deserialize, Serialize};

use crate::config::{ModelId, model::ModelConfig};

#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    Hash,
    Serialize,
    Deserialize
)]
#[serde(rename_all = "lowercase")]
pub enum Protocol {
    OpenAI,
}

#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    Hash,
    Serialize,
    Deserialize
)]
#[serde(rename_all = "lowercase")]
pub struct ProviderId(pub String);

impl ProviderId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }
}

impl From<&str> for ProviderId {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

impl Display for ProviderId {
    fn fmt(
        &self,
        f: &mut Formatter<'_>
    ) -> Result {
        self.0.fmt(f)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ApiKey {
    Direct(String),
    FromEnv { env: String },
}

impl ApiKey {
    pub fn resolve(&self) -> std::result::Result<String, ConfigError> {
        match self {
            ApiKey::Direct(key) => Ok(key.clone()),
            ApiKey::FromEnv { env } => {
                std::env::var(env).map_err(|_| ConfigError::MissingEnvVar(env.clone()))
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderConfig {
    pub protocol: Protocol,
    pub api_key: ApiKey,
    pub base_url: String,
    pub models: HashMap<ModelId, ModelConfig>,
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum ConfigError {
    #[error("Environment variable `{0}` is not set")]
    MissingEnvVar(String),
}