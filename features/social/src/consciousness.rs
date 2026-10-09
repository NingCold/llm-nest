use crate::{Result, SocialError};
use ai_client::router::ModelSpec;
use common::{ContentPart, Message, Role};
use social::*;
use storage::social::ContextMaterial;

pub(crate) struct PreparedContext {
    pub messages: Vec<Message>,
    pub snapshot: ContextSnapshot,
    pub expected_revision: Revision,
}

pub(crate) struct ConsciousnessService;
impl ConsciousnessService {
    pub fn prepare(
        material: ContextMaterial,
        model: &ModelSpec,
        output_tokens: u32,
    ) -> Result<PreparedContext> {
        // Until model-specific tokenizers are introduced, UTF-8 bytes plus message
        // overhead provide a conservative text estimate. Complete history stays in SQLite.
        let window = model.context_window.unwrap_or(8192);
        let input_limit = window.saturating_sub(output_tokens);
        let space = if material.conversation.kind == ConversationKind::Group {
            format!(
                "你正在群聊「{}」中。当前成员：{}。记录里的其他 AI 是独立成员，不代表你的发言。只用自己的身份回复，不替其他成员说话。",
                material.conversation.title,
                material.participant_names.join("、")
            )
        } else {
            "你正在一个长期私聊中与用户交流。".into()
        };
        let trigger = material
            .messages
            .iter()
            .find(|m| m.id == material.trigger_id)
            .expect("store supplies the visible trigger");
        let system = Message::system(format!(
            "你是 AI 联系人「{}」。用户知道你的 AI 身份。以下是你的角色设定：\n{}\n{space}本次请回应 {} 在 {} 发送的第 {} 条消息。只依据本次提供的记录回答；未提供的历史不要假装记得。",
            material.contact.actor.display_name,
            material.contact.profile.persona,
            trigger.sender_name,
            trigger.created_at.to_rfc3339(),
            trigger.sequence
        ));
        let mut used = estimate(&system);
        if output_tokens == 0 || used >= input_limit {
            return Err(SocialError::BudgetExceeded);
        }
        let quoted = trigger
            .reply_to
            .and_then(|id| material.messages.iter().find(|m| m.id == id));
        let trigger_wire = render(
            trigger,
            material.contact.actor.id,
            quoted.map(|m| m.sequence),
        );
        used += estimate(&trigger_wire);
        if used > input_limit {
            return Err(SocialError::BudgetExceeded);
        }
        let mut selected = vec![(trigger, trigger_wire)];
        if let Some(quoted) = quoted {
            let wire = render(quoted, material.contact.actor.id, None);
            used += estimate(&wire);
            if used > input_limit {
                return Err(SocialError::BudgetExceeded);
            }
            selected.push((quoted, wire));
        }
        for message in material
            .messages
            .iter()
            .rev()
            .filter(|m| m.id != trigger.id && Some(m.id) != quoted.map(|m| m.id))
        {
            let wire = render(message, material.contact.actor.id, None);
            let cost = estimate(&wire);
            if used + cost > input_limit {
                break;
            }
            used += cost;
            selected.push((message, wire));
        }
        selected.sort_by_key(|(m, _)| m.sequence);
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

fn render(message: &SocialMessage, actor_id: ActorId, reply_sequence: Option<Sequence>) -> Message {
    let role = if message.sender_id == actor_id {
        Role::Assistant
    } else {
        Role::User
    };
    let mut content = vec![ContentPart::Text(format!(
        "[{} · {} · #{}]\n",
        message.sender_name,
        message.created_at.to_rfc3339(),
        message.sequence
    ))];
    if let Some(sequence) = reply_sequence {
        content.push(ContentPart::Text(format!("（回复第 {sequence} 条消息）\n")));
    }
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
    Message::new(role, content)
}
