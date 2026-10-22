use crate::Result;
pub(crate) use builtin_agent::consciousness::estimate;
use builtin_agent::{
    ContactInput, ContextInput, ContextTrigger, ReferenceInput, ReferenceInputPreview,
};
use storage::social::{ContextMaterial, ContextTrigger as StoredTrigger};

pub(crate) struct ConsciousnessService;
impl ConsciousnessService {
    pub fn prepare(
        material: ContextMaterial,
        model: &ai_client::ModelSpec,
        output_tokens: u32,
    ) -> Result<builtin_agent::consciousness::PreparedContext> {
        let input = ContextInput {
            required_message_ids: material.required_message_ids,
            reference: material.reference.map(|reference| ReferenceInput {
                message_id: reference.message_id,
                revision: reference.revision,
                required: reference.required,
                preview: ReferenceInputPreview {
                    conversation: reference.preview.conversation,
                    messages: reference.preview.messages,
                    summary: reference.preview.summary,
                    dependencies: reference.preview.dependencies,
                },
            }),
            summaries: material.summaries,
            memories: material.memories,
            contact: ContactInput {
                actor: material.contact.actor,
                profile: material.contact.profile,
                binding: material.contact.binding,
            },
            conversation: material.conversation,
            members: material.members,
            relationship_revision: material.relationship_revision,
            participant_names: material.participant_names,
            messages: material.messages,
            trigger: match material.trigger {
                StoredTrigger::Message(id) => ContextTrigger::Message(id),
                StoredTrigger::Contact(intent) => ContextTrigger::Contact(intent),
            },
        };
        Ok(builtin_agent::consciousness::ConsciousnessService::prepare(
            input,
            model,
            output_tokens,
        )?)
    }
}
