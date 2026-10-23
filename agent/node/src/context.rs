use crate::store::Profile;
use crate::store::Store;
use ai_client::{ChatRequest, ModelRouter, image_input::ImageSupport};
use anyhow::{Result, bail};
use builtin_agent::{
    ContactInput, ContextInput, ContextTrigger, consciousness::ConsciousnessService, generation,
};
use connector::node::{
    MaterialScope, NodeClient, NodeMessage, NodeTrigger, ScopedSource, TaskMaterials,
};
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
#[cfg(test)]
fn select_image_history(messages: Vec<NodeMessage>, trigger: &str) -> Result<Vec<NodeMessage>> {
    let required = message_required_ids(&messages, trigger, &[trigger.to_string()])?;
    select_images(messages, &required, 0, 0)
}
fn message_required_ids(
    messages: &[NodeMessage],
    trigger: &str,
    inputs: &[String],
) -> Result<Vec<String>> {
    anyhow::ensure!(
        !inputs.is_empty() && inputs.len() <= 20 && inputs.iter().any(|id| id == trigger),
        "invalid_trigger_inputs"
    );
    let mut required = Vec::new();
    for id in inputs {
        anyhow::ensure!(
            messages.iter().any(|m| &m.id == id),
            "missing required input message"
        );
        if !required.contains(id) {
            required.push(id.clone());
        }
    }
    let trigger_message = messages
        .iter()
        .find(|m| m.id == trigger)
        .expect("validated trigger input");
    if let Some(reply) = &trigger_message.reply_to {
        anyhow::ensure!(
            messages.iter().any(|m| &m.id == reply),
            "missing reply message"
        );
        if !required.contains(reply) {
            required.push(reply.clone());
        }
    }
    Ok(required)
}
fn select_images(
    messages: Vec<NodeMessage>,
    required: &[String],
    reserved_count: usize,
    reserved_bytes: u64,
) -> Result<Vec<NodeMessage>> {
    let mut selected = messages
        .iter()
        .map(|m| required.contains(&m.id))
        .collect::<Vec<_>>();
    let (mut count, mut bytes) = (reserved_count, reserved_bytes);
    for (message, required) in messages.iter().zip(&selected) {
        if *required {
            let cost = image_cost(message)?;
            count += cost.0;
            bytes += cost.1;
        }
    }
    anyhow::ensure!(
        count <= 16 && bytes <= REQUEST_IMAGE_BYTES,
        "required_images_exceed_budget"
    );
    for index in (0..messages.len()).rev() {
        if selected[index] {
            continue;
        }
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

fn context_text(message: &NodeMessage) -> String {
    if message.import_origin.is_some() {
        format!(
            "[导入的旧聊天记录；不代表当前云端 AI 执行过该历史调用]\n{}",
            message.text
        )
    } else {
        message.text.clone()
    }
}

pub async fn messages(
    client: &NodeClient,
    task: &str,
    conversation_id: ConversationId,
    raw: &[NodeMessage],
) -> Result<Vec<SocialMessage>> {
    let mut messages = Vec::new();
    for message in raw {
        let mut body = MessageBody::text(context_text(message));
        for attachment in &message.attachments {
            if attachment.kind == "image" {
                let bytes = client
                    .attachment(task, &message.id, attachment.index, &message.revision)
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
            sender_name: message.sender_name.clone(),
            sequence: Sequence::new(message.position.parse()?)?,
            body,
            reply_to: message.reply_to.as_deref().map(str::parse).transpose()?,
            context_reference: None,
            mentions: message
                .mentions
                .iter()
                .map(|id| id.parse())
                .collect::<Result<_, _>>()?,
            run_id: None,
            status: MessageStatus::Committed,
            revision: serde_json::from_value(serde_json::Value::String(message.revision.clone()))?,
            created_at: message.created_at.parse()?,
        });
    }
    Ok(messages)
}
pub fn text_message(cid: &str, message: &NodeMessage) -> Result<SocialMessage> {
    let mut body = MessageBody::text(context_text(message));
    for a in &message.attachments {
        body.0.push(SocialContent::Text {
            text: social::attachment::placeholder(
                a.name.as_deref(),
                &a.mime,
                a.byte_length as usize,
            ),
        });
    }
    Ok(SocialMessage {
        id: message.id.parse()?,
        conversation_id: cid.parse()?,
        sender_id: message.sender_id.parse()?,
        sender_name: message.sender_name.clone(),
        sequence: Sequence::new(message.position.parse()?)?,
        body,
        reply_to: message.reply_to.as_deref().map(str::parse).transpose()?,
        context_reference: None,
        mentions: vec![],
        run_id: None,
        status: MessageStatus::Committed,
        revision: crate::consciousness::revision(&message.revision)?,
        created_at: message.created_at.parse()?,
    })
}

#[cfg(test)]
pub async fn prepare(
    client: &NodeClient,
    router: &ModelRouter,
    profile: &Profile,
    material: TaskMaterials,
) -> Result<(ChatRequest, u64)> {
    let (request, tokens, _, _) = prepare_scoped(client, router, profile, material, None).await?;
    Ok((request, tokens))
}
pub async fn prepare_scoped(
    client: &NodeClient,
    router: &ModelRouter,
    profile: &Profile,
    material: TaskMaterials,
    store: Option<&Store>,
) -> Result<(ChatRequest, u64, Vec<ScopedSource>, Vec<MaterialScope>)> {
    let (request, tokens, derived, scopes, _) =
        prepare_inner(client, router, profile, material, store).await?;
    Ok((request, tokens, derived, scopes))
}
/// Return the core-selected message sources, including local IDs for caller filtering.
pub async fn prepare_direct(
    client: &NodeClient,
    router: &ModelRouter,
    profile: &Profile,
    material: TaskMaterials,
    store: Option<&Store>,
) -> Result<(
    ChatRequest,
    u64,
    Vec<ScopedSource>,
    Vec<MaterialScope>,
    Vec<connector::node::SourceRevision>,
)> {
    anyhow::ensure!(
        material.reference.is_none() && material.messages.iter().all(|m| m.attachments.is_empty()),
        "direct_text_only"
    );
    prepare_inner(client, router, profile, material, store).await
}
async fn prepare_inner(
    client: &NodeClient,
    router: &ModelRouter,
    profile: &Profile,
    mut material: TaskMaterials,
    store: Option<&Store>,
) -> Result<(
    ChatRequest,
    u64,
    Vec<ScopedSource>,
    Vec<MaterialScope>,
    Vec<connector::node::SourceRevision>,
)> {
    let resolved = router.resolve(&profile.selection)?;
    let options = generation::options(
        &generation::Parameters {
            temperature: profile.temperature,
            max_tokens: profile.max_tokens,
        },
        &resolved,
    )?;
    let (mut ref_count, mut ref_bytes) = (0, 0);
    if let Some(reference) = &material.reference {
        for message in &reference.messages {
            let (count, bytes) = image_cost(message)?;
            ref_count += count;
            ref_bytes += bytes;
        }
    }
    let required_ids = match &material.task.trigger {
        NodeTrigger::Message {
            message_id,
            input_ids,
        } => message_required_ids(&material.messages, message_id, input_ids)?,
        NodeTrigger::Contact { .. } => {
            anyhow::ensure!(
                material.task.automatic && material.reference.is_none(),
                "invalid_contact_trigger"
            );
            vec![]
        }
    };
    let mut required_count = 0;
    let mut required_bytes = 0;
    for message in material
        .messages
        .iter()
        .filter(|m| required_ids.contains(&m.id))
    {
        let (count, bytes) = image_cost(message)?;
        required_count += count;
        required_bytes += bytes;
    }
    if ref_count + required_count > 16 || ref_bytes + required_bytes > REQUEST_IMAGE_BYTES {
        if material.reference.as_ref().is_some_and(|r| r.required) {
            bail!("required_images_exceed_budget");
        }
        material.reference = None;
        ref_count = 0;
        ref_bytes = 0;
    }
    material.messages = select_images(material.messages, &required_ids, ref_count, ref_bytes)?;
    let has_images = material
        .messages
        .iter()
        .any(|m| m.attachments.iter().any(|a| a.kind == "image"))
        || ref_count > 0;
    if has_images && resolved.spec.image_input.support == ImageSupport::Unsupported {
        bail!("image_input_unsupported");
    }
    let ai: ActorId = material.ai.id.parse()?;
    let conversation_id: ConversationId = material.task.conversation_id.parse()?;
    anyhow::ensure!(
        material.conversation.id == material.task.conversation_id,
        "conversation mismatch"
    );
    let conversation_revision = serde_json::from_value(serde_json::Value::String(
        material.conversation.revision.clone(),
    ))?;
    let now = chrono::Utc::now();
    let trigger = match &material.task.trigger {
        NodeTrigger::Message { message_id, .. } => ContextTrigger::Message(message_id.parse()?),
        NodeTrigger::Contact {
            intent_id,
            revision,
            creator_id,
            content,
            due_at,
        } => ContextTrigger::Contact(ContactIntent {
            id: intent_id.parse()?,
            conversation_id,
            actor_id: ai,
            created_by: creator_id.parse()?,
            authorization_id: intent_id.parse()?,
            text: content.clone(),
            due_at: due_at.parse()?,
            expires_at: material.task.expires_at.parse()?,
            utc_offset_minutes: 0,
            status: ContactIntentStatus::Executing,
            plan_id: None,
            revision: crate::consciousness::revision(revision)?,
            reason: None,
            created_at: now,
            updated_at: now,
        }),
    };
    let messages = messages(
        client,
        &material.task.id,
        conversation_id,
        &material.messages,
    )
    .await?;
    let mut reference = None;
    let mut reference_summary = None;
    if let Some(r) = &material.reference {
        if let Some(store) = store {
            reference_summary = store
                .summaries(&material.ai.id, &r.conversation.id)?
                .into_iter()
                .find(|s| {
                    s.item.status == "ready"
                        && r.sources.iter().any(|src| {
                            s.item.sources.iter().any(|own| {
                                own.message_id == src.message_id && own.revision == src.revision
                            })
                        })
                });
        }
        reference = Some(builtin_agent::ReferenceInput {
            message_id: r.message_id.parse()?,
            revision: crate::consciousness::revision(&r.revision)?,
            required: r.required,
            preview: builtin_agent::ReferenceInputPreview {
                conversation: Conversation {
                    id: r.conversation.id.parse()?,
                    kind: ConversationKind::Group,
                    relationship_id: None,
                    title: r.conversation.name.clone(),
                    status: EntityStatus::Active,
                    paused: false,
                    revision: crate::consciousness::revision(&r.conversation.revision)?,
                    context_epoch: Epoch::ONE,
                    participation_epoch: Epoch::ONE,
                    last_sequence: Sequence::new(
                        r.messages
                            .last()
                            .map(|m| m.position.parse())
                            .transpose()?
                            .unwrap_or(0),
                    )?,
                    created_at: now,
                    legacy: false,
                },
                messages: self::messages(
                    client,
                    &material.task.id,
                    r.conversation.id.parse()?,
                    &r.messages,
                )
                .await?,
                summary: reference_summary
                    .as_ref()
                    .map(|s| crate::consciousness::summary_record(&r.conversation.id, s))
                    .transpose()?,
                dependencies: vec![],
            },
        });
    }
    let local_memories = store
        .map(|s| s.memories(&material.ai.id, &material.conversation.id))
        .transpose()?
        .unwrap_or_default();
    let local_summaries = store
        .map(|s| s.summaries(&material.ai.id, &material.conversation.id))
        .transpose()?
        .unwrap_or_default();
    let memories = local_memories
        .iter()
        .filter(|m| m.status == "confirmed")
        .map(|m| crate::consciousness::memory_record(&material.ai.id, &material.conversation.id, m))
        .collect::<Result<Vec<_>>>()?;
    let summaries = local_summaries
        .iter()
        .filter(|m| m.item.status == "ready")
        .map(|m| crate::consciousness::summary_record(&material.conversation.id, m))
        .collect::<Result<Vec<_>>>()?;
    let mut scopes = if let Some(store) = store {
        let mut scopes = vec![store.material_scope(&material.ai.id, &material.conversation.id)?];
        if let Some(r) = &material.reference {
            scopes.push(store.material_scope(&material.ai.id, &r.conversation.id)?);
        }
        scopes
    } else {
        vec![]
    };
    let input = ContextInput {
        required_message_ids: required_ids
            .iter()
            .map(|id| id.parse())
            .collect::<Result<_, _>>()?,
        reference,
        summaries,
        memories,
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
            kind: match material.conversation.kind.as_str() {
                "direct" => ConversationKind::Direct,
                "group" => ConversationKind::Group,
                _ => bail!("invalid conversation kind"),
            },
            relationship_id: None,
            title: material.conversation.name.clone(),
            status: EntityStatus::Active,
            paused: false,
            revision: conversation_revision,
            context_epoch: Epoch::ONE,
            participation_epoch: Epoch::ONE,
            last_sequence: messages
                .last()
                .map(|m| m.sequence)
                .unwrap_or(Sequence::ZERO),
            created_at: now,
            legacy: false,
        },
        members: material
            .conversation
            .members
            .iter()
            .map(|member| {
                Ok(Membership {
                    conversation_id,
                    actor_id: member.id.parse()?,
                    status: MembershipStatus::Active,
                    paused: false,
                    revision: Revision::ONE,
                })
            })
            .collect::<Result<_>>()?,
        relationship_revision: None,
        participant_names: material
            .conversation
            .members
            .iter()
            .map(|member| member.name.clone())
            .collect(),
        messages,
        trigger,
    };
    let prepared =
        ConsciousnessService::prepare(input, &resolved.spec, options.max_tokens.unwrap())?;
    let mut derived = Vec::new();
    let mut add = |cid: &str, src: &connector::node::SourceRevision| {
        let source = ScopedSource {
            conversation_id: cid.into(),
            message_id: src.message_id.clone(),
            revision: src.revision.clone(),
        };
        if !derived.contains(&source) {
            derived.push(source);
        }
    };
    for source in &prepared.snapshot.sources {
        match source {
            SourceRef::Memory { memory_id, .. } => {
                if let Some(m) = local_memories
                    .iter()
                    .find(|m| m.id == memory_id.to_string())
                    && let Some(source) = &m.source
                {
                    add(&material.conversation.id, source);
                }
            }
            SourceRef::Summary { summary_id, .. } => {
                if let Some(m) = local_summaries
                    .iter()
                    .find(|m| m.item.id == summary_id.to_string())
                {
                    for source in &m.item.sources {
                        add(&material.conversation.id, source);
                    }
                } else if let (Some(s), Some(r)) = (&reference_summary, &material.reference)
                    && s.item.id == summary_id.to_string()
                {
                    for source in &s.item.sources {
                        add(&r.conversation.id, source);
                    }
                }
            }
            SourceRef::Message {
                conversation_id,
                message_id,
                revision,
            } if *conversation_id != material.task.conversation_id.parse()? => add(
                &conversation_id.to_string(),
                &connector::node::SourceRevision {
                    message_id: message_id.to_string(),
                    revision: revision.to_string(),
                },
            ),
            _ => {}
        }
    }
    if !prepared
        .snapshot
        .sources
        .iter()
        .any(|s| matches!(s, SourceRef::Reference { .. }))
    {
        scopes.retain(|s| s.conversation_id == material.task.conversation_id);
    }
    let actual_messages = prepared
        .snapshot
        .sources
        .iter()
        .filter_map(|source| match source {
            SourceRef::Message {
                conversation_id: cid,
                message_id,
                revision,
            } if *cid == conversation_id => Some(connector::node::SourceRevision {
                message_id: message_id.to_string(),
                revision: revision.to_string(),
            }),
            _ => None,
        })
        .collect();
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
        derived,
        scopes,
        actual_messages,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use connector::node::*;
    fn materials() -> TaskMaterials {
        let ai = uuid::Uuid::new_v4().to_string();
        let peer = uuid::Uuid::new_v4().to_string();
        let conversation = uuid::Uuid::new_v4().to_string();
        let trigger = uuid::Uuid::new_v4().to_string();
        TaskMaterials {
            task: NodeTask {
                id: uuid::Uuid::new_v4().to_string(),
                ai_id: ai.clone(),
                conversation_id: conversation.clone(),
                binding_version: "1".into(),
                revision: "1".into(),
                trigger: NodeTrigger::Message {
                    message_id: trigger.clone(),
                    input_ids: vec![trigger.clone()],
                },
                automatic: false,
                status: "queued".into(),
                expires_at: chrono::Utc::now().to_rfc3339(),
            },
            ai: NodeAccount {
                id: ai,
                name: "Assistant".into(),
                bio: String::new(),
                kind: "ai".into(),
            },
            conversation: NodeConversation {
                id: conversation,
                kind: "direct".into(),
                name: String::new(),
                revision: "1".into(),
                members: vec![NodeAccount {
                    id: peer.clone(),
                    name: "Human".into(),
                    bio: String::new(),
                    kind: "human".into(),
                }],
            },
            messages: vec![NodeMessage {
                import_origin: None,
                id: trigger,
                position: "1".into(),
                sender_id: peer,
                sender_name: "Human".into(),
                reply_to: None,
                mentions: vec![],
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
            reference: None,
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
            auto_daily_tokens: None,
        }
    }
    #[tokio::test]
    async fn merged_inputs_are_all_required_and_never_truncated() {
        let api = NodeClient::new("http://127.0.0.1:1", "test", None).unwrap();
        let mut material = materials();
        let mut first = material.messages[0].clone();
        first.id = uuid::Uuid::new_v4().to_string();
        first.text = "first necessary input".into();
        first.position = "1".into();
        material.messages[0].position = "2".into();
        material.messages[0].text = "last necessary input".into();
        let trigger = material.messages[0].id.clone();
        material.task.trigger = NodeTrigger::Message {
            message_id: trigger.clone(),
            input_ids: vec![first.id.clone(), trigger],
        };
        material.messages.insert(0, first);
        let (request, _) = prepare(&api, &router(), &profile(), material.clone())
            .await
            .unwrap();
        let text = format!("{:?}", request.messages);
        assert!(text.contains("first necessary input") && text.contains("last necessary input"));
        material.messages[0].text = "required".repeat(2000);
        assert!(
            prepare(&api, &router(), &profile(), material.clone())
                .await
                .is_err()
        );
        material.messages.remove(0);
        let error = prepare(&api, &router(), &profile(), material)
            .await
            .unwrap_err();
        assert_eq!(error.to_string(), "missing required input message");
    }
    #[test]
    fn merged_input_images_are_reserved_before_optional_history() {
        let first = image_message("first", 2, IMAGE_BYTES);
        let history = image_message("history", 2, IMAGE_BYTES);
        let trigger = image_message("trigger", 2, IMAGE_BYTES);
        let required = vec!["first".to_string(), "trigger".to_string()];
        let selected = select_images(vec![first, history, trigger], &required, 0, 0).unwrap();
        assert_eq!(
            selected.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(),
            vec!["first", "trigger"]
        );
        assert!(
            select_images(
                vec![
                    image_message("first", 3, IMAGE_BYTES),
                    image_message("trigger", 2, IMAGE_BYTES)
                ],
                &required,
                0,
                0
            )
            .is_err()
        );
    }
    #[tokio::test]
    async fn contact_is_an_independent_trigger_with_empty_history_and_no_reference() {
        let api = NodeClient::new("http://127.0.0.1:1", "test", None).unwrap();
        let mut material = materials();
        material.task.automatic = true;
        material.task.trigger = NodeTrigger::Contact {
            intent_id: uuid::Uuid::new_v4().to_string(),
            revision: "2".into(),
            creator_id: material.messages[0].sender_id.clone(),
            content: "authorized follow up".into(),
            due_at: chrono::Utc::now().to_rfc3339(),
        };
        material.messages.clear();
        let (request, _) = prepare(&api, &router(), &profile(), material.clone())
            .await
            .unwrap();
        let text = format!("{:?}", request.messages);
        assert!(text.contains("authorized follow up"));
        assert!(text.contains("不是新的聊天消息"));
        assert!(!text.contains("hello"));
        material.reference = Some(NodeReference {
            message_id: uuid::Uuid::new_v4().to_string(),
            revision: "1".into(),
            required: false,
            conversation: material.conversation.clone(),
            messages: vec![],
            sources: vec![],
        });
        let error = prepare(&api, &router(), &profile(), material)
            .await
            .unwrap_err();
        assert_eq!(error.to_string(), "invalid_contact_trigger");
    }
    #[tokio::test]
    async fn direct_sources_match_selected_text_without_network() {
        let api = NodeClient::new("http://127.0.0.1:1", "test", None).unwrap();
        let mut material = materials();
        for message in &mut material.messages {
            message.attachments.clear();
        }
        let trigger = match &material.task.trigger {
            NodeTrigger::Message { message_id, .. } => message_id.clone(),
            _ => unreachable!(),
        };
        let (_, _, _, _, sources) =
            prepare_direct(&api, &router(), &profile(), material.clone(), None)
                .await
                .unwrap();
        assert!(sources.iter().any(|s| s.message_id == trigger));
        assert!(sources.iter().all(|s| {
            material
                .messages
                .iter()
                .any(|m| m.id == s.message_id && m.revision == s.revision)
        }));
        material.messages[0].attachments.push(NodeAttachment {
            index: 0,
            kind: "file".into(),
            mime: "text/plain".into(),
            byte_length: 1,
            name: None,
        });
        assert_eq!(
            prepare_direct(&api, &router(), &profile(), material, None)
                .await
                .unwrap_err()
                .to_string(),
            "direct_text_only"
        );
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
    #[tokio::test]
    async fn group_context_keeps_roster_actual_senders_and_required_reply() {
        let api = NodeClient::new("http://127.0.0.1:1", "test", None).unwrap();
        let mut material = materials();
        material.conversation.kind = "group".into();
        material.conversation.name = "Working group".into();
        material.conversation.members.push(material.ai.clone());
        let other = NodeAccount {
            id: uuid::Uuid::new_v4().to_string(),
            name: "Other AI".into(),
            bio: String::new(),
            kind: "ai".into(),
        };
        material.conversation.members.push(other.clone());
        let mut quoted = material.messages[0].clone();
        quoted.id = uuid::Uuid::new_v4().to_string();
        quoted.sender_id = other.id;
        quoted.sender_name = other.name;
        quoted.text = "Earlier independent answer".into();
        quoted.attachments.clear();
        material.messages[0].reply_to = Some(quoted.id.clone());
        material.messages[0].sender_name = "Original human".into();
        material.messages.insert(0, quoted);
        let (request, _) = prepare(&api, &router(), &profile(), material.clone())
            .await
            .unwrap();
        let text = format!("{:?}", request.messages);
        for expected in [
            "Working group",
            "Other AI",
            "Original human",
            "Earlier independent answer",
        ] {
            assert!(text.contains(expected), "missing {expected}");
        }
        material.task.conversation_id = uuid::Uuid::new_v4().to_string();
        material.conversation.id = material.task.conversation_id.clone();
        material.conversation.name = "Separate group".into();
        material.messages.remove(0);
        material.messages[0].reply_to = None;
        let (request, _) = prepare(&api, &router(), &profile(), material)
            .await
            .unwrap();
        let text = format!("{:?}", request.messages);
        assert!(text.contains("Separate group"));
        assert!(!text.contains("Earlier independent answer"));
    }
    #[test]
    fn reply_images_are_required_even_when_older_than_recent_history() {
        let reply = image_message("reply", 2, IMAGE_BYTES);
        let history = image_message("history", 2, IMAGE_BYTES);
        let mut trigger = image_message("trigger", 2, IMAGE_BYTES);
        trigger.reply_to = Some("reply".into());
        let selected = select_image_history(vec![reply, history, trigger], "trigger").unwrap();
        assert_eq!(
            selected.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(),
            vec!["reply", "trigger"]
        );
        let mut trigger = image_message("trigger", 3, IMAGE_BYTES);
        trigger.reply_to = Some("reply".into());
        assert!(
            select_image_history(
                vec![image_message("reply", 2, IMAGE_BYTES), trigger],
                "trigger"
            )
            .is_err()
        );
        let mut trigger = image_message("trigger", 0, 0);
        trigger.reply_to = Some("missing".into());
        assert!(select_image_history(vec![trigger], "trigger").is_err());
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
    #[tokio::test]
    async fn scoped_context_uses_only_confirmed_memory_and_expands_actual_source_ids() {
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        let material = materials();
        let ai = &material.ai.id;
        let cid = &material.conversation.id;
        let memory = connector::node::MemoryItem {
            id: uuid::Uuid::new_v4().to_string(),
            revision: "1".into(),
            kind: "fact".into(),
            text: "confirmed useful fact".into(),
            status: "confirmed".into(),
            source: Some(SourceRevision {
                message_id: material.messages[0].id.clone(),
                revision: "1".into(),
            }),
            created_at: chrono::Utc::now().to_rfc3339(),
            updated_at: chrono::Utc::now().to_rfc3339(),
        };
        store
            .db
            .execute(
                "INSERT INTO conscious_memory VALUES(?1,?2,?3,?4)",
                rusqlite::params![ai, cid, memory.id, serde_json::to_string(&memory).unwrap()],
            )
            .unwrap();
        let mut stale = memory.clone();
        stale.id = uuid::Uuid::new_v4().to_string();
        stale.text = "stale hidden fact".into();
        stale.status = "stale".into();
        store
            .db
            .execute(
                "INSERT INTO conscious_memory VALUES(?1,?2,?3,?4)",
                rusqlite::params![ai, cid, stale.id, serde_json::to_string(&stale).unwrap()],
            )
            .unwrap();
        let api = NodeClient::new("http://127.0.0.1:1", "test", None).unwrap();
        let (request, _, sources, scopes) =
            prepare_scoped(&api, &router(), &profile(), material.clone(), Some(&store))
                .await
                .unwrap();
        let text = format!("{:?}", request.messages);
        assert!(text.contains("confirmed useful fact"));
        assert!(!text.contains("stale hidden fact"));
        assert_eq!(
            sources,
            vec![ScopedSource {
                conversation_id: cid.clone(),
                message_id: material.messages[0].id.clone(),
                revision: "1".into()
            }]
        );
        let unrelated = store
            .material_scope(ai, &uuid::Uuid::new_v4().to_string())
            .unwrap();
        store
            .db
            .execute(
                "UPDATE conscious_scopes SET epoch=?3 WHERE ai=?1 AND cid=?2",
                rusqlite::params![
                    ai,
                    unrelated.conversation_id,
                    uuid::Uuid::new_v4().to_string()
                ],
            )
            .unwrap();
        assert!(store.scopes_current(ai, &scopes).unwrap());
        store
            .db
            .execute(
                "UPDATE conscious_scopes SET epoch=?3 WHERE ai=?1 AND cid=?2",
                rusqlite::params![ai, cid, uuid::Uuid::new_v4().to_string()],
            )
            .unwrap();
        assert!(!store.scopes_current(ai, &scopes).unwrap());
    }
    #[tokio::test]
    async fn optional_reference_images_never_displace_required_trigger_images() {
        let mut material = materials();
        material.messages[0].attachments = image_message("source", 4, IMAGE_BYTES).attachments;
        let reference = NodeReference {
            message_id: material.messages[0].id.clone(),
            revision: "1".into(),
            required: false,
            conversation: material.conversation.clone(),
            messages: vec![image_message("reference", 1, 1)],
            sources: vec![],
        };
        material.reference = Some(reference);
        let api = NodeClient::new("http://127.0.0.1:1", "test", None).unwrap();
        let error = prepare_scoped(&api, &router(), &profile(), material.clone(), None)
            .await
            .unwrap_err();
        assert_eq!(error.to_string(), "image_input_unsupported");
        material.reference.as_mut().unwrap().required = true;
        let error = prepare_scoped(&api, &router(), &profile(), material, None)
            .await
            .unwrap_err();
        assert_eq!(error.to_string(), "required_images_exceed_budget");
    }
}
