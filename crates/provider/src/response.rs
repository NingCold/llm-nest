use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderResponse {
    pub message: common::Message,
    pub usage: Option<common::Usage>,
}