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
            history_from_seq: Sequence::ZERO,
            revision: Counter::new(5).unwrap(),
        })
        .collect();
    let messages: Vec<SocialMessage> = ["old ".repeat(1000), "latest".into()]
        .into_iter()
        .enumerate()
        .map(|(i, text)| SocialMessage {
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
        participant_names: vec!["Human".into(), "A".into()],
        trigger_id,
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
fn context_selects_recent_sources_with_versions_without_rewriting_history() {
    let (material, model) = fixture();
    let original = material.messages.clone();
    let prepared = ConsciousnessService::prepare(material.clone(), &model).unwrap();
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
fn oversized_latest_message_is_rejected_instead_of_silently_truncated() {
    let (mut material, model) = fixture();
    material.messages.last_mut().unwrap().body = MessageBody::text("large".repeat(1000));
    assert!(matches!(
        ConsciousnessService::prepare(material, &model),
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
    let prepared = ConsciousnessService::prepare(material.clone(), &model).unwrap();
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
        |s| matches!(s,SourceRef::Message {message_id,..} if *message_id==material.trigger_id)
    ));
    material.messages.last_mut().unwrap().body = MessageBody::text("huge".repeat(1000));
    let prepared = ConsciousnessService::prepare(material, &model).unwrap();
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
    let prepared = ConsciousnessService::prepare(material.clone(), &model).unwrap();
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
        ConsciousnessService::prepare(material, &model),
        Err(SocialError::BudgetExceeded)
    ));
}
