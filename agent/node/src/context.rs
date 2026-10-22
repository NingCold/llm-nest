use crate::store::Profile;
use ai_client::{ChatRequest, ModelRouter, image_input::ImageSupport};
use anyhow::{Result, bail};
use builtin_agent::{
    ContactInput, ContextInput, ContextTrigger, consciousness::ConsciousnessService, generation,
};
use connector::node::{NodeClient, NodeMessage, TaskMaterials};
use social::*;

const IMAGE_BYTES: u64 = 4 * 1024 * 1024;
const REQUEST_IMAGE_BYTES: u64 = 16 * 1024 * 1024;
fn image_cost(message: &NodeMessage) -> Result<(usize, u64)> {
    let mut count = 0;
    let mut bytes = 0;
    for attachment in message.attachments.iter().filter(|a| a.kind == "image") {
        anyhow::ensure!(
            attachment.byte_length > 0
                && attachment.byte_length <= IMAGE_BYTES
                && matches!(
                    attachment.mime.as_str(),
                    "image/png" | "image/jpeg" | "image/webp"
                ),
            "invalid_image_metadata"
        );
        count += 1;
        bytes += attachment.byte_length;
    }
    Ok((count, bytes))
}
/// Keep whole messages: required trigger first, then newest history within image limits.
fn select_image_history(messages: Vec<NodeMessage>, trigger: &str) -> Result<Vec<NodeMessage>> {
    let required = messages
        .iter()
        .position(|m| m.id == trigger)
        .ok_or_else(|| anyhow::anyhow!("missing trigger message"))?;
    let (mut count, mut bytes) = image_cost(&messages[required])?;
    anyhow::ensure!(
        count <= 16 && bytes <= REQUEST_IMAGE_BYTES,
        "required_images_exceed_budget"
    );
    let mut selected = vec![false; messages.len()];
    selected[required] = true;
    for index in (0..messages.len()).rev().filter(|index| *index != required) {
        let (more_count, more_bytes) = image_cost(&messages[index])?;
        if count + more_count <= 16 && bytes + more_bytes <= REQUEST_IMAGE_BYTES {
            selected[index] = true;
            count += more_count;
            bytes += more_bytes;
        }
    }
    Ok(messages
        .into_iter()
        .zip(selected)
        .filter_map(|(message, keep)| keep.then_some(message))
        .collect())
}
fn validate_image_download(bytes: &[u8], expected: u64) -> Result<()> {
    anyhow::ensure!(
        bytes.len() as u64 == expected && bytes.len() as u64 <= IMAGE_BYTES,
        "image_download_length_mismatch"
    );
    Ok(())
}

pub async fn prepare(
    client: &NodeClient,
    router: &ModelRouter,
    profile: &Profile,
    mut material: TaskMaterials,
) -> Result<(ChatRequest, u64)> {
    let resolved = router.resolve(&profile.selection)?;
    let options = generation::options(
        &generation::Parameters {
            temperature: profile.temperature,
            max_tokens: profile.max_tokens,
        },
        &resolved,
    )?;
    material.messages = select_image_history(material.messages, &material.task.trigger_message_id)?;
    let has_images = material
        .messages
        .iter()
        .any(|m| m.attachments.iter().any(|a| a.kind == "image"));
    if has_images && resolved.spec.image_input.support == ImageSupport::Unsupported {
        bail!("image_input_unsupported");
    }
    let ai: ActorId = material.ai.id.parse()?;
    let conversation_id: ConversationId = material.task.conversation_id.parse()?;
    let trigger = material.task.trigger_message_id.parse()?;
    if !material
        .messages
        .iter()
        .any(|m| m.id == material.task.trigger_message_id)
    {
        bail!("missing trigger message");
    }
    let now = chrono::Utc::now();
    let mut messages = Vec::new();
    for (index, message) in material.messages.iter().enumerate() {
        let mut body = MessageBody::text(&message.text);
        for attachment in &message.attachments {
            if attachment.kind == "image" {
                let bytes = client
                    .attachment(
                        &material.task.id,
                        &message.id,
                        attachment.index,
                        &message.revision,
                    )
                    .await?;
                validate_image_download(&bytes, attachment.byte_length)?;
                body.0.push(SocialContent::Image {
                    mime: Some(attachment.mime.clone()),
                    data: bytes,
                });
            } else {
                body.0.push(SocialContent::Text {
                    text: format!(
                        "[附件：{}；{}；{} 字节；未读取内容]",
                        attachment.name.as_deref().unwrap_or("未命名文件"),
                        attachment.mime,
                        attachment.byte_length
                    ),
                });
            }
        }
        messages.push(SocialMessage {
            id: message.id.parse()?,
            conversation_id,
            sender_id: message.sender_id.parse()?,
            sender_name: if message.sender_id == material.ai.id {
                material.ai.name.clone()
            } else {
                material.peer.name.clone()
            },
            sequence: Sequence::new(index as u64 + 1)?,
            body,
            reply_to: None,
            context_reference: None,
            mentions: vec![],
            run_id: None,
            status: MessageStatus::Committed,
            revision: serde_json::from_value(serde_json::Value::String(message.revision.clone()))?,
            created_at: message.created_at.parse()?,
        });
    }
    let input = ContextInput {
        required_message_ids: vec![trigger],
        reference: None,
        summaries: vec![],
        memories: vec![],
        contact: ContactInput {
            actor: Actor {
                id: ai,
                kind: ActorKind::Ai,
                display_name: material.ai.name.clone(),
                avatar_ref: None,
                bio: material.ai.bio,
                status: EntityStatus::Active,
                revision: Revision::ONE,
                created_at: now,
            },
            profile: AiProfile {
                actor_id: ai,
                persona: profile.persona.clone(),
                revision: Revision::ONE,
            },
            binding: Some(ExecutionBinding {
                id: BindingId::new(),
                actor_id: ai,
                provider_id: profile.selection.provider.clone(),
                model_id: profile.selection.model.clone(),
                reasoning_effort: profile
                    .selection
                    .reasoning_effort
                    .map(|effort| effort.as_wire().to_string()),
                temperature: profile.temperature,
                max_tokens: profile.max_tokens,
                revision: Revision::ONE,
            }),
        },
        conversation: Conversation {
            id: conversation_id,
            kind: ConversationKind::Direct,
            relationship_id: None,
            title: String::new(),
            status: EntityStatus::Active,
            paused: false,
            revision: Revision::ONE,
            context_epoch: Epoch::ONE,
            participation_epoch: Epoch::ONE,
            last_sequence: Sequence::new(messages.len() as u64)?,
            created_at: now,
            legacy: false,
        },
        members: vec![],
        relationship_revision: None,
        participant_names: vec![material.ai.name, material.peer.name],
        messages,
        trigger: ContextTrigger::Message(trigger),
    };
    let prepared =
        ConsciousnessService::prepare(input, &resolved.spec, options.max_tokens.unwrap())?;
    let reserve = resolved.spec.context_window.unwrap_or(8192) as u64;
    // The request and client are frozen for the entire invocation.
    Ok((
        ChatRequest {
            selection: profile.selection.clone(),
            messages: prepared.messages,
            options,
            tools: vec![],
            resolved: Some(resolved),
        },
        reserve,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use connector::node::*;
    fn materials() -> TaskMaterials {
        let ai = uuid::Uuid::new_v4().to_string();
        let peer = uuid::Uuid::new_v4().to_string();
        let trigger = uuid::Uuid::new_v4().to_string();
        TaskMaterials {
            task: NodeTask {
                id: uuid::Uuid::new_v4().to_string(),
                ai_id: ai.clone(),
                conversation_id: uuid::Uuid::new_v4().to_string(),
                binding_version: "1".into(),
                revision: "1".into(),
                trigger_message_id: trigger.clone(),
                status: "queued".into(),
                expires_at: chrono::Utc::now().to_rfc3339(),
            },
            ai: NodeAccount {
                id: ai,
                name: "Assistant".into(),
                bio: String::new(),
                kind: "ai".into(),
            },
            peer: NodeAccount {
                id: peer.clone(),
                name: "Human".into(),
                bio: String::new(),
                kind: "human".into(),
            },
            messages: vec![NodeMessage {
                id: trigger,
                sender_id: peer,
                text: "hello".into(),
                revision: "1".into(),
                created_at: chrono::Utc::now().to_rfc3339(),
                attachments: vec![NodeAttachment {
                    index: 0,
                    kind: "file".into(),
                    mime: "text/plain".into(),
                    byte_length: 34,
                    name: Some("readme.txt".into()),
                }],
            }],
            sources: vec![],
        }
    }
    fn router() -> ModelRouter {
        let providers = toml::from_str(
            r#"
[test]
protocol="openai"
base_url="https://example.invalid"
api_key={env="UNUSED_TEST_KEY"}
[test.models.small]
model="small"
context_window=8192
max_tokens=4096
image_input=false
"#,
        )
        .unwrap();
        ModelRouter::new(&providers).unwrap()
    }
    fn profile() -> Profile {
        Profile {
            revision: "1".into(),
            persona: "Be helpful".into(),
            selection: ai_client::ModelSelection {
                provider: "test".into(),
                model: "small".into(),
                reasoning_effort: None,
            },
            temperature: None,
            max_tokens: None,
        }
    }
    #[tokio::test]
    async fn context_uses_core_and_file_metadata_without_fetching_bytes() {
        let api = NodeClient::new("http://127.0.0.1:1", "test", None).unwrap();
        let (request, reserved) = prepare(&api, &router(), &profile(), materials())
            .await
            .unwrap();
        let text = format!("{:?}", request.messages);
        assert!(text.contains("Be helpful"));
        assert!(text.contains("readme.txt"));
        assert!(text.contains("34"));
        assert_eq!(reserved, 8192);
        assert!(request.tools.is_empty());
    }
    #[tokio::test]
    async fn strict_selection_and_unsupported_image_fail_before_network() {
        let api = NodeClient::new("http://127.0.0.1:1", "test", None).unwrap();
        let mut profile = profile();
        profile.selection.model = "missing".into();
        assert!(
            prepare(&api, &router(), &profile, materials())
                .await
                .is_err()
        );
        let mut material = materials();
        material.messages[0].attachments[0].kind = "image".into();
        material.messages[0].attachments[0].mime = "image/png".into();
        assert!(
            prepare(&api, &router(), &super::tests::profile(), material)
                .await
                .unwrap_err()
                .to_string()
                .contains("image_input_unsupported")
        );
    }
    fn image_message(id: &str, count: usize, bytes: u64) -> NodeMessage {
        let mut message = materials().messages.remove(0);
        message.id = id.into();
        message.attachments = (0..count)
            .map(|index| NodeAttachment {
                index,
                kind: "image".into(),
                mime: "image/png".into(),
                byte_length: bytes,
                name: None,
            })
            .collect();
        message
    }
    #[test]
    fn image_candidates_prefer_recent_whole_messages_and_reserve_trigger() {
        let messages = vec![
            image_message("old", 2, IMAGE_BYTES),
            image_message("recent", 2, IMAGE_BYTES),
            image_message("trigger", 2, IMAGE_BYTES),
        ];
        let selected = select_image_history(messages, "trigger").unwrap();
        assert_eq!(
            selected.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(),
            vec!["recent", "trigger"]
        );
        assert!(selected.iter().all(|m| m.attachments.len() == 2));
        let mut messages = (0..20)
            .map(|i| image_message(&i.to_string(), 1, 1))
            .collect::<Vec<_>>();
        messages.push(image_message("trigger", 1, 1));
        let selected = select_image_history(messages, "trigger").unwrap();
        assert_eq!(selected.len(), 16);
        assert_eq!(selected[0].id, "5");
        assert_eq!(selected[15].id, "trigger");
    }
    #[test]
    fn required_image_overflow_and_untrusted_download_lengths_are_rejected() {
        assert!(select_image_history(vec![image_message("trigger", 17, 1)], "trigger").is_err());
        assert!(
            select_image_history(vec![image_message("trigger", 5, IMAGE_BYTES)], "trigger")
                .is_err()
        );
        assert!(
            select_image_history(
                vec![image_message("trigger", 1, IMAGE_BYTES + 1)],
                "trigger"
            )
            .is_err()
        );
        assert!(validate_image_download(&[1, 2, 3], 2).is_err());
        validate_image_download(&[1, 2, 3], 3).unwrap();
    }
}
