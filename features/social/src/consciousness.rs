use crate::{Result, SocialError};
use ai_client::router::ModelSpec;
use common::{ContentPart, Message, Role};
use social::*;
use storage::social::ContextMaterial;

pub(crate) struct PreparedContext {
    pub messages: Vec<Message>,
    pub snapshot: ContextSnapshot,
    pub expected_revision: Revision,
    pub output_tokens: u32,
}

pub(crate) struct ConsciousnessService;
impl ConsciousnessService {
    pub fn prepare(material: ContextMaterial, model: &ModelSpec) -> Result<PreparedContext> {
        // Until model-specific tokenizers are introduced, UTF-8 bytes plus message
        // overhead provide a conservative text estimate. Complete history stays in SQLite.
        let window = model.context_window.unwrap_or(8192);
        let output_tokens = model.max_tokens.unwrap_or(1024).min(1024).min(window / 4);
        let input_limit = window.saturating_sub(output_tokens);
        let system = Message::system(format!(
            "你是 AI 联系人「{}」。用户知道你的 AI 身份。以下是你的角色设定：\n{}\n你正在一个长期私聊中与用户交流。只依据本次提供的记录回答；未提供的历史不要假装记得。",
            material.contact.actor.display_name, material.contact.profile.persona
        ));
        let mut used = estimate(&system);
        if output_tokens == 0 || used >= input_limit {
            return Err(SocialError::BudgetExceeded);
        }
        let mut selected = Vec::new();
        for message in material.messages.iter().rev() {
            let role = if message.sender_id == material.contact.actor.id {
                Role::Assistant
            } else {
                Role::User
            };
            let mut content = vec![ContentPart::Text(format!(
                "[{} · {}]\n",
                message.sender_name,
                message.created_at.to_rfc3339()
            ))];
            content.extend(message.body.0.iter().map(|part| match part {
                SocialContent::Text { text } => ContentPart::Text(text.clone()),
                SocialContent::Image { mime, data } => ContentPart::Image {
                    mime: mime.clone(),
                    data: data.clone(),
                },
                SocialContent::File { mime, data } => ContentPart::File {
                    mime: mime.clone(),
                    data: data.clone(),
                },
            }));
            let wire = Message::new(role, content);
            let cost = estimate(&wire);
            if used + cost > input_limit {
                break;
            }
            used += cost;
            selected.push((message, wire));
        }
        if selected.is_empty() {
            return Err(SocialError::BudgetExceeded);
        }
        selected.reverse();
        let sources = selected
            .iter()
            .map(|(m, _)| SourceRef::Message {
                conversation_id: m.conversation_id,
                message_id: m.id,
                revision: m.revision,
            })
            .collect();
        let mut messages = vec![system];
        messages.extend(selected.into_iter().map(|(_, wire)| wire));
        let binding = material
            .contact
            .binding
            .as_ref()
            .ok_or_else(|| SocialError::ModelUnavailable("该 AI 尚未配置模型".into()))?;
        let snapshot = ContextSnapshot {
            scope: ContextScope {
                actor_id: material.contact.actor.id,
                conversation_id: material.conversation.id,
            },
            profile_revision: material.contact.profile.revision,
            binding_revision: binding.revision,
            relationship_revision: material.relationship_revision,
            budget: ContextBudget {
                input_token_limit: Counter::new(input_limit.into()).expect("u32 token limit"),
                reserved_output_tokens: Counter::new(output_tokens.into())
                    .expect("u32 output limit"),
            },
            participation_epoch: material.conversation.participation_epoch,
            dependencies: material
                .members
                .iter()
                .map(|m| VisibilityDependency {
                    conversation_id: m.conversation_id,
                    actor_id: m.actor_id,
                    membership_revision: m.revision,
                    context_epoch: material.conversation.context_epoch,
                })
                .collect(),
            sources,
        };
        Ok(PreparedContext {
            messages,
            snapshot,
            expected_revision: material.conversation.revision,
            output_tokens,
        })
    }
}

fn estimate(message: &Message) -> u32 {
    message
        .content
        .iter()
        .map(|part| match part {
            ContentPart::Text(text) => text.len() as u32,
            ContentPart::Image { data, .. } | ContentPart::File { data, .. } => data.len() as u32,
            _ => unreachable!("consciousness creates social text/attachment parts only"),
        })
        .sum::<u32>()
        + 16
}
