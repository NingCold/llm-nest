use super::*;
dto!(ScopedSource {
    conversation_id: String,
    message_id: String,
    revision: String
});
dto!(MaterialScope {
    conversation_id: String,
    revision: String
});
dto!(ReferenceSelection { conversation_id: String, sources: Vec<SourceRevision> });
dto!(NodeReference { message_id: String, revision: String, required: bool, conversation: NodeConversation, messages: Vec<NodeMessage>, sources: Vec<SourceRevision> });
dto!(ConsciousnessChange { id: String, conversation_id: String, message_id: Option<String>, reason: String });
dto!(ConsciousnessChanges { cursor: String, has_more: bool, changes: Vec<ConsciousnessChange> });
dto!(ValidateSources { sources: Vec<ScopedSource>, conversations: Vec<MaterialScope> });
dto!(ValidationResult { valid: bool });
dto!(NodeHistory { conversation: NodeConversation, messages: Vec<NodeMessage>, recent_boundary: Option<String> });
dto!(SummaryModel { provider_id: String, model_id: String, reasoning_effort: Option<String>, max_tokens: u32 });
dto!(MemoryItem { id: String, revision: String, kind: String, text: String, status: String, source: Option<SourceRevision>, created_at: String, updated_at: String });
dto!(SummaryItem { id: String, revision: String, status: String, text: String, from_position: String, through_position: String, sources: Vec<SourceRevision>, error: Option<String>, created_at: String, updated_at: String });
dto!(SummarySettings { revision: String, enabled: bool, binding: Option<SummaryModel> });
dto!(SummaryModelChoice { provider_id: String, model_id: String, name: String, reasoning_efforts: Vec<String> });
dto!(ConsciousnessView { memories: Vec<MemoryItem>, summaries: Vec<SummaryItem>, settings: Option<SummarySettings>, models: Vec<SummaryModelChoice> });
dto!(ConsciousnessError {
    code: String,
    message: String
});
dto!(SummaryStart { request_id: String, conversation_id: String, conversation_revision: String, sources: Vec<SourceRevision>, settings_revision: String, material_scope: MaterialScope });
dto!(SummaryConfirmation {
    id: String,
    confirmation_id: String
});
dto!(SummaryFinish { confirmation_id: String, status: String, usage: Option<NodeUsage>, error_code: Option<String> });
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ConsciousnessOperation {
    Memories,
    CreateMemory {
        kind: String,
        text: String,
        source: Option<SourceRevision>,
    },
    EditMemory {
        id: String,
        revision: String,
        kind: String,
        text: String,
        source: Option<SourceRevision>,
    },
    ForgetMemory {
        id: String,
        revision: String,
    },
    Summaries,
    ConfigureSummaries {
        revision: String,
        enabled: bool,
        binding: Option<SummaryModel>,
    },
    GenerateSummary {
        retry_id: Option<String>,
    },
}
