use std::{
    collections::HashMap,
    fmt::{Display, Formatter, Result},
};

use serde::{Deserialize, Serialize};

use super::{ModelConfig, ModelId};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Protocol {
    #[default]
    OpenAIChat,
    #[serde(rename = "openai")]
    OpenAI,
    #[serde(rename = "openai_responses")]
    OpenAIResponses,
    Anthropic,
    Gemini,
    Ollama,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
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
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
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
    /// Wire protocol of this route. Absent when the provider id matches a
    /// builtin directory entry (falls back to its protocol); required
    /// otherwise (fails at startup naming the provider).
    #[serde(default)]
    pub protocol: Option<Protocol>,
    pub api_key: ApiKey,
    /// API endpoint. Absent when the provider id matches a builtin directory
    /// entry (falls back to its base_url); required otherwise.
    #[serde(default)]
    pub base_url: Option<String>,
    /// Models of this provider. Absent/empty falls back to the builtin model
    /// directory when the provider id matches one.
    #[serde(default)]
    pub models: HashMap<ModelId, ModelConfig>,
    /// Default model for this provider (a configured model key or wire name);
    /// falls back to the builtin default, then to the first effective model.
    #[serde(default)]
    pub default_model: Option<String>,
    /// Extra headers attached to every request of this provider route.
    /// Validated at provider construction; invalid names or values fail
    /// startup naming the offending key.
    #[serde(default)]
    pub headers: HashMap<String, String>,
    /// Per-provider request timeout in milliseconds, streaming reads
    /// included. `0` is refused at construction.
    #[serde(default)]
    pub timeout_ms: Option<u64>,
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum ConfigError {
    #[error("Environment variable `{0}` is not set")]
    MissingEnvVar(String),
}
