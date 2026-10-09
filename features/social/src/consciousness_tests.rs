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
    let messages = ["old ".repeat(1000), "latest".into()]
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
            run_id: None,
            status: MessageStatus::Committed,
            revision: Counter::new(6).unwrap(),
            created_at: "2026-10-09T00:00:00Z".parse().unwrap(),
        })
        .collect();
    let material = ContextMaterial {
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
