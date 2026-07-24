use serde::{Deserialize, Serialize};

use crate::GenerationParameters;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderRequest {
    pub model: String,
    pub messages: Vec<common::Message>,
    pub parameters: GenerationParameters,
}