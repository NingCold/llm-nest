use serde::{Deserialize, Serialize};

use crate::GenerationOptions;
use crate::ModelSelection;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompletionRequest {
    pub model: ModelSelection,
    pub messages: Vec<common::Message>,
    pub options: GenerationOptions,
}