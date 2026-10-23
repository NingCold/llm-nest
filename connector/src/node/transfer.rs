//! Credential-free, versioned AI materials transfer format.
use super::{MemoryItem, SummaryItem, SummaryModel, SummarySettings};
use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AiTransfer {
    pub version: u32,
    pub package_id: String,
    /// "local" or "node".
    pub origin: String,
    pub ai_id: String,
    pub profile: TransferProfile,
    pub windows: Vec<TransferWindow>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TransferProfile {
    pub persona: String,
    pub provider_id: String,
    pub model_id: String,
    pub reasoning_effort: Option<String>,
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
    pub auto_daily_tokens: Option<u64>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TransferWindow {
    pub conversation_id: String,
    pub memories: Vec<MemoryItem>,
    pub settings: SummarySettings,
    pub summaries: Vec<TransferSummary>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TransferSummary {
    pub item: SummaryItem,
    pub binding: SummaryModel,
    pub requested_by: String,
}
