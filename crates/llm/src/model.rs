use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelSelection {
    pub provider: String,
    pub model: String,
}