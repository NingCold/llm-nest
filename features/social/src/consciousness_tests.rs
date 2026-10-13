use crate::{SocialError, consciousness::ConsciousnessService};
use ai_client::{ModelSpec, Protocol};
use social::*;
use storage::social::{ContactRecord, ContextMaterial};

fn fixture() -> (ContextMaterial, ModelSpec) {
    let human = ActorId::new();
    let ai = ActorId::new();
    let id = ConversationId::new();
    let actor = Actor {
        id: ai,
        kind: ActorKind::Ai,
        display_name: "A".into(),
        avatar_ref: None,
        bio: String::new(),
        status: EntityStatus::Active,
        revision: Revision::ONE,
        created_at: "2026-10-09T00:00:00Z".parse().unwrap(),
    };
    let contact = ContactRecord {
        actor,
        profile: AiProfile {
            actor_id: ai,
            persona: "short persona".into(),
            revision: Counter::new(3).unwrap(),
        },
        binding: Some(ExecutionBinding {
            id: BindingId::new(),
            actor_id: ai,
            provider_id: "test".into(),
            model_id: "model".into(),
            reasoning_effort: None,
            temperature: None,
            max_tokens: None,
            revision: Counter::new(4).unwrap(),
        }),
    };
    let conversation = Conversation {
        id,
        kind: ConversationKind::Direct,
        relationship_id: Some(RelationshipId::new()),
        title: "A".into(),
        status: EntityStatus::Active,
        paused: false,
        revision: Counter::new(8).unwrap(),
        context_epoch: Counter::new(2).unwrap(),
        participation_epoch: Revision::ONE,
        last_sequence: Counter::new(2).unwrap(),
        created_at: "2026-10-09T00:00:00Z".parse().unwrap(),
        legacy: false,
    };
    let members = [human, ai]
        .into_iter()
        .map(|actor_id| Membership {
            conversation_id: id,
            actor_id,
            status: MembershipStatus::Active,
            paused: false,
            revision: Counter::new(5).unwrap(),
        })
        .collect();
    let messages: Vec<SocialMessage> = ["old ".repeat(1000), "latest".into()]
        .into_iter()
        .enumerate()
        .map(|(i, text)| SocialMessage {
            context_reference: None,
            id: MessageId::new(),
            conversation_id: id,
            sender_id: human,
            sender_name: "Human".into(),
            sequence: Counter::new(i as u64 + 1).unwrap(),
            body: MessageBody::text(text),
            reply_to: None,
            mentions: vec![],
            run_id: None,
            status: MessageStatus::Committed,
            revision: Counter::new(6).unwrap(),
            created_at: "2026-10-09T00:00:00Z".parse().unwrap(),
        })
        .collect();
    let trigger_id = messages.last().unwrap().id;
    let material = ContextMaterial {
        required_message_ids: vec![],
        reference: None,
        summaries: vec![],
        memories: vec![],
        participant_names: vec!["Human".into(), "A".into()],
        trigger: storage::social::ContextTrigger::Message(trigger_id),
        contact,
        conversation,
        members,
        relationship_revision: Some(Revision::ONE),
        messages,
    };
    let model = ModelSpec {
        id: "model".into(),
        wire: "model-wire".into(),
        display_name: "Model".into(),
        context_window: Some(1024),
        max_tokens: Some(128),
        protocol: Protocol::OpenAIChat,
        reasoning: None,
    };
    (material, model)
}

#[test]
fn scheduled_contact_has_its_own_required_instruction_and_proof_without_fake_history() {
    let (mut m, mut model) = fixture();
    model.context_window = Some(8192);
    m.messages.clear();
    let i = ContactIntent {
        id: IntentId::new(),
        conversation_id: m.conversation.id,
        actor_id: m.contact.actor.id,
        created_by: m.members[0].actor_id,
        authorization_id: CommandId::new(),
        text: "ask about reading".into(),
        due_at: m.contact.actor.created_at,
        expires_at: m.contact.actor.created_at,
        utc_offset_minutes: 480,
        status: ContactIntentStatus::Claimed,
        plan_id: Some(PlanId::new()),
        revision: Revision::ONE,
        reason: None,
        created_at: m.contact.actor.created_at,
        updated_at: m.contact.actor.created_at,
    };
    m.trigger = storage::social::ContextTrigger::Contact(i.clone());
    let prepared = ConsciousnessService::prepare(m.clone(), &model, 128).unwrap();
    assert_eq!(
        prepared.snapshot.sources,
        vec![SourceRef::ContactIntent { intent_id: i.id }]
    );
    assert_eq!(prepared.messages.len(), 2);
    let wire = serde_json::to_string(&prepared.messages).unwrap();
    assert!(wire.contains("不是新的聊天消息"));
    assert!(wire.contains("ask about reading"));
    assert!(!wire.contains("Human ·"));
    if let storage::social::ContextTrigger::Contact(i) = &mut m.trigger {
        i.text = "x".repeat(9000);
    }
    assert!(matches!(
        ConsciousnessService::prepare(m, &model, 128),
        Err(SocialError::BudgetExceeded)
    ));
}

fn segment(material: &ContextMaterial) -> ConversationSummary {
    let message = &material.messages[0];
    ConversationSummary {
        id: SummaryId::new(),
        conversation_id: material.conversation.id,
        from_sequence: message.sequence,
        through_sequence: message.sequence,
        text: "earlier shared decision".into(),
        status: SummaryStatus::Ready,
        revision: Revision::ONE,
        sources: vec![SourceRef::Message {
            conversation_id: message.conversation_id,
            message_id: message.id,
            revision: message.revision,
        }],
        binding: SummaryBinding {
            provider_id: "summary-provider".into(),
            model_id: "summary-model".into(),
            reasoning_effort: None,
            max_tokens: 1024,
        },
        settings_revision: Revision::ONE,
        requested_by: message.sender_id,
        membership_revision: Revision::ONE,
        participation_epoch: Revision::ONE,
        attempt: Revision::ONE,
        error: None,
        created_at: message.created_at,
        updated_at: message.created_at,
    }
}

#[test]
fn merged_inputs_are_all_required_and_cannot_be_replaced_by_a_summary() {
    let (mut material, mut model) = fixture();
    material.required_message_ids = material.messages.iter().map(|m| m.id).collect();
    material.summaries.push(segment(&material));
    assert!(matches!(
        ConsciousnessService::prepare(material.clone(), &model, 128),
        Err(SocialError::BudgetExceeded)
    ));
    model.context_window = Some(8192);
    let prepared = ConsciousnessService::prepare(material.clone(), &model, 128).unwrap();
    for id in material.required_message_ids {
        assert!(
            prepared
                .snapshot
                .sources
                .iter()
                .any(|s| matches!(s,SourceRef::Message{message_id,..} if *message_id==id))
        );
    }
}

#[test]
fn summaries_compress_old_originals_but_preserve_trigger_and_explicit_reply() {
    let (mut material, mut model) = fixture();
    model.context_window = Some(2048);
    let summary = segment(&material);
    material.summaries.push(summary.clone());
    let prepared = ConsciousnessService::prepare(material.clone(), &model, 128).unwrap();
    assert!(prepared.snapshot.sources.contains(&SourceRef::Summary {
        summary_id: summary.id,
        revision: summary.revision
    }));
    assert!(!prepared.snapshot.sources.iter().any(
        |s| matches!(s,SourceRef::Message{message_id,..} if *message_id==material.messages[0].id)
    ));
    assert!(prepared.snapshot.sources.iter().any(
        |s| matches!(s,SourceRef::Message{message_id,..} if *message_id==material.trigger.message_id().unwrap())
    ));
    assert!(
        prepared
            .messages
            .iter()
            .map(crate::consciousness::estimate)
            .sum::<u32>()
            <= 1920
    );
    material.messages[1].reply_to = Some(material.messages[0].id);
    assert!(
        matches!(
            ConsciousnessService::prepare(material, &model, 128),
            Err(SocialError::BudgetExceeded)
        ),
        "a summary never substitutes an explicitly quoted original that cannot fit"
    );
}

#[test]
fn short_windows_skip_summaries_and_stale_or_future_segments_are_excluded() {
    let (mut material, mut model) = fixture();
    model.context_window = Some(2048);
    let mut summary = segment(&material);
    summary.status = SummaryStatus::Stale;
    material.summaries.push(summary.clone());
    let prepared = ConsciousnessService::prepare(material.clone(), &model, 128).unwrap();
    assert!(
        !prepared
            .snapshot
            .sources
            .iter()
            .any(|s| matches!(s, SourceRef::Summary { .. }))
    );
    material.summaries[0].status = SummaryStatus::Ready;
    material.summaries[0].through_sequence = material.messages[1].sequence;
    let prepared = ConsciousnessService::prepare(material.clone(), &model, 128).unwrap();
    assert!(
        !prepared
            .snapshot
            .sources
            .iter()
            .any(|s| matches!(s, SourceRef::Summary { .. }))
    );
    material.summaries[0].through_sequence = Sequence::ONE;
    material.messages[0].body = MessageBody::text("brief old message");
    let prepared = ConsciousnessService::prepare(material, &model, 128).unwrap();
    assert!(
        !prepared
            .snapshot
            .sources
            .iter()
            .any(|s| matches!(s, SourceRef::Summary { .. }))
    );
    assert_eq!(prepared.snapshot.sources.len(), 2);
}

#[test]
fn context_selects_recent_sources_with_versions_without_rewriting_history() {
    let (material, model) = fixture();
    let original = material.messages.clone();
    let prepared = ConsciousnessService::prepare(material.clone(), &model, 128).unwrap();
    assert_eq!(material.messages, original);
    assert_eq!(prepared.messages.len(), 2);
    assert!(prepared.messages[0].text().contains("short persona"));
    assert!(prepared.messages[1].text().contains("Human"));
    assert!(prepared.messages[1].text().contains("latest"));
    assert!(!prepared.messages[1].text().contains("old "));
    assert_eq!(prepared.snapshot.scope.actor_id, material.contact.actor.id);
    assert_eq!(
        prepared.snapshot.scope.conversation_id,
        material.conversation.id
    );
    assert_eq!(prepared.snapshot.profile_revision, Counter::new(3).unwrap());
    assert_eq!(prepared.snapshot.binding_revision, Counter::new(4).unwrap());
    assert_eq!(
        prepared.snapshot.sources,
        vec![SourceRef::Message {
            conversation_id: material.conversation.id,
            message_id: original[1].id,
            revision: original[1].revision
        }]
    );
    assert_eq!(prepared.snapshot.dependencies.len(), 2);
    assert!(
        prepared
            .snapshot
            .dependencies
            .iter()
            .all(|d| d.context_epoch == material.conversation.context_epoch)
    );
}

#[test]
fn explicit_memory_is_budgeted_background_and_snapshot_tracks_only_selected_entries() {
    let (mut material, mut model) = fixture();
    model.context_window = Some(8192);
    let time = material.contact.actor.created_at;
    let memory = MemoryRecord {
        id: MemoryId::new(),
        owner_actor_id: material.contact.actor.id,
        conversation_id: material.conversation.id,
        scope: MemoryScope::Relationship {
            relationship_id: material.conversation.relationship_id.unwrap(),
        },
        kind: MemoryKind::Preference,
        text: "prefer short paragraphs".into(),
        status: MaterialStatus::Confirmed,
        revision: Revision::ONE,
        sources: vec![],
        created_by: material.members[0].actor_id,
        updated_by: material.members[0].actor_id,
        created_at: time,
        updated_at: time,
    };
    let mut large = memory.clone();
    large.id = MemoryId::new();
    large.text = "oversized memory".repeat(1000);
    material.memories = vec![large.clone(), memory.clone()];
    let prepared = ConsciousnessService::prepare(material.clone(), &model, 128).unwrap();
    assert_eq!(prepared.messages[1].role, common::Role::User);
    assert!(prepared.messages[1].text().contains(&memory.text));
    assert!(!prepared.messages[0].text().contains(&memory.text));
    assert!(prepared.snapshot.sources.contains(&SourceRef::Memory {
        memory_id: memory.id,
        revision: memory.revision
    }));
    assert!(
        !prepared
            .snapshot
            .sources
            .iter()
            .any(|s| matches!(s, SourceRef::Memory { memory_id, .. } if *memory_id == large.id))
    );
    assert!(
        prepared
            .messages
            .iter()
            .any(|m| m.text().contains("latest"))
    );
    // The memory belongs to the account; changing the execution model retains it.
    material.contact.binding.as_mut().unwrap().model_id = "replacement".into();
    let changed = ConsciousnessService::prepare(material, &model, 128).unwrap();
    assert!(
        changed
            .messages
            .iter()
            .any(|m| m.text().contains(&memory.text))
    );
}

#[test]
fn oversized_latest_message_is_rejected_instead_of_silently_truncated() {
    let (mut material, model) = fixture();
    material.messages.last_mut().unwrap().body = MessageBody::text("large".repeat(1000));
    assert!(matches!(
        ConsciousnessService::prepare(material, &model, 128),
        Err(SocialError::BudgetExceeded)
    ));
}

#[test]
fn group_context_keeps_trigger_and_distinguishes_other_ai_from_self() {
    let (mut material, mut model) = fixture();
    material.conversation.kind = ConversationKind::Group;
    material.conversation.relationship_id = None;
    material.relationship_revision = None;
    material.participant_names.push("Other AI".into());
    let mut answer = material.messages.last().unwrap().clone();
    answer.id = MessageId::new();
    answer.sequence = answer.sequence.next().unwrap();
    answer.sender_id = ActorId::new();
    answer.sender_name = "Other AI".into();
    answer.body = MessageBody::text("a previous group answer");
    material.messages.push(answer.clone());
    model.context_window = Some(2048);
    let prepared = ConsciousnessService::prepare(material.clone(), &model, 128).unwrap();
    assert!(prepared.messages[0].text().contains("群聊"));
    assert!(
        prepared.messages[0]
            .text()
            .contains("当前成员：Human、A、Other AI")
    );
    let other = prepared.messages.last().unwrap();
    assert_eq!(other.role, common::Role::User);
    assert!(other.text().contains("Other AI"));
    assert!(prepared.snapshot.sources.iter().any(
        |s| matches!(s,SourceRef::Message {message_id,..} if *message_id==material.trigger.message_id().unwrap())
    ));
    material.messages.last_mut().unwrap().body = MessageBody::text("huge".repeat(1000));
    let prepared = ConsciousnessService::prepare(material, &model, 128).unwrap();
    assert_eq!(
        prepared.snapshot.sources.len(),
        1,
        "the human trigger remains even when the prior AI answer cannot fit"
    );
}

#[test]
fn explicit_reply_reserves_its_visible_source_and_identifies_the_reference() {
    let (mut material, model) = fixture();
    let quoted = material.messages[0].id;
    material.messages[0].body = MessageBody::text("an earlier point");
    material.messages[1].reply_to = Some(quoted);
    let prepared = ConsciousnessService::prepare(material.clone(), &model, 128).unwrap();
    assert!(
        prepared
            .messages
            .last()
            .unwrap()
            .text()
            .contains("回复第 1 条消息")
    );
    assert!(
        prepared
            .snapshot
            .sources
            .iter()
            .any(|s| matches!(s,SourceRef::Message {message_id,..} if *message_id==quoted))
    );
    material.messages[0].body = MessageBody::text("huge quote".repeat(1000));
    assert!(matches!(
        ConsciousnessService::prepare(material, &model, 128),
        Err(SocialError::BudgetExceeded)
    ));
}

#[test]
fn explicit_output_budget_is_reserved_and_large_output_limits_are_not_clamped() {
    let (material, mut model) = fixture();
    model.context_window = Some(8192);
    model.max_tokens = Some(6000);
    let prepared = ConsciousnessService::prepare(material.clone(), &model, 4096).unwrap();
    assert_eq!(prepared.snapshot.budget.reserved_output_tokens.get(), 4096);
    assert_eq!(prepared.snapshot.budget.input_token_limit.get(), 4096);
    // The original history remains complete even when output reservation excludes older input.
    assert_eq!(material.messages.len(), 2);
    assert_eq!(prepared.snapshot.sources.len(), 1);
    assert!(matches!(
        ConsciousnessService::prepare(material, &model, 8000),
        Err(SocialError::BudgetExceeded)
    ));
}

#[test]
fn cross_window_material_is_labeled_bounded_and_snapshot_records_only_actual_reads() {
    let (mut material, mut model) = fixture();
    model.context_window = Some(8192);
    let mut group = material.conversation.clone();
    group.id = ConversationId::new();
    group.kind = ConversationKind::Group;
    group.title = "source group".into();
    let mut original = material.messages.last().unwrap().clone();
    original.id = MessageId::new();
    original.conversation_id = group.id;
    original.body = MessageBody::text("selected group point");
    let dependency = VisibilityDependency {
        conversation_id: group.id,
        actor_id: material.contact.actor.id,
        membership_revision: Revision::ONE,
        context_epoch: group.context_epoch,
    };
    material.reference = Some(storage::social::ReferenceMaterial {
        message_id: material.trigger.message_id().unwrap(),
        revision: Revision::ONE,
        required: true,
        preview: storage::social::ReferencePreview {
            conversation: group.clone(),
            messages: vec![original.clone()],
            summary: None,
            dependencies: vec![dependency.clone()],
        },
    });
    let prepared = ConsciousnessService::prepare(material.clone(), &model, 128).unwrap();
    assert!(prepared.messages[1].text().contains("source group"));
    assert!(prepared.messages[1].text().contains("selected group point"));
    assert_eq!(prepared.messages[1].role, common::Role::User);
    assert!(prepared.snapshot.dependencies.contains(&dependency));
    assert!(prepared.snapshot.sources.iter().any(|s|matches!(s, SourceRef::Message { message_id, conversation_id, .. } if *message_id == original.id && *conversation_id == group.id)));
    assert!(prepared.snapshot.sources.iter().any(|s|matches!(s, SourceRef::Reference { message_id, .. } if *message_id == material.trigger.message_id().unwrap())));
    material.reference.as_mut().unwrap().preview.messages[0].body =
        MessageBody::text("oversized group source".repeat(1000));
    assert!(matches!(
        ConsciousnessService::prepare(material.clone(), &model, 128),
        Err(SocialError::BudgetExceeded)
    ));
    material.reference.as_mut().unwrap().required = false;
    let skipped = ConsciousnessService::prepare(material, &model, 128).unwrap();
    assert!(!skipped.snapshot.dependencies.contains(&dependency));
    assert!(
        !skipped
            .snapshot
            .sources
            .iter()
            .any(|s| matches!(s, SourceRef::Reference { .. }))
    );
}
