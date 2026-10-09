use chrono::{TimeZone, Utc};
use common::{ContentPart, Message, SessionId, ToolCall};
use rusqlite::{Connection, params};
use social::*;
use std::{
    collections::HashMap,
    fs,
    path::Path,
    sync::Arc,
    time::{Duration, Instant},
};
use storage::social::*;
use storage::{DataDirectoryGuard, FileSessionStore, SessionRecord, SessionStore};

struct Fixture {
    dir: tempfile::TempDir,
    store: SqliteSocialStore,
    human: Actor,
    clock: Arc<ManualClock>,
}
impl Fixture {
    async fn group(&self, members: Vec<ActorId>) -> Conversation {
        self.store
            .create_group(self.ctx(), "Group".into(), members)
            .await
            .unwrap()
    }
    async fn send_group(&self, c: ConversationId, targets: Vec<ActorId>) -> SavedMessage {
        self.store
            .send_message(
                self.ctx(),
                SendMessage {
                    conversation_id: c,
                    body: MessageBody::text("trigger"),
                    reply_to: None,
                    mentions: targets,
                },
            )
            .await
            .unwrap()
    }
    async fn begin_plan(&self, p: &ParticipationPlan) -> Result<AiRun, SocialStorageError> {
        let material = self
            .store
            .context_material(
                self.human.id,
                ContextScope {
                    actor_id: p.actor_id,
                    conversation_id: p.conversation_id,
                },
                p.trigger_id,
            )
            .await?;
        let snapshot = ContextSnapshot {
            scope: ContextScope {
                actor_id: p.actor_id,
                conversation_id: p.conversation_id,
            },
            profile_revision: material.contact.profile.revision,
            binding_revision: material.contact.binding.unwrap().revision,
            relationship_revision: material.relationship_revision,
            budget: ContextBudget {
                input_token_limit: Counter::new(8192).unwrap(),
                reserved_output_tokens: Counter::new(1024).unwrap(),
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
            sources: material
                .messages
                .iter()
                .map(|m| SourceRef::Message {
                    conversation_id: m.conversation_id,
                    message_id: m.id,
                    revision: m.revision,
                })
                .collect(),
        };
        self.store
            .begin_plan(
                self.human.id,
                p.id,
                BeginRun {
                    snapshot,
                    expected_revision: material.conversation.revision,
                    details: None,
                },
            )
            .await
    }
    async fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let clock = Arc::new(ManualClock::new(
            Utc.with_ymd_and_hms(2026, 10, 9, 0, 0, 0).unwrap(),
        ));
        let store = SqliteSocialStore::open(
            DataDirectoryGuard::acquire(dir.path()).unwrap(),
            clock.clone(),
        )
        .await
        .unwrap();
        let human = store.local_human("我".into()).await.unwrap();
        Self {
            dir,
            store,
            human,
            clock,
        }
    }
    fn ctx(&self) -> CommandContext {
        CommandContext {
            operator: self.human.id,
            request_id: CommandId::new(),
        }
    }
    fn sql(&self) -> Connection {
        Connection::open(self.store.path()).unwrap()
    }
    async fn ai(&self, name: &str) -> Actor {
        self.store
            .create_ai(
                self.ctx(),
                CreateAi {
                    display_name: name.into(),
                    persona: "角色设定".into(),
                    binding: Some(BindingCandidate {
                        provider_id: "mock".into(),
                        model_id: "same-model".into(),
                        reasoning_effort: None,
                    }),
                },
            )
            .await
            .unwrap()
    }
    async fn direct(&self, ai: ActorId) -> Conversation {
        self.store.open_direct(self.ctx(), ai).await.unwrap()
    }
    async fn append(&self, c: ConversationId, text: &str) -> SocialMessage {
        let c = self.store.conversation(self.human.id, c).await.unwrap();
        self.store
            .append_message(
                self.ctx(),
                AppendMessage {
                    conversation_id: c.id,
                    body: MessageBody::text(text),
                    reply_to: None,
                    expected_revision: Some(c.revision),
                },
            )
            .await
            .unwrap()
    }
    async fn snapshot(
        &self,
        ai: ActorId,
        c: ConversationId,
        sources: Vec<SourceRef>,
    ) -> ContextSnapshot {
        let conversation = self.store.conversation(self.human.id, c).await.unwrap();
        let mut dependencies = vec![
            VisibilityDependency {
                conversation_id: c,
                actor_id: self.human.id,
                membership_revision: Revision::ONE,
                context_epoch: conversation.context_epoch,
            },
            VisibilityDependency {
                conversation_id: c,
                actor_id: ai,
                membership_revision: Revision::ONE,
                context_epoch: conversation.context_epoch,
            },
        ];
        for source in &sources {
            if let SourceRef::Message {
                conversation_id, ..
            } = source
            {
                if *conversation_id == c {
                    continue;
                }
                let source = self
                    .store
                    .conversation(self.human.id, *conversation_id)
                    .await
                    .unwrap();
                for actor in [self.human.id, ai] {
                    dependencies.push(VisibilityDependency {
                        conversation_id: source.id,
                        actor_id: actor,
                        membership_revision: Revision::ONE,
                        context_epoch: source.context_epoch,
                    });
                }
            }
        }
        ContextSnapshot {
            scope: ContextScope {
                actor_id: ai,
                conversation_id: c,
            },
            profile_revision: Revision::ONE,
            binding_revision: Revision::ONE,
            relationship_revision: conversation.relationship_id.map(|_| Revision::ONE),
            budget: ContextBudget {
                input_token_limit: Counter::new(1000).unwrap(),
                reserved_output_tokens: Counter::new(100).unwrap(),
            },
            participation_epoch: conversation.participation_epoch,
            dependencies,
            sources,
        }
    }
    async fn begin(&self, ai: ActorId, c: ConversationId, sources: Vec<SourceRef>) -> AiRun {
        let snapshot = self.snapshot(ai, c, sources).await;
        let c = self.store.conversation(self.human.id, c).await.unwrap();
        self.store
            .begin_run(
                self.ctx(),
                BeginRun {
                    details: None,
                    snapshot,
                    expected_revision: c.revision,
                },
            )
            .await
            .unwrap()
    }
    fn counts(&self) -> Vec<i64> {
        let sql = self.sql();
        [
            "actors",
            "ai_profiles",
            "execution_bindings",
            "relationships",
            "conversations",
            "memberships",
            "messages",
            "message_revisions",
            "bouts",
            "ai_runs",
            "committed_events",
            "command_receipts",
            "legacy_imports",
            "legacy_entries",
            "legacy_id_map",
        ]
        .iter()
        .map(|t| {
            sql.query_row(&format!("SELECT count(*) FROM {t}"), [], |r| r.get(0))
                .unwrap()
        })
        .collect()
    }
    fn inject(&self) {
        self.sql().execute_batch("CREATE TRIGGER fail_receipt BEFORE INSERT ON command_receipts BEGIN SELECT RAISE(ABORT,'injected receipt failure'); END;").unwrap();
    }
    fn clear(&self) {
        self.sql()
            .execute_batch("DROP TRIGGER fail_receipt")
            .unwrap();
    }
    async fn close(self) {
        self.store.close().await.unwrap();
        drop(self.dir);
    }
}

#[tokio::test]
async fn group_queue_is_atomic_idempotent_fifo_and_shares_only_its_committed_answers() {
    let f = Fixture::new().await;
    let a = f.ai("A").await;
    let b = f.ai("B").await;
    let c = f.group(vec![a.id, b.id]).await;
    let ctx = f.ctx();
    let input = SendMessage {
        conversation_id: c.id,
        body: MessageBody::text("first"),
        reply_to: None,
        mentions: vec![b.id, a.id, b.id],
    };
    let first = f
        .store
        .send_message(ctx.clone(), input.clone())
        .await
        .unwrap();
    let retry = f.store.send_message(ctx, input).await.unwrap();
    assert_eq!(first.message.id, retry.message.id);
    assert_eq!(first.plans, retry.plans);
    assert_eq!(first.message.mentions, vec![b.id, a.id]);
    let second = f.send_group(c.id, vec![a.id]).await;
    assert_eq!(
        f.store
            .next_plan(f.human.id, c.id)
            .await
            .unwrap()
            .unwrap()
            .actor_id,
        b.id
    );
    assert!(matches!(
        f.begin_plan(&first.plans[1]).await,
        Err(SocialStorageError::Conflict(_))
    ));
    let run = f.begin_plan(&first.plans[0]).await.unwrap();
    assert!(f.store.next_plan(f.human.id, c.id).await.unwrap().is_none());
    assert!(matches!(
        f.begin_plan(&first.plans[0]).await,
        Err(SocialStorageError::Conflict(_))
    ));
    f.store
        .finish_run(
            f.ctx(),
            FinishRun {
                run_id: run.id,
                expected_revision: run.revision,
                status: RunStatus::Succeeded,
                body: MessageBody::text("B answered first"),
                details: None,
            },
        )
        .await
        .unwrap();
    let material = f
        .store
        .context_material(
            f.human.id,
            ContextScope {
                actor_id: a.id,
                conversation_id: c.id,
            },
            first.message.id,
        )
        .await
        .unwrap();
    assert!(material.messages.iter().any(|m| m.id == run.message_id));
    assert!(
        !material.messages.iter().any(|m| m.id == second.message.id),
        "future human input must not change which message is answered"
    );
    let run_a = f.begin_plan(&first.plans[1]).await.unwrap();
    f.store
        .finish_run(
            f.ctx(),
            FinishRun {
                run_id: run_a.id,
                expected_revision: run_a.revision,
                status: RunStatus::Failed,
                body: MessageBody::text("partial"),
                details: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(
        f.store
            .next_plan(f.human.id, c.id)
            .await
            .unwrap()
            .unwrap()
            .id,
        second.plans[0].id,
        "one failure does not block the next human message"
    );
    let history = f
        .store
        .messages(f.human.id, c.id, Sequence::ZERO, 100)
        .await
        .unwrap();
    assert_eq!(
        history
            .iter()
            .find(|m| m.id == run.message_id)
            .unwrap()
            .reply_to,
        Some(first.message.id)
    );
    assert_eq!(
        f.store
            .plans(f.human.id, c.id)
            .await
            .unwrap()
            .iter()
            .find(|p| p.id == first.plans[1].id)
            .unwrap()
            .status,
        PlanStatus::Failed
    );
    f.store
        .end_bout(f.ctx(), run_a.bout_id, Revision::ONE)
        .await
        .unwrap();
    assert!(f.store.next_plan(f.human.id, c.id).await.unwrap().is_none());
    assert_eq!(
        f.store
            .plans(f.human.id, c.id)
            .await
            .unwrap()
            .iter()
            .find(|p| p.id == second.plans[0].id)
            .unwrap()
            .status,
        PlanStatus::Cancelled,
        "ending the bout revokes unstarted replies"
    );
    f.store.close().await.unwrap();
}

#[tokio::test]
async fn group_shared_history_removal_and_plan_failure_rollback_preserve_original_messages() {
    let f = Fixture::new().await;
    let a = f.ai("A").await;
    let b = f.ai("B").await;
    let c = f.group(vec![a.id]).await;
    let old = f.send_group(c.id, vec![]).await.message;
    assert!(f.store.plans(f.human.id, c.id).await.unwrap().is_empty());
    let c = f.store.conversation(f.human.id, c.id).await.unwrap();
    f.store
        .change_membership(
            f.ctx(),
            ChangeMembership {
                conversation_id: c.id,
                actor_id: b.id,
                expected_revision: c.revision,
                action: MembershipAction::Add,
            },
        )
        .await
        .unwrap();
    let input = SendMessage {
        conversation_id: c.id,
        body: MessageBody::text("rollback"),
        reply_to: None,
        mentions: vec![b.id],
    };
    let ctx = f.ctx();
    let cursor = f.store.overview(f.human.id).await.unwrap().cursor;
    f.sql().execute_batch("CREATE TRIGGER reject_plan BEFORE INSERT ON participation_plans BEGIN SELECT RAISE(ABORT,'synthetic'); END;").unwrap();
    assert!(
        f.store
            .send_message(ctx.clone(), input.clone())
            .await
            .is_err()
    );
    assert_eq!(f.store.overview(f.human.id).await.unwrap().cursor, cursor);
    assert_eq!(
        f.store
            .messages(f.human.id, c.id, Sequence::ZERO, 100)
            .await
            .unwrap()
            .len(),
        1
    );
    f.sql().execute_batch("DROP TRIGGER reject_plan").unwrap();
    let saved = f.store.send_message(ctx, input).await.unwrap();
    let scope = ContextScope {
        actor_id: b.id,
        conversation_id: c.id,
    };
    let material = f
        .store
        .context_material(f.human.id, scope, saved.message.id)
        .await
        .unwrap();
    assert!(material.messages.iter().any(|m| m.id == old.id));
    assert!(
        f.store
            .context_material(f.human.id, scope, old.id)
            .await
            .is_ok()
    );
    let run = f.begin_plan(&saved.plans[0]).await.unwrap();
    let waiting = f.send_group(c.id, vec![b.id, a.id]).await;
    let c = f.store.conversation(f.human.id, c.id).await.unwrap();
    f.store
        .change_membership(
            f.ctx(),
            ChangeMembership {
                conversation_id: c.id,
                actor_id: b.id,
                expected_revision: c.revision,
                action: MembershipAction::Remove,
            },
        )
        .await
        .unwrap();
    assert_eq!(
        f.store.run(f.human.id, run.id).await.unwrap().status,
        RunStatus::Cancelled
    );
    let plans = f.store.plans(f.human.id, c.id).await.unwrap();
    assert_eq!(
        plans
            .iter()
            .find(|p| p.id == waiting.plans[0].id)
            .unwrap()
            .status,
        PlanStatus::Cancelled
    );
    assert_eq!(
        f.store
            .next_plan(f.human.id, c.id)
            .await
            .unwrap()
            .unwrap()
            .id,
        waiting.plans[1].id
    );
    assert!(matches!(
        f.store.context_material(f.human.id, scope, old.id).await,
        Err(SocialStorageError::Forbidden(_))
    ));
    let c = f.store.conversation(f.human.id, c.id).await.unwrap();
    f.store
        .change_membership(
            f.ctx(),
            ChangeMembership {
                conversation_id: c.id,
                actor_id: b.id,
                expected_revision: c.revision,
                action: MembershipAction::Add,
            },
        )
        .await
        .unwrap();
    let new = f.send_group(c.id, vec![b.id]).await;
    assert!(
        f.store
            .context_material(f.human.id, scope, new.message.id)
            .await
            .unwrap()
            .messages
            .iter()
            .any(|m| m.id == old.id)
    );
    assert_eq!(
        f.store
            .plans(f.human.id, c.id)
            .await
            .unwrap()
            .iter()
            .find(|p| p.id == waiting.plans[0].id)
            .unwrap()
            .status,
        PlanStatus::Cancelled,
        "rejoining never revives revoked plans"
    );
    f.store.close().await.unwrap();
}

#[tokio::test]
async fn group_recovery_keeps_unstarted_plans_and_never_replays_started_runs() {
    let f = Fixture::new().await;
    let a = f.ai("A").await;
    let b = f.ai("B").await;
    let c = f.group(vec![a.id, b.id]).await;
    let saved = f.send_group(c.id, vec![a.id, b.id]).await;
    let run = f.begin_plan(&saved.plans[0]).await.unwrap();
    f.store
        .checkpoint_run(f.human.id, run.id, MessageBody::text("saved partial"), None)
        .await
        .unwrap();
    f.store.close().await.unwrap();
    let store =
        SqliteSocialStore::open(DataDirectoryGuard::acquire(f.dir.path()).unwrap(), f.clock)
            .await
            .unwrap();
    let recovered = store.run(f.human.id, run.id).await.unwrap();
    assert_eq!(recovered.status, RunStatus::Interrupted);
    let plans = store.plans(f.human.id, c.id).await.unwrap();
    assert_eq!(
        plans
            .iter()
            .find(|p| p.id == saved.plans[0].id)
            .unwrap()
            .status,
        PlanStatus::Interrupted
    );
    assert_eq!(
        store.next_plan(f.human.id, c.id).await.unwrap().unwrap().id,
        saved.plans[1].id
    );
    assert_eq!(
        store.queued_conversations(f.human.id).await.unwrap(),
        vec![c.id]
    );
    assert_eq!(
        store
            .messages(f.human.id, c.id, Sequence::ZERO, 100)
            .await
            .unwrap()
            .last()
            .unwrap()
            .body,
        MessageBody::text("saved partial")
    );
    store.close().await.unwrap();
}
fn source(m: &SocialMessage) -> SourceRef {
    SourceRef::Message {
        conversation_id: m.conversation_id,
        message_id: m.id,
        revision: m.revision,
    }
}

#[tokio::test]
async fn identities_direct_uniqueness_archive_restore_and_clock() {
    let f = Fixture::new().await;
    assert_eq!(f.human.created_at, f.clock.now());
    assert_eq!(
        f.store.local_human("another".into()).await.unwrap().id,
        f.human.id
    );
    let ai = f.ai("小林").await;
    let other = f.ai("小林").await;
    assert_ne!(ai.id, other.id);
    let c = f.direct(ai.id).await;
    assert_eq!(f.direct(ai.id).await.id, c.id);
    assert_ne!(f.direct(other.id).await.id, c.id);
    let g = f
        .store
        .create_group(f.ctx(), "群".into(), vec![ai.id, other.id])
        .await
        .unwrap();
    assert_ne!(g.id, c.id);
    let archived = f
        .store
        .control_conversation(
            f.ctx(),
            ConversationControl {
                conversation_id: c.id,
                expected_revision: c.revision,
                status: EntityStatus::Archived,
                paused: true,
            },
        )
        .await
        .unwrap();
    assert_eq!(f.direct(ai.id).await, archived);
    assert!(
        f.store
            .append_message(
                f.ctx(),
                AppendMessage {
                    conversation_id: c.id,
                    expected_revision: Some(archived.revision),
                    body: MessageBody::text("read-only"),
                    reply_to: None
                }
            )
            .await
            .is_err()
    );
    let restored = f
        .store
        .control_conversation(
            f.ctx(),
            ConversationControl {
                conversation_id: c.id,
                expected_revision: archived.revision,
                status: EntityStatus::Active,
                paused: false,
            },
        )
        .await
        .unwrap();
    assert_eq!(restored.id, c.id);
    let m = f.append(c.id, "first").await;
    f.clock
        .set(Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap());
    let n = f.append(c.id, "second").await;
    assert!(n.created_at < m.created_at);
    assert!(n.sequence > m.sequence);
    assert_eq!(
        f.store
            .messages(f.human.id, c.id, m.sequence, 1)
            .await
            .unwrap(),
        vec![n]
    );
    assert!(
        f.store
            .messages(ai.id, c.id, Sequence::ZERO, 20)
            .await
            .is_err()
    );
    f.close().await;
}

#[tokio::test]
async fn receipt_returns_current_revision_and_rejects_changed_input() {
    let f = Fixture::new().await;
    let ai = f.ai("AI").await;
    let c = f.direct(ai.id).await;
    let ctx = f.ctx();
    let input = AppendMessage {
        conversation_id: c.id,
        body: MessageBody::text("private-original"),
        reply_to: None,
        expected_revision: Some(c.revision),
    };
    let (first, retry) = tokio::join!(
        f.store.append_message(ctx.clone(), input.clone()),
        f.store.append_message(ctx.clone(), input.clone())
    );
    let m = first.unwrap();
    assert_eq!(m, retry.unwrap());
    let revised = f
        .store
        .revise_message(
            f.ctx(),
            ReviseMessage {
                message_id: m.id,
                body: MessageBody::text("corrected"),
                expected_revision: m.revision,
            },
        )
        .await
        .unwrap();
    assert_eq!(
        f.store
            .append_message(ctx.clone(), input.clone())
            .await
            .unwrap(),
        revised
    );
    let mut changed = input;
    changed.body = MessageBody::text("different");
    assert!(matches!(
        f.store.append_message(ctx, changed).await,
        Err(SocialStorageError::Conflict(_))
    ));
    let events = serde_json::to_string(
        &f.store
            .events(f.human.id, EventCursor::ZERO, 500)
            .await
            .unwrap(),
    )
    .unwrap();
    assert!(!events.contains("private-original"));
    assert!(!events.contains("corrected"));
    let receipts: String = f
        .sql()
        .query_row(
            "SELECT group_concat(digest || entity_id) FROM command_receipts",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(!receipts.contains("private-original"));
    assert_eq!(
        f.store
            .messages(f.human.id, c.id, Sequence::ZERO, 200)
            .await
            .unwrap()
            .len(),
        1
    );
    f.close().await;
}

#[tokio::test]
async fn every_command_rolls_back_entities_events_and_receipts() {
    let f = Fixture::new().await;
    let ai = f.ai("AI").await;
    let c = f.direct(ai.id).await;
    let m = f.append(c.id, "message").await;
    let before = f.counts();
    f.inject();
    assert!(
        f.store
            .create_ai(
                f.ctx(),
                CreateAi {
                    display_name: "failed".into(),
                    persona: String::new(),
                    binding: None
                }
            )
            .await
            .is_err()
    );
    assert!(
        f.store
            .create_group(f.ctx(), "failed".into(), vec![ai.id])
            .await
            .is_err()
    );
    let current = f.store.conversation(f.human.id, c.id).await.unwrap();
    assert!(
        f.store
            .append_message(
                f.ctx(),
                AppendMessage {
                    conversation_id: c.id,
                    body: MessageBody::text("failed"),
                    reply_to: None,
                    expected_revision: Some(current.revision)
                }
            )
            .await
            .is_err()
    );
    assert!(
        f.store
            .revise_message(
                f.ctx(),
                ReviseMessage {
                    message_id: m.id,
                    body: MessageBody::text("failed"),
                    expected_revision: m.revision
                }
            )
            .await
            .is_err()
    );
    assert!(
        f.store
            .control_conversation(
                f.ctx(),
                ConversationControl {
                    conversation_id: c.id,
                    expected_revision: current.revision,
                    status: EntityStatus::Archived,
                    paused: true
                }
            )
            .await
            .is_err()
    );
    let snapshot = f.snapshot(ai.id, c.id, vec![source(&m)]).await;
    assert!(
        f.store
            .begin_run(
                f.ctx(),
                BeginRun {
                    details: None,
                    snapshot,
                    expected_revision: current.revision
                }
            )
            .await
            .is_err()
    );
    assert_eq!(f.counts(), before);
    assert_eq!(
        f.store.conversation(f.human.id, c.id).await.unwrap(),
        current
    );
    assert_eq!(
        f.store
            .messages(f.human.id, c.id, Sequence::ZERO, 20)
            .await
            .unwrap(),
        vec![m.clone()]
    );
    f.clear();
    let run = f.begin(ai.id, c.id, vec![source(&m)]).await;
    let before_history = f
        .store
        .messages(f.human.id, c.id, Sequence::ZERO, 200)
        .await
        .unwrap();
    let before = f.counts();
    f.inject();
    assert!(
        f.store
            .finish_run(
                f.ctx(),
                FinishRun {
                    details: None,
                    run_id: run.id,
                    expected_revision: run.revision,
                    status: RunStatus::Succeeded,
                    body: MessageBody::text("failed")
                }
            )
            .await
            .is_err()
    );
    assert!(
        f.store
            .finish_run(
                f.ctx(),
                FinishRun {
                    details: None,
                    run_id: run.id,
                    expected_revision: run.revision,
                    status: RunStatus::Cancelled,
                    body: MessageBody::text("failed")
                }
            )
            .await
            .is_err()
    );
    assert!(
        f.store
            .end_bout(f.ctx(), run.bout_id, Revision::ONE)
            .await
            .is_err()
    );
    assert_eq!(f.counts(), before);
    let persisted: AiRun = serde_json::from_str(
        &f.sql()
            .query_row::<String, _, _>(
                "SELECT payload FROM ai_runs WHERE id=?1",
                [run.id.to_string()],
                |r| r.get(0),
            )
            .unwrap(),
    )
    .unwrap();
    assert_eq!(persisted, run);
    assert_eq!(
        f.store
            .messages(f.human.id, c.id, Sequence::ZERO, 200)
            .await
            .unwrap(),
        before_history
    );
    f.clear();
    f.sql().execute_batch("CREATE TRIGGER fail_checkpoint BEFORE UPDATE ON messages BEGIN SELECT RAISE(ABORT,'injected draft failure'); END").unwrap();
    assert!(
        f.store
            .checkpoint_run(f.human.id, run.id, MessageBody::text("failed draft"), None)
            .await
            .is_err()
    );
    assert_eq!(
        f.store
            .messages(f.human.id, c.id, Sequence::ZERO, 200)
            .await
            .unwrap(),
        before_history
    );
    f.sql()
        .execute_batch("DROP TRIGGER fail_checkpoint")
        .unwrap();
    f.close().await;
}

#[tokio::test]
async fn stop_finish_competition_cross_window_versions_and_continuous_input() {
    let f = Fixture::new().await;
    let ai = f.ai("AI").await;
    let other = f.ai("Other").await;
    let c = f.direct(ai.id).await;
    let g = f
        .store
        .create_group(f.ctx(), "shared".into(), vec![ai.id])
        .await
        .unwrap();
    let gm = f.append(g.id, "group topic").await;
    let run = f.begin(ai.id, c.id, vec![source(&gm)]).await;
    f.append(c.id, "still sending during generation").await;
    assert!(
        f.store
            .begin_run(
                f.ctx(),
                BeginRun {
                    details: None,
                    snapshot: f.snapshot(ai.id, c.id, vec![]).await,
                    expected_revision: f
                        .store
                        .conversation(f.human.id, c.id)
                        .await
                        .unwrap()
                        .revision
                }
            )
            .await
            .is_err()
    );
    f.store
        .revise_message(
            f.ctx(),
            ReviseMessage {
                message_id: gm.id,
                body: MessageBody::text("changed"),
                expected_revision: gm.revision,
            },
        )
        .await
        .unwrap();
    assert!(
        f.store
            .finish_run(
                f.ctx(),
                FinishRun {
                    details: None,
                    run_id: run.id,
                    expected_revision: run.revision,
                    status: RunStatus::Succeeded,
                    body: MessageBody::text("late")
                }
            )
            .await
            .is_err()
    );
    f.store
        .finish_run(
            f.ctx(),
            FinishRun {
                details: None,
                run_id: run.id,
                expected_revision: run.revision,
                status: RunStatus::Interrupted,
                body: MessageBody::text("draft"),
            },
        )
        .await
        .unwrap();
    let run = f.begin(ai.id, c.id, vec![]).await;
    let current = f.store.conversation(f.human.id, c.id).await.unwrap();
    f.store
        .control_conversation(
            f.ctx(),
            ConversationControl {
                conversation_id: c.id,
                expected_revision: current.revision,
                status: EntityStatus::Active,
                paused: true,
            },
        )
        .await
        .unwrap();
    assert!(
        f.store
            .finish_run(
                f.ctx(),
                FinishRun {
                    details: None,
                    run_id: run.id,
                    expected_revision: run.revision,
                    status: RunStatus::Succeeded,
                    body: MessageBody::text("late")
                }
            )
            .await
            .is_err()
    );
    f.append(c.id, "human allowed while paused").await;
    let current = f.store.conversation(f.human.id, c.id).await.unwrap();
    f.store
        .control_conversation(
            f.ctx(),
            ConversationControl {
                conversation_id: c.id,
                expected_revision: current.revision,
                status: EntityStatus::Active,
                paused: false,
            },
        )
        .await
        .unwrap();
    let run = f.begin(ai.id, c.id, vec![]).await;
    let ctx = f.ctx();
    let finish = FinishRun {
        details: None,
        run_id: run.id,
        expected_revision: run.revision,
        status: RunStatus::Succeeded,
        body: MessageBody::text("complete"),
    };
    let succeeded = f
        .store
        .finish_run(ctx.clone(), finish.clone())
        .await
        .unwrap();
    assert_eq!(f.store.finish_run(ctx, finish).await.unwrap(), succeeded);
    let run = f.begin(ai.id, c.id, vec![]).await;
    f.store
        .end_bout(f.ctx(), run.bout_id, Revision::ONE)
        .await
        .unwrap();
    assert!(
        f.store
            .finish_run(
                f.ctx(),
                FinishRun {
                    details: None,
                    run_id: run.id,
                    expected_revision: run.revision,
                    status: RunStatus::Succeeded,
                    body: MessageBody::text("late")
                }
            )
            .await
            .is_err()
    );
    let private = f.direct(other.id).await;
    let private_m = f.append(private.id, "not shared with target AI").await;
    assert!(
        f.store
            .begin_run(
                f.ctx(),
                BeginRun {
                    details: None,
                    snapshot: f.snapshot(ai.id, c.id, vec![source(&private_m)]).await,
                    expected_revision: f
                        .store
                        .conversation(f.human.id, c.id)
                        .await
                        .unwrap()
                        .revision
                }
            )
            .await
            .is_err()
    );
    f.close().await;
}

#[tokio::test]
async fn wal_backup_busy_and_foreign_key_constraints() {
    let f = Fixture::new().await;
    let ai = f.ai("AI").await;
    let c = f.direct(ai.id).await;
    f.append(c.id, "in WAL").await;
    let sql = f.sql();
    assert_eq!(
        sql.query_row::<String, _, _>("PRAGMA journal_mode", [], |r| r.get(0))
            .unwrap(),
        "wal"
    );
    // Verify the writer's settings indirectly through behavior; FK is per connection.
    sql.execute_batch("PRAGMA foreign_keys=ON").unwrap();
    assert!(
        sql.execute("INSERT INTO ai_profiles VALUES('missing',1,'{}')", [])
            .is_err()
    );
    let duplicate: Actor = f.ai("another").await;
    assert!(
        sql.execute(
            "INSERT INTO memberships VALUES(?1,?2,'active',1,1,'{}')",
            params![c.id.to_string(), duplicate.id.to_string()]
        )
        .is_err()
    );
    // Schema itself rejects a second direct conversation for an archived relationship.
    assert!(sql.execute("INSERT INTO conversations SELECT ?1,kind,relationship_id,status,paused,revision,context_epoch,participation_epoch,last_sequence,json_set(payload,'$.id',?1) FROM conversations WHERE id=?2",params![ConversationId::new().to_string(),c.id.to_string()]).is_err());
    let backup = f.dir.path().join("explicit-backup.sqlite3");
    f.store.backup(backup.clone()).await.unwrap();
    let copied = Connection::open(&backup).unwrap();
    assert_eq!(
        copied
            .query_row::<i64, _, _>("SELECT count(*) FROM messages", [], |r| r.get(0))
            .unwrap(),
        1
    );
    assert!(f.store.backup(backup.clone()).await.is_err());
    sql.execute_batch("BEGIN IMMEDIATE").unwrap();
    let before = f.counts();
    assert!(
        matches!(f.store.create_ai(f.ctx(),CreateAi{display_name:"busy".into(),persona:String::new(),binding:None}).await,Err(SocialStorageError::Sqlite{source:rusqlite::Error::SqliteFailure(error,_),..}) if error.code==rusqlite::ErrorCode::DatabaseBusy)
    );
    sql.execute_batch("ROLLBACK").unwrap();
    assert_eq!(f.counts(), before);
    drop(sql);
    drop(copied);
    f.close().await;
}

fn record(messages: Vec<Message>) -> SessionRecord {
    SessionRecord {
        version: SessionRecord::VERSION,
        id: SessionId::new(),
        title: Some("old".into()),
        messages,
        metadata: HashMap::new(),
        model: None,
        run: None,
        created_at: Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap(),
        updated_at: Utc.with_ymd_and_hms(2026, 1, 2, 0, 0, 0).unwrap(),
    }
}
fn save_legacy(root: &Path, r: &SessionRecord) -> (std::path::PathBuf, Vec<u8>) {
    let path = root.join(format!("{}.json", r.id));
    let bytes = serde_json::to_vec(r).unwrap();
    fs::write(&path, &bytes).unwrap();
    (path, bytes)
}

#[tokio::test]
async fn legacy_preserves_sources_ids_metadata_tools_and_checkpoint_without_replay() {
    let f = Fixture::new().await;
    let mut user = Message::new(
        common::Role::User,
        vec![ContentPart::File {
            mime: "application/octet-stream".into(),
            data: vec![0, 255, 42],
        }],
    );
    user.id = None;
    let mut answer = Message::assistant_with_reasoning("answer", "thinking");
    answer.feedback = Some(common::Feedback::Up);
    answer.created_at = Some(12345);
    answer.thinking_ms = Some(120);
    answer.usage = Some(common::Usage {
        prompt_tokens: 20,
        completion_tokens: 10,
        total_tokens: 30,
        cached_tokens: 5,
    });
    answer.timings = Some(common::MessageTimings {
        ttft_ms: Some(30),
        reasoning_ms: Some(120),
        total_ms: Some(200),
    });
    let mut r = record(vec![
        Message::system("private old settings"),
        user,
        answer.clone(),
        Message::new(
            common::Role::Assistant,
            vec![ContentPart::ToolCall(ToolCall {
                id: "call".into(),
                name: "echo".into(),
                arguments: "{}".into(),
                thought_signature: None,
            })],
        ),
    ]);
    let partial = Message::assistant("unfinished");
    r.model = Some(ai_client::ModelSelection {
        provider: "mock".into(),
        model: "unvalidated-model-reference".into(),
        reasoning_effort: Some(ai_client::ReasoningEffort::High),
    });
    r.metadata
        .insert("topic".into(), "synthetic import fixture".into());
    r.run = Some(common::RunCheckpoint {
        id: partial.id.unwrap(),
        status: common::RunStatus::Running,
        partial: Some(partial),
    });
    let (path, bytes) = save_legacy(f.dir.path(), &r);
    let other = record(vec![Message::user("other session")]);
    let (other_path, other_bytes) = save_legacy(f.dir.path(), &other);
    fs::write(f.dir.path().join("llmn.toml"), "must not read or rewrite").unwrap();
    fs::write(f.dir.path().join("catalog.json"), "not session JSON").unwrap();
    let preview = f.store.preview_legacy().await.unwrap();
    assert_eq!(preview.sessions.len(), 2);
    assert_eq!(fs::read(&path).unwrap(), bytes);
    let first = f
        .store
        .import_legacy(f.human.id, preview.clone())
        .await
        .unwrap();
    assert_eq!(first.imported, 2);
    let counts = f.counts();
    let second = f.store.import_legacy(f.human.id, preview).await.unwrap();
    assert_eq!(second.already_imported, 2);
    assert_eq!(second.conversations, first.conversations);
    assert_eq!(f.counts(), counts);
    assert_eq!(fs::read(&path).unwrap(), bytes);
    assert_eq!(fs::read(other_path).unwrap(), other_bytes);
    let sql = f.sql();
    let conversation: String = sql
        .query_row(
            "SELECT conversation_id FROM legacy_imports WHERE session_id=?1",
            [r.id.to_string()],
            |r| r.get(0),
        )
        .unwrap();
    let c: ConversationId = conversation.parse().unwrap();
    let history = f
        .store
        .messages(f.human.id, c, Sequence::ZERO, 200)
        .await
        .unwrap();
    assert_eq!(history.len(), 3);
    let historical_ai: String = sql
        .query_row(
            "SELECT ai_actor_id FROM legacy_imports WHERE session_id=?1",
            [r.id.to_string()],
            |r| r.get(0),
        )
        .unwrap();
    let historical_ai: ActorId = historical_ai.parse().unwrap();
    assert_eq!(
        f.store
            .actor(f.human.id, historical_ai)
            .await
            .unwrap()
            .status,
        EntityStatus::Archived
    );
    assert!(f.store.open_direct(f.ctx(), historical_ai).await.is_err());
    let binding: ExecutionBinding = serde_json::from_str(
        &sql.query_row::<String, _, _>(
            "SELECT payload FROM execution_bindings WHERE actor_id=?1",
            [historical_ai.to_string()],
            |r| r.get(0),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(binding.model_id, "unvalidated-model-reference");
    assert_eq!(binding.reasoning_effort.as_deref(), Some("high"));
    assert_eq!(history[1].id.to_string(), answer.id.unwrap().to_string());
    assert_eq!(history[1].created_at.timestamp(), 12345);
    assert_eq!(history[2].status, MessageStatus::Interrupted);
    assert!(matches!(&history[0].body.0[0],SocialContent::File{data,..} if data==&vec![0,255,42]));
    let preserved: String = sql
        .query_row(
            "SELECT payload FROM legacy_entries WHERE message_id=?1",
            [answer.id.unwrap().to_string()],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(serde_json::from_str::<Message>(&preserved).unwrap(), answer);
    let traces: Vec<String> = {
        let mut stmt = sql
            .prepare("SELECT payload FROM legacy_entries WHERE entry_kind='trace'")
            .unwrap();
        stmt.query_map([], |r| r.get(0))
            .unwrap()
            .map(|r| r.unwrap())
            .collect()
    };
    assert!(traces.iter().any(|s| s.contains("private old settings")));
    assert!(traces.iter().any(|s| s.contains("\"is_error\":true")));
    assert_eq!(
        sql.query_row::<i64, _, _>("SELECT count(*) FROM ai_runs", [], |r| r.get(0))
            .unwrap(),
        0
    );
    assert_eq!(
        sql.query_row::<i64, _, _>(
            "SELECT sequence FROM read_positions WHERE conversation_id=?1",
            [c.to_string()],
            |r| r.get(0)
        )
        .unwrap(),
        3
    );
    let mapped_before: String = sql
        .query_row(
            "SELECT group_concat(message_id) FROM legacy_id_map",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let backup = f.dir.path().join("restorable.sqlite3");
    f.store.backup(backup).await.unwrap();
    drop(sql);
    f.store.close().await.unwrap();
    let reopened =
        SqliteSocialStore::open(DataDirectoryGuard::acquire(f.dir.path()).unwrap(), f.clock)
            .await
            .unwrap();
    assert_eq!(
        reopened
            .import_legacy(f.human.id, reopened.preview_legacy().await.unwrap())
            .await
            .unwrap()
            .already_imported,
        2
    );
    let sql = Connection::open(reopened.path()).unwrap();
    assert_eq!(
        sql.query_row::<String, _, _>(
            "SELECT group_concat(message_id) FROM legacy_id_map",
            [],
            |r| r.get(0)
        )
        .unwrap(),
        mapped_before
    );
    drop(sql);
    reopened.close().await.unwrap();
}

#[tokio::test]
async fn legacy_corruption_duplicates_source_changes_and_transaction_failure() {
    let f = Fixture::new().await;
    let mut r = record(vec![Message::user("old")]);
    let (path, bytes) = save_legacy(f.dir.path(), &r);
    let p = f.store.preview_legacy().await.unwrap();
    let before = f.counts();
    f.sql().execute_batch("CREATE TRIGGER fail_import BEFORE INSERT ON legacy_session_data BEGIN SELECT RAISE(ABORT,'injected import failure'); END").unwrap();
    assert!(f.store.import_legacy(f.human.id, p.clone()).await.is_err());
    assert_eq!(f.counts(), before);
    assert_eq!(fs::read(&path).unwrap(), bytes);
    f.sql().execute_batch("DROP TRIGGER fail_import").unwrap();
    f.store.import_legacy(f.human.id, p.clone()).await.unwrap();
    r.title = Some("changed".into());
    save_legacy(f.dir.path(), &r);
    assert!(f.store.import_legacy(f.human.id, p).await.is_err());
    assert!(
        f.store
            .import_legacy(f.human.id, f.store.preview_legacy().await.unwrap())
            .await
            .is_err()
    );
    fs::write(&path, "broken").unwrap();
    assert!(
        matches!(f.store.preview_legacy().await,Err(SocialStorageError::Legacy{path:p,..}) if p==path)
    );
    fs::write(&path, &bytes).unwrap();
    let mut duplicate = record(vec![r.messages[0].clone()]);
    let (dup_path, _) = save_legacy(f.dir.path(), &duplicate);
    assert!(f.store.preview_legacy().await.is_err());
    fs::remove_file(dup_path).unwrap();
    duplicate = r.clone();
    duplicate.messages.push(duplicate.messages[0].clone());
    save_legacy(f.dir.path(), &duplicate);
    assert!(f.store.preview_legacy().await.is_err());
    duplicate = r.clone();
    duplicate.version = 2;
    save_legacy(f.dir.path(), &duplicate);
    assert!(f.store.preview_legacy().await.is_err());
    fs::write(&path, &bytes).unwrap();
    let mut invalid: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    invalid["messages"][0]["content"] =
        serde_json::json!([{"type":"image","mime":"image/png","data":"invalid-base64"}]);
    fs::write(&path, serde_json::to_vec(&invalid).unwrap()).unwrap();
    assert!(f.store.preview_legacy().await.is_err());
    f.close().await;
}

#[tokio::test]
async fn shared_directory_guard_and_newer_schema_do_not_mutate() {
    let dir = tempfile::tempdir().unwrap();
    let guard = DataDirectoryGuard::acquire(dir.path()).unwrap();
    let json = FileSessionStore::from_guard(guard.clone());
    json.save_session(&record(vec![])).unwrap();
    let store = SqliteSocialStore::open(guard.clone(), Arc::new(SystemClock))
        .await
        .unwrap();
    assert!(DataDirectoryGuard::acquire(dir.path()).is_err());
    drop(guard);
    drop(json);
    assert!(FileSessionStore::new(dir.path()).is_err());
    store.close().await.unwrap();
    let guard = DataDirectoryGuard::acquire(dir.path()).unwrap();
    let db = dir.path().join("social.sqlite3");
    let sql = Connection::open(&db).unwrap();
    sql.execute_batch("PRAGMA user_version=999").unwrap();
    drop(sql);
    let before = fs::read(&db).unwrap();
    assert!(matches!(
        SqliteSocialStore::open(guard, Arc::new(SystemClock)).await,
        Err(SocialStorageError::Schema { .. })
    ));
    assert_eq!(fs::read(&db).unwrap(), before);
    let sql = Connection::open(&db).unwrap();
    sql.execute_batch("PRAGMA user_version=1; UPDATE schema_migrations SET checksum='tampered'")
        .unwrap();
    drop(sql);
    assert!(matches!(
        SqliteSocialStore::open(
            DataDirectoryGuard::acquire(dir.path()).unwrap(),
            Arc::new(SystemClock)
        )
        .await,
        Err(SocialStorageError::Schema { .. })
    ));
}

#[test]
fn social_worker_child() {
    let Some(path) = std::env::var_os("LLMN_SOCIAL_CRASH_FIXTURE") else {
        return;
    };
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(async {
        let store = SqliteSocialStore::open(
            DataDirectoryGuard::acquire(&path).unwrap(),
            Arc::new(SystemClock),
        )
        .await
        .unwrap();
        let human = store.local_human("test".into()).await.unwrap();
        let ctx = || CommandContext {
            operator: human.id,
            request_id: CommandId::new(),
        };
        let ai = store
            .create_ai(
                ctx(),
                CreateAi {
                    display_name: "AI".into(),
                    persona: String::new(),
                    binding: Some(BindingCandidate {
                        provider_id: "mock".into(),
                        model_id: "mock".into(),
                        reasoning_effort: None,
                    }),
                },
            )
            .await
            .unwrap();
        let c = store.open_direct(ctx(), ai.id).await.unwrap();
        let snapshot = ContextSnapshot {
            scope: ContextScope {
                actor_id: ai.id,
                conversation_id: c.id,
            },
            profile_revision: Revision::ONE,
            binding_revision: Revision::ONE,
            relationship_revision: Some(Revision::ONE),
            budget: ContextBudget {
                input_token_limit: Counter::new(1000).unwrap(),
                reserved_output_tokens: Counter::new(100).unwrap(),
            },
            participation_epoch: c.participation_epoch,
            dependencies: [human.id, ai.id]
                .into_iter()
                .map(|actor_id| VisibilityDependency {
                    conversation_id: c.id,
                    actor_id,
                    membership_revision: Revision::ONE,
                    context_epoch: c.context_epoch,
                })
                .collect(),
            sources: vec![],
        };
        let run = store
            .begin_run(
                ctx(),
                BeginRun {
                    details: None,
                    snapshot,
                    expected_revision: c.revision,
                },
            )
            .await
            .unwrap();
        store
            .checkpoint_run(human.id, run.id, MessageBody::text("durable draft"), None)
            .await
            .unwrap();
        // Leave an uncommitted write in WAL; process kill must roll it back.
        let sql = Connection::open(store.path()).unwrap();
        sql.execute_batch("BEGIN IMMEDIATE; INSERT INTO committed_events(payload) VALUES('{}')")
            .unwrap();
        let ready_tmp = Path::new(&path).join("ready.tmp");
        fs::write(
            &ready_tmp,
            serde_json::to_vec(&(human.id, c.id, run.id)).unwrap(),
        )
        .unwrap();
        fs::rename(ready_tmp, Path::new(&path).join("ready")).unwrap();
        loop {
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    });
}

#[tokio::test]
async fn killed_writer_releases_lock_recovers_draft_once_and_rolls_back_wal() {
    let dir = tempfile::tempdir().unwrap();
    let mut child = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "social_worker_child", "--nocapture"])
        .env("LLMN_SOCIAL_CRASH_FIXTURE", dir.path())
        .stdout(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let started = Instant::now();
    while !dir.path().join("ready").exists() {
        if child.try_wait().unwrap().is_some() {
            panic!("child exited before checkpoint");
        }
        if started.elapsed() > Duration::from_secs(20) {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("child checkpoint timeout");
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(DataDirectoryGuard::acquire(dir.path()).is_err());
    child.kill().unwrap();
    child.wait().unwrap();
    let (human, c, run): (ActorId, ConversationId, RunId) =
        serde_json::from_slice(&fs::read(dir.path().join("ready")).unwrap()).unwrap();
    let store = SqliteSocialStore::open(
        DataDirectoryGuard::acquire(dir.path()).unwrap(),
        Arc::new(SystemClock),
    )
    .await
    .unwrap();
    let messages = store.messages(human, c, Sequence::ZERO, 20).await.unwrap();
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].body, MessageBody::text("durable draft"));
    assert_eq!(messages[0].status, MessageStatus::Interrupted);
    let sql = Connection::open(store.path()).unwrap();
    assert_eq!(
        sql.query_row::<i64, _, _>(
            "SELECT count(*) FROM committed_events WHERE payload='{}'",
            [],
            |r| r.get(0)
        )
        .unwrap(),
        0
    );
    let state: AiRun = serde_json::from_str(
        &sql.query_row::<String, _, _>(
            "SELECT payload FROM ai_runs WHERE id=?1",
            [run.to_string()],
            |r| r.get(0),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(state.status, RunStatus::Interrupted);
    let events = store.events(human, EventCursor::ZERO, 500).await.unwrap();
    drop(sql);
    store.close().await.unwrap();
    let store = SqliteSocialStore::open(
        DataDirectoryGuard::acquire(dir.path()).unwrap(),
        Arc::new(SystemClock),
    )
    .await
    .unwrap();
    assert_eq!(
        store.events(human, EventCursor::ZERO, 500).await.unwrap(),
        events
    );
    store.close().await.unwrap();
}
