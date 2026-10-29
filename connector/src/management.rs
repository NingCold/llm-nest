//! Local Agent management contracts, independent of model implementations.

use serde::{Deserialize, Serialize};

pub const AGENT_CATALOG_PROTOCOL_VERSION: u32 = 1;

/// Builtin choices returned by `llmn-agent catalog`, without node configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentCatalog {
    pub protocol_version: u32,
    pub agent_version: String,
    pub providers: Vec<AgentCatalogProvider>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentCatalogProvider {
    pub id: String,
    pub name: String,
    pub models: Vec<AgentCatalogModel>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentCatalogModel {
    pub id: String,
    pub name: String,
    pub context_window: Option<u32>,
    pub reasoning_efforts: Vec<String>,
}
