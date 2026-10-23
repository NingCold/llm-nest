//! Remote profile management deliberately excludes endpoints and credentials.
use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentProfile {
    pub revision: String,
    pub persona: String,
    pub provider_id: String,
    pub model_id: String,
    pub reasoning_effort: Option<String>,
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
    pub auto_daily_tokens: Option<u64>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentModelChoice {
    pub provider_id: String,
    pub model_id: String,
    pub name: String,
    pub reasoning_efforts: Vec<String>,
    pub image_input: String,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentManagementView {
    pub profile: AgentProfile,
    pub models: Vec<AgentModelChoice>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum AgentManagementOperation {
    View,
    SaveProfile {
        expected_revision: String,
        persona: String,
        provider_id: String,
        model_id: String,
        reasoning_effort: Option<String>,
        temperature: Option<f32>,
        max_tokens: Option<u32>,
        auto_daily_tokens: Option<u64>,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn profile_command_roundtrips_camel_case_without_node_secrets() {
        let wire = serde_json::json!({"type":"saveProfile","expectedRevision":"1","persona":"role","providerId":"provider","modelId":"model","reasoningEffort":"high","temperature":0.5,"maxTokens":128,"autoDailyTokens":1024});
        let operation: AgentManagementOperation = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(serde_json::to_value(operation).unwrap(), wire);
        let old_hello = serde_json::json!({"type":"hello","protocolVersion":4,"nodeId":"node","aiId":"ai","bindingVersion":"1"});
        let hello: crate::node::ClientFrame = serde_json::from_value(old_hello).unwrap();
        assert!(
            matches!(hello, crate::node::ClientFrame::Hello { capabilities, .. } if capabilities.is_empty())
        );
    }
}
