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
use storage::{DataDirectoryGuard, legacy::SessionRecord};
#[path = "social_block/mod.rs"]
mod block_tests;
#[path = "social_contact/mod.rs"]
mod contact_tests;
#[path = "social_export/mod.rs"]
mod export_tests;
#[path = "social_memory/mod.rs"]
mod memory_tests;
#[path = "social_message_list/mod.rs"]
mod message_list_tests;
#[path = "social_participation/mod.rs"]
mod participation_tests;
#[path = "social_reference/mod.rs"]
mod reference_tests;
#[path = "social_summary/mod.rs"]
mod summary_tests;

struct Fixture {
    dir: tempfile::TempDir,
    store: SqliteSocialStore,
    human: Actor,
    clock: Arc<ManualClock>,
}
fn downgrade_block_schema(sql: &Connection) {
    sql.execute_batch("ALTER TABLE address_book DROP COLUMN blocked; DELETE FROM schema_migrations WHERE version=13; PRAGMA user_version=12;").unwrap();
}
fn downgrade_list_schema(sql: &Connection) {
    downgrade_block_schema(sql);
    sql.execute_batch("DROP TRIGGER message_delivered_insert; DROP TRIGGER message_delivered_update; DROP TABLE message_deliveries; DROP TABLE conversation_list_state; CREATE TABLE read_positions(conversation_id TEXT NOT NULL REFERENCES conversations(id),actor_id TEXT NOT NULL REFERENCES actors(id),sequence INTEGER NOT NULL CHECK(sequence>=0),PRIMARY KEY(conversation_id,actor_id)); DELETE FROM schema_migrations WHERE version=12; PRAGMA user_version=11;").unwrap();
}
fn downgrade_contact_schema(sql: &Connection) {
    downgrade_list_schema(sql);
    sql.execute_batch("DROP TRIGGER plan_same_space; DROP INDEX pending_participation; DROP INDEX one_running_plan; DROP INDEX message_plan_order; ALTER TABLE participation_plans RENAME TO plans_v11;").unwrap();
    sql.execute_batch(include_str!("../src/social/schema-v3.sql"))
        .unwrap();
    sql.execute_batch("INSERT INTO participation_plans SELECT id,conversation_id,actor_id,trigger_id,trigger_sequence,ordinal,status,run_id,payload FROM plans_v11; DROP TABLE plans_v11; DROP TABLE contact_intents; DROP TABLE contact_settings; DELETE FROM schema_migrations WHERE version=11; PRAGMA user_version=10;").unwrap();
}

#[tokio::test]
async fn generation_parameters_are_independent_atomic_authorized_and_persistent() {
    let f = Fixture::new().await;
    let a = f.ai("A").await;
    let b = f.ai("B").await;
    let private = f.direct(a.id).await;
    let original = f.store.contact(f.human.id, a.id).await.unwrap();
    // Old JSON bindings and command digests retain their omitted optional fields.
    let old =
        serde_json::json!({"provider_id":"mock","model_id":"same-model","reasoning_effort":null});
    let candidate: BindingCandidate = serde_json::from_value(old.clone()).unwrap();
    assert_eq!(serde_json::to_value(&candidate).unwrap(), old);
    let mut input = UpdateAi {
        display_name: a.display_name.clone(),
        bio: a.bio.clone(),
        avatar_ref: None,
        persona: original.profile.persona.clone(),
        binding: Some(candidate.clone()),
        expected_revision: a.revision,
    };
    let counts = f.counts();
    for bad in [Some(f32::NAN), Some(f32::INFINITY), Some(-0.1), Some(2.1)] {
        input.binding.as_mut().unwrap().temperature = bad;
        assert!(
            f.store
                .update_ai(f.ctx(), input.clone(), a.id)
                .await
                .is_err()
        );
        assert_eq!(f.counts(), counts);
    }
    input.binding = Some(candidate);
    input.binding.as_mut().unwrap().max_tokens = Some(0);
    assert!(
        f.store
            .update_ai(f.ctx(), input.clone(), a.id)
            .await
            .is_err()
    );
    assert_eq!(f.counts(), counts);
    input.binding.as_mut().unwrap().temperature = Some(0.25);
    input.binding.as_mut().unwrap().max_tokens = Some(4096);
    let unauthorized = CommandContext {
        operator: b.id,
        request_id: CommandId::new(),
    };
    assert!(matches!(
        f.store.update_ai(unauthorized, input.clone(), a.id).await,
        Err(SocialStorageError::Forbidden(_))
    ));
    f.inject();
    assert!(
        f.store
            .update_ai(f.ctx(), input.clone(), a.id)
            .await
            .is_err()
    );
    assert_eq!(f.counts(), counts);
    assert_eq!(
        f.store.contact(f.human.id, a.id).await.unwrap().binding,
        original.binding
    );
    f.clear();
    let ctx = f.ctx();
    let saved = f
        .store
        .update_ai(ctx.clone(), input.clone(), a.id)
        .await
        .unwrap();
    let binding = saved.binding.unwrap();
    assert_eq!(binding.temperature, Some(0.25));
    assert_eq!(binding.max_tokens, Some(4096));
    assert_eq!(binding.id, original.binding.as_ref().unwrap().id);
    assert!(binding.revision > original.binding.as_ref().unwrap().revision);
    assert_eq!(
        f.store
            .contact(f.human.id, b.id)
            .await
            .unwrap()
            .binding
            .unwrap()
            .temperature,
        None
    );
    assert_eq!(
        f.store.open_direct(f.ctx(), a.id).await.unwrap().id,
        private.id
    );
    let counts = f.counts();
    assert_eq!(
        f.store
            .update_ai(ctx.clone(), input.clone(), a.id)
            .await
            .unwrap()
            .binding,
        Some(binding.clone())
    );
    assert_eq!(f.counts(), counts);
    input.binding.as_mut().unwrap().max_tokens = Some(2048);
    assert!(matches!(
        f.store.update_ai(ctx, input, a.id).await,
        Err(SocialStorageError::Conflict(_))
    ));
    let path = f.dir.path().to_owned();
    let reopen_context = f.ctx();
    f.store.close().await.unwrap();
    let reopened =
        SqliteSocialStore::open(DataDirectoryGuard::acquire(&path).unwrap(), f.clock.clone())
            .await
            .unwrap();
    assert_eq!(
        reopened.contact(f.human.id, a.id).await.unwrap().binding,
        Some(binding)
    );
    assert_eq!(
        reopened.open_direct(reopen_context, a.id).await.unwrap().id,
        private.id
    );
    reopened.close().await.unwrap();
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
                    reference: None,
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
        if let Some(t) = p.not_before
            && t > self.clock.now()
        {
            self.clock.set(t);
        }
        let material = self.store.plan_material(self.human.id, p.id).await?;
        let mut snapshot = ContextSnapshot {
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
        if let Some(intent_id) = p.contact_intent_id {
            snapshot
                .sources
                .push(SourceRef::ContactIntent { intent_id });
        }
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
                    bio: String::new(),
                    avatar_ref: None,
                    persona: "角色设定".into(),
                    binding: Some(BindingCandidate {
                        provider_id: "mock".into(),
                        model_id: "same-model".into(),
                        reasoning_effort: None,
                        temperature: None,
                        max_tokens: None,
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
            "contact_settings",
            "contact_intents",
            "participation_policies",
            "participation_state",
            "participation_daily_budget",
            "ai_profiles",
            "execution_bindings",
            "relationships",
            "conversations",
            "memberships",
            "messages",
            "message_deliveries",
            "conversation_list_state",
            "message_revisions",
            "bouts",
            "ai_runs",
            "committed_events",
            "command_receipts",
            "legacy_imports",
            "legacy_entries",
            "legacy_id_map",
            "account_admins",
            "address_book",
            "context_references",
            "context_reference_sources",
            "summary_settings",
            "conversation_summaries",
            "summary_revisions",
            "summary_message_sources",
            "memories",
            "memory_revisions",
            "memory_message_sources",
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
async fn v5_account_migration_preserves_history_private_settings_and_old_disabled_accounts() {
    let f = Fixture::new().await;
    let a = f.ai("active account").await;
    let b = f.ai("previously archived contact").await;
    let c = f.direct(a.id).await;
    let m = f.append(c.id, "preserved history").await;
    let old_stop = f.ctx();
    let old_stop_input = AccountControl {
        actor_id: b.id,
        expected_revision: b.revision,
        action: AccountAction::Archive,
    };
    f.store
        .control_account(old_stop.clone(), old_stop_input.clone())
        .await
        .unwrap();
    let path = f.store.path().to_owned();
    let operator = f.human.id;
    f.store.close().await.unwrap();
    let old = Connection::open(&path).unwrap();
    downgrade_contact_schema(&old);
    old.execute_batch("DROP TABLE participation_daily_budget; DROP TABLE participation_state; DROP TABLE participation_policies; DROP TABLE context_reference_sources; DROP TABLE context_references; DROP TABLE summary_message_sources; DROP TABLE summary_revisions; DROP TABLE conversation_summaries; DROP TABLE summary_settings; DROP TABLE memory_message_sources; DROP TABLE memory_revisions; DROP TABLE memories; DROP TABLE address_book; DROP TABLE account_admins; DELETE FROM schema_migrations WHERE version>=6; UPDATE actors SET payload=json_remove(payload,'$.bio'); UPDATE command_receipts SET kind='control_contact' WHERE kind='control_account'; PRAGMA user_version=5;").unwrap();
    drop(old);
    let store = SqliteSocialStore::open(
        DataDirectoryGuard::acquire(f.dir.path()).unwrap(),
        f.clock.clone(),
    )
    .await
    .unwrap();
    let overview = store.overview(operator).await.unwrap();
    assert_eq!(
        store
            .control_account(old_stop, old_stop_input)
            .await
            .unwrap()
            .actor
            .status,
        EntityStatus::Archived
    );
    assert_eq!(
        store.overview(operator).await.unwrap().cursor,
        overview.cursor,
        "upgraded command receipts do not repeat the stop"
    );
    assert!(
        overview
            .address_book
            .iter()
            .any(|e| e.account_id == a.id && e.listed)
    );
    assert!(
        overview
            .address_book
            .iter()
            .any(|e| e.account_id == b.id && !e.listed)
    );
    assert_eq!(store.managed_accounts(operator).await.unwrap().len(), 2);
    let managed = store.managed_account(operator, a.id).await.unwrap();
    assert_eq!(
        managed.administrators,
        vec![Administrator {
            account_id: operator,
            initial: true
        }]
    );
    assert_eq!(managed.contact.actor.id, a.id);
    assert_eq!(managed.contact.actor.bio, "");
    assert_eq!(managed.contact.profile.persona, "角色设定");
    assert_eq!(managed.contact.binding.unwrap().model_id, "same-model");
    assert_eq!(
        store.actor(operator, b.id).await.unwrap().status,
        EntityStatus::Archived
    );
    assert_eq!(
        store.message_page(operator, c.id, None).await.unwrap().0,
        vec![m]
    );
    let mut preserved_backup = false;
    for entry in fs::read_dir(f.dir.path()).unwrap() {
        let path = entry.unwrap().path();
        if !path
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("social.before-v6-")
        {
            continue;
        }
        let backup = Connection::open(&path).unwrap();
        if backup
            .query_row::<i64, _, _>("SELECT count(*) FROM messages", [], |r| r.get(0))
            .unwrap()
            == 1
        {
            assert_eq!(
                backup
                    .pragma_query_value::<i64, _>(None, "user_version", |r| r.get(0))
                    .unwrap(),
                5
            );
            let payload: String = backup
                .query_row(
                    "SELECT payload FROM actors WHERE id=?1",
                    [a.id.to_string()],
                    |r| r.get(0),
                )
                .unwrap();
            assert!(
                serde_json::from_str::<serde_json::Value>(&payload)
                    .unwrap()
                    .get("bio")
                    .is_none()
            );
            preserved_backup = true;
        }
    }
    assert!(preserved_backup);
    store.close().await.unwrap();
}

#[tokio::test]
async fn removing_a_contact_preserves_running_replies_groups_and_the_unique_private_chat() {
    let f = Fixture::new().await;
    let a = f.ai("A").await;
    let b = f.ai("B").await;
    let direct = f.direct(a.id).await;
    let group = f.group(vec![a.id, b.id]).await;
    let saved = f.send_group(group.id, vec![a.id, b.id]).await;
    let run = f.begin_plan(&saved.plans[0]).await.unwrap();
    let group_before = f.store.conversation(f.human.id, group.id).await.unwrap();
    let before = f.store.overview(f.human.id).await.unwrap();
    let entry = before
        .address_book
        .iter()
        .find(|e| e.account_id == a.id)
        .unwrap();
    let input = SetContact {
        account_id: a.id,
        listed: false,
        expected_revision: Some(entry.revision),
    };
    let ctx = f.ctx();
    f.inject();
    assert!(
        f.store
            .set_contact(ctx.clone(), input.clone())
            .await
            .is_err()
    );
    assert_eq!(
        f.store.overview(f.human.id).await.unwrap().address_book,
        before.address_book
    );
    assert_eq!(
        f.store.overview(f.human.id).await.unwrap().cursor,
        before.cursor
    );
    f.clear();
    let removed = f
        .store
        .set_contact(ctx.clone(), input.clone())
        .await
        .unwrap();
    assert!(!removed.listed);
    assert_eq!(f.store.actor(f.human.id, a.id).await.unwrap(), a);
    assert_eq!(
        f.store.conversation(f.human.id, group.id).await.unwrap(),
        group_before
    );
    assert_eq!(
        f.store.run(f.human.id, run.id).await.unwrap().status,
        RunStatus::Running
    );
    assert_eq!(
        f.store
            .plans(f.human.id, group.id)
            .await
            .unwrap()
            .iter()
            .find(|p| p.id == saved.plans[1].id)
            .unwrap()
            .status,
        PlanStatus::Queued
    );
    f.store
        .finish_run(
            f.ctx(),
            FinishRun {
                run_id: run.id,
                expected_revision: run.revision,
                status: RunStatus::Succeeded,
                body: MessageBody::text("completed after removal"),
                details: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(f.direct(a.id).await.id, direct.id);
    assert!(!f.send_group(direct.id, vec![]).await.plans.is_empty());
    let added = f
        .store
        .set_contact(
            f.ctx(),
            SetContact {
                account_id: a.id,
                listed: true,
                expected_revision: Some(removed.revision),
            },
        )
        .await
        .unwrap();
    assert!(added.listed);
    assert_eq!(
        f.store.set_contact(ctx, input.clone()).await.unwrap(),
        added,
        "receipt returns current relationship state"
    );
    assert!(matches!(
        f.store.set_contact(f.ctx(), input).await,
        Err(SocialStorageError::Conflict(_))
    ));
    assert_eq!(f.direct(a.id).await.id, direct.id);
    // The AI's address book is a separate owner relationship.
    let ai_view = f.store.account_view(f.human.id, a.id).await.unwrap();
    assert!(
        ai_view
            .address_book
            .iter()
            .any(|e| e.account_id == f.human.id && e.listed)
    );
    f.close().await;
}

#[tokio::test]
async fn administrator_grants_enforce_permissions_revocation_versions_and_transaction_rollback() {
    let f = Fixture::new().await;
    let a = f.ai("A").await;
    let b = f.ai("B").await;
    let as_b = || CommandContext {
        operator: b.id,
        request_id: CommandId::new(),
    };
    assert!(matches!(
        f.store.managed_account(b.id, a.id).await,
        Err(SocialStorageError::Forbidden(_))
    ));
    assert!(matches!(
        f.store.account_view(b.id, a.id).await,
        Err(SocialStorageError::Forbidden(_))
    ));
    assert!(matches!(
        f.store
            .control_account(
                as_b(),
                AccountControl {
                    actor_id: a.id,
                    expected_revision: a.revision,
                    action: AccountAction::Archive
                }
            )
            .await,
        Err(SocialStorageError::Forbidden(_))
    ));
    let grant = SetAdministrator {
        account_id: b.id,
        granted: true,
        expected_revision: a.revision,
    };
    let ctx = f.ctx();
    let counts = f.counts();
    f.inject();
    assert!(
        f.store
            .set_administrator(ctx.clone(), a.id, grant.clone())
            .await
            .is_err()
    );
    assert_eq!(f.counts(), counts);
    assert_eq!(f.store.actor(f.human.id, a.id).await.unwrap(), a);
    f.clear();
    let managed = f
        .store
        .set_administrator(ctx.clone(), a.id, grant.clone())
        .await
        .unwrap();
    assert!(managed.is_initial_administrator);
    assert_eq!(managed.administrators.len(), 2);
    assert!(
        !f.store
            .managed_account(b.id, a.id)
            .await
            .unwrap()
            .is_initial_administrator
    );
    assert!(matches!(
        f.store
            .set_administrator(
                as_b(),
                a.id,
                SetAdministrator {
                    account_id: f.human.id,
                    granted: false,
                    expected_revision: managed.contact.actor.revision
                }
            )
            .await,
        Err(SocialStorageError::Forbidden(_))
    ));
    let update_ctx = as_b();
    let update = UpdateAi {
        display_name: "A edited".into(),
        bio: "public".into(),
        avatar_ref: None,
        persona: "private".into(),
        binding: None,
        expected_revision: managed.contact.actor.revision,
    };
    let edited = f
        .store
        .update_ai(update_ctx.clone(), update.clone(), a.id)
        .await
        .unwrap();
    assert_eq!(edited.profile.persona, "private");
    let stopped = f
        .store
        .control_account(
            as_b(),
            AccountControl {
                actor_id: a.id,
                expected_revision: edited.actor.revision,
                action: AccountAction::Archive,
            },
        )
        .await
        .unwrap();
    assert!(
        f.store.account_view(b.id, a.id).await.is_ok(),
        "a stopped AI may still be inspected by an active administrator"
    );
    let restored = f
        .store
        .control_account(
            as_b(),
            AccountControl {
                actor_id: a.id,
                expected_revision: stopped.actor.revision,
                action: AccountAction::Restore,
            },
        )
        .await
        .unwrap();
    assert!(matches!(
        f.store
            .set_administrator(f.ctx(), a.id, grant.clone())
            .await,
        Err(SocialStorageError::Conflict(_))
    ));
    let revoked = f
        .store
        .set_administrator(
            f.ctx(),
            a.id,
            SetAdministrator {
                account_id: b.id,
                granted: false,
                expected_revision: restored.actor.revision,
            },
        )
        .await
        .unwrap();
    assert_eq!(revoked.administrators.len(), 1);
    assert!(
        matches!(
            f.store.update_ai(update_ctx, update, a.id).await,
            Err(SocialStorageError::Forbidden(_))
        ),
        "permission is checked before an old receipt is replayed"
    );
    assert!(matches!(
        f.store.account_view(b.id, a.id).await,
        Err(SocialStorageError::Forbidden(_))
    ));
    assert_eq!(
        f.store
            .set_administrator(ctx, a.id, grant)
            .await
            .unwrap()
            .administrators,
        revoked.administrators
    );
    assert!(
        f.store
            .set_administrator(
                f.ctx(),
                a.id,
                SetAdministrator {
                    account_id: f.human.id,
                    granted: false,
                    expected_revision: revoked.contact.actor.revision
                }
            )
            .await
            .is_err()
    );
    f.close().await;
}

#[tokio::test]
async fn account_view_reads_the_ais_memberships_and_checks_permission_on_every_history_read() {
    let f = Fixture::new().await;
    let a = f.ai("A").await;
    let b = f.ai("B").await;
    let own = f.direct(a.id).await;
    let unrelated = f.direct(b.id).await;
    let group = f.group(vec![a.id, b.id]).await;
    f.append(group.id, "shared history").await;
    // Trusted store callers may represent other active accounts; the GUI does not accept an operator ID.
    let ctx = CommandContext {
        operator: b.id,
        request_id: CommandId::new(),
    };
    let ai_private = f.store.open_direct(ctx.clone(), a.id).await.unwrap();
    f.store
        .append_message(
            CommandContext {
                request_id: CommandId::new(),
                ..ctx
            },
            AppendMessage {
                conversation_id: ai_private.id,
                body: MessageBody::text("AI-owned conversation"),
                reply_to: None,
                expected_revision: None,
            },
        )
        .await
        .unwrap();
    assert!(matches!(
        f.store.message_page(f.human.id, ai_private.id, None).await,
        Err(SocialStorageError::Forbidden(_))
    ));
    let view = f.store.account_view(f.human.id, a.id).await.unwrap();
    assert_eq!(view.account.id, a.id);
    assert!(
        view.conversations
            .iter()
            .any(|c| c.conversation.id == own.id)
    );
    assert!(
        view.conversations
            .iter()
            .any(|c| c.conversation.id == ai_private.id)
    );
    assert!(
        !view
            .conversations
            .iter()
            .any(|c| c.conversation.id == unrelated.id)
    );
    assert_eq!(
        f.store
            .account_view_messages(f.human.id, a.id, ai_private.id, None)
            .await
            .unwrap()
            .0[0]
            .body,
        MessageBody::text("AI-owned conversation")
    );
    assert!(matches!(
        f.store
            .account_view_messages(f.human.id, a.id, unrelated.id, None)
            .await,
        Err(SocialStorageError::Forbidden(_))
    ));
    let current = f.store.conversation(f.human.id, group.id).await.unwrap();
    f.store
        .change_membership(
            f.ctx(),
            ChangeMembership {
                conversation_id: group.id,
                actor_id: a.id,
                action: MembershipAction::Remove,
                expected_revision: current.revision,
            },
        )
        .await
        .unwrap();
    assert!(matches!(
        f.store
            .account_view_messages(f.human.id, a.id, group.id, None)
            .await,
        Err(SocialStorageError::Forbidden(_))
    ));
    assert!(
        !f.store
            .account_view(f.human.id, a.id)
            .await
            .unwrap()
            .conversations
            .iter()
            .any(|c| c.conversation.id == group.id)
    );
    let stopped = f
        .store
        .control_account(
            f.ctx(),
            AccountControl {
                actor_id: a.id,
                expected_revision: a.revision,
                action: AccountAction::Archive,
            },
        )
        .await
        .unwrap();
    assert!(f.store.account_view(f.human.id, a.id).await.is_ok());
    assert_eq!(
        f.store.open_direct(f.ctx(), a.id).await.unwrap().id,
        own.id,
        "stopped-account lookup is scoped to the exact account pair"
    );
    assert_eq!(stopped.actor.id, a.id);
    f.close().await;
}

#[tokio::test]
async fn configuration_changes_keep_existing_runs_frozen_and_new_runs_use_new_versions() {
    let f = Fixture::new().await;
    let a = f.ai("A").await;
    let c = f.direct(a.id).await;
    let m = f.append(c.id, "before update").await;
    let run = f.begin(a.id, c.id, vec![source(&m)]).await;
    let update = UpdateAi {
        display_name: "new name".into(),
        bio: "public bio".into(),
        avatar_ref: None,
        persona: "new internal persona".into(),
        binding: Some(BindingCandidate {
            provider_id: "mock".into(),
            model_id: "new-model".into(),
            reasoning_effort: None,
            temperature: None,
            max_tokens: None,
        }),
        expected_revision: a.revision,
    };
    let counts = f.counts();
    f.inject();
    assert!(
        f.store
            .update_ai(f.ctx(), update.clone(), a.id)
            .await
            .is_err()
    );
    assert_eq!(f.counts(), counts);
    f.clear();
    let updated = f.store.update_ai(f.ctx(), update, a.id).await.unwrap();
    f.store
        .finish_run(
            f.ctx(),
            FinishRun {
                run_id: run.id,
                expected_revision: run.revision,
                status: RunStatus::Succeeded,
                body: MessageBody::text("old configuration succeeded"),
                details: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(
        f.store
            .run(f.human.id, run.id)
            .await
            .unwrap()
            .snapshot
            .profile_revision,
        Revision::ONE
    );
    let mut snapshot = f.snapshot(a.id, c.id, vec![source(&m)]).await;
    let current = f.store.conversation(f.human.id, c.id).await.unwrap();
    assert!(
        matches!(
            f.store
                .begin_run(
                    f.ctx(),
                    BeginRun {
                        snapshot: snapshot.clone(),
                        expected_revision: current.revision,
                        details: None
                    }
                )
                .await,
            Err(SocialStorageError::Conflict(_))
        ),
        "stale preparation is rejected before starting"
    );
    snapshot.profile_revision = updated.profile.revision;
    snapshot.binding_revision = updated.binding.unwrap().revision;
    let next = f
        .store
        .begin_run(
            f.ctx(),
            BeginRun {
                snapshot: snapshot.clone(),
                expected_revision: current.revision,
                details: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(next.snapshot, snapshot);
    assert_eq!(f.direct(a.id).await.id, c.id);
    f.close().await;
}

#[tokio::test]
async fn contact_archive_is_atomic_across_spaces_and_preserves_other_ai_and_history() {
    let f = Fixture::new().await;
    let a = f.ai("A").await;
    let b = f.ai("B").await;
    let direct = f.direct(a.id).await;
    let group = f.group(vec![a.id, b.id]).await;
    let other = f.group(vec![a.id, b.id]).await;
    let private = f.send_group(direct.id, vec![]).await;
    let collaborative = f.send_group(group.id, vec![a.id, b.id]).await;
    let unrelated = f.send_group(other.id, vec![b.id, a.id]).await;
    let private_run = f.begin_plan(&private.plans[0]).await.unwrap();
    let group_run = f.begin_plan(&collaborative.plans[0]).await.unwrap();
    let other_run = f.begin_plan(&unrelated.plans[0]).await.unwrap();
    for run in [&private_run, &group_run] {
        f.store
            .checkpoint_run(
                f.human.id,
                run.id,
                MessageBody::text("saved fragment"),
                None,
            )
            .await
            .unwrap();
    }
    let old_contact = f.store.contact(f.human.id, a.id).await.unwrap();
    let before = f.store.overview(f.human.id).await.unwrap();
    let ctx = f.ctx();
    let input = AccountControl {
        actor_id: a.id,
        expected_revision: a.revision,
        action: AccountAction::Archive,
    };
    f.inject();
    assert!(
        f.store
            .control_account(ctx.clone(), input.clone())
            .await
            .is_err()
    );
    assert_eq!(
        f.store.overview(f.human.id).await.unwrap().cursor,
        before.cursor
    );
    assert_eq!(f.store.actor(f.human.id, a.id).await.unwrap(), a);
    assert_eq!(
        f.store.run(f.human.id, group_run.id).await.unwrap().status,
        RunStatus::Running
    );
    assert_eq!(
        f.store
            .plans(f.human.id, other.id)
            .await
            .unwrap()
            .iter()
            .find(|p| p.id == unrelated.plans[1].id)
            .unwrap()
            .status,
        PlanStatus::Queued
    );
    f.clear();
    let archived = f
        .store
        .control_account(ctx.clone(), input.clone())
        .await
        .unwrap();
    assert_eq!(archived.actor.status, EntityStatus::Archived);
    assert_eq!(archived.actor.revision, a.revision.next().unwrap());
    assert_eq!(archived.profile, old_contact.profile);
    assert_eq!(archived.binding, old_contact.binding);
    assert_eq!(
        f.store
            .control_account(ctx.clone(), input.clone())
            .await
            .unwrap()
            .actor,
        archived.actor
    );
    for run in [&private_run, &group_run] {
        assert_eq!(
            f.store.run(f.human.id, run.id).await.unwrap().status,
            RunStatus::Cancelled
        );
        let message = f.store.message(f.human.id, run.message_id).await.unwrap();
        assert_eq!(message.status, MessageStatus::Interrupted);
        assert_eq!(message.body, MessageBody::text("saved fragment"));
        assert_eq!(message.sender_id, a.id);
        assert_eq!(message.sender_name, "A");
        assert!(matches!(
            f.store
                .finish_run(
                    f.ctx(),
                    FinishRun {
                        run_id: run.id,
                        expected_revision: run.revision,
                        status: RunStatus::Succeeded,
                        body: MessageBody::text("late success"),
                        details: None
                    }
                )
                .await,
            Err(SocialStorageError::Conflict(_))
        ));
    }
    assert_eq!(
        f.store.run(f.human.id, other_run.id).await.unwrap().status,
        RunStatus::Running
    );
    assert_eq!(
        f.store
            .next_plan(f.human.id, group.id)
            .await
            .unwrap()
            .unwrap()
            .actor_id,
        b.id
    );
    let plans = f.store.plans(f.human.id, other.id).await.unwrap();
    assert_eq!(
        plans
            .iter()
            .find(|p| p.id == unrelated.plans[1].id)
            .unwrap()
            .status,
        PlanStatus::Cancelled
    );
    let overview = f.store.overview(f.human.id).await.unwrap();
    assert_eq!(
        overview.accounts.len(),
        3,
        "archived identities remain discoverable for history and restore"
    );
    for c in [direct.id, group.id, other.id] {
        let record = f.store.conversation_record(f.human.id, c).await.unwrap();
        assert!(record.members.iter().any(|m| m.actor_id == a.id));
        assert_eq!(record.conversation.status, EntityStatus::Active);
    }
    assert_eq!(f.direct(a.id).await.id, direct.id);
    let saved = f.send_group(direct.id, vec![]).await;
    assert!(saved.plans.is_empty());
    let saved = f.send_group(group.id, vec![a.id, b.id]).await;
    assert_eq!(saved.plans.len(), 1);
    assert_eq!(saved.plans[0].actor_id, b.id);
    let restored = f
        .store
        .control_account(
            f.ctx(),
            AccountControl {
                actor_id: a.id,
                expected_revision: archived.actor.revision,
                action: AccountAction::Restore,
            },
        )
        .await
        .unwrap();
    assert_eq!(restored.actor.status, EntityStatus::Active);
    assert_eq!(
        f.store.control_account(ctx, input).await.unwrap().actor,
        restored.actor,
        "replaying an old archive must not rearchive"
    );
    assert_eq!(f.direct(a.id).await.id, direct.id);
    assert!(
        f.store
            .next_plan(f.human.id, direct.id)
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(f.send_group(direct.id, vec![]).await.plans.len(), 1);
    let events = f
        .store
        .events(f.human.id, before.cursor, 100)
        .await
        .unwrap();
    assert!(events.iter().any(
        |e| matches!(e.change, connector::SocialChange::ActorChanged {actor_id,..} if actor_id==a.id)
    ));
    f.close().await;
}

#[tokio::test]
async fn contact_restore_preserves_window_controls_and_group_removal_and_checks_versions() {
    let f = Fixture::new().await;
    let a = f.ai("A").await;
    let unused = f.ai("Unused").await;
    let private = f.direct(a.id).await;
    let group = f.group(vec![a.id]).await;
    let retained = f.group(vec![a.id]).await;
    f.store
        .pause_member(
            f.ctx(),
            PauseMember {
                conversation_id: retained.id,
                actor_id: a.id,
                expected_revision: Revision::ONE,
                paused: true,
            },
        )
        .await
        .unwrap();
    f.store
        .pause_member(
            f.ctx(),
            PauseMember {
                conversation_id: group.id,
                actor_id: a.id,
                expected_revision: Revision::ONE,
                paused: true,
            },
        )
        .await
        .unwrap();
    f.store
        .control_conversation(
            f.ctx(),
            ConversationControl {
                conversation_id: private.id,
                expected_revision: private.revision,
                action: ConversationAction::Archive,
            },
        )
        .await
        .unwrap();
    let ctx = f.ctx();
    let input = AccountControl {
        actor_id: a.id,
        expected_revision: a.revision,
        action: AccountAction::Archive,
    };
    let archived = f
        .store
        .control_account(ctx.clone(), input.clone())
        .await
        .unwrap();
    assert!(matches!(
        f.store.control_account(f.ctx(), input.clone()).await,
        Err(SocialStorageError::Conflict(_))
    ));
    assert!(matches!(
        f.store
            .control_account(
                ctx,
                AccountControl {
                    action: AccountAction::Restore,
                    ..input.clone()
                }
            )
            .await,
        Err(SocialStorageError::Conflict(_))
    ));
    assert!(
        f.store
            .control_account(
                CommandContext {
                    operator: a.id,
                    request_id: CommandId::new()
                },
                input
            )
            .await
            .is_err()
    );
    assert!(
        f.store
            .control_account(
                f.ctx(),
                AccountControl {
                    actor_id: f.human.id,
                    expected_revision: f.human.revision,
                    action: AccountAction::Archive
                }
            )
            .await
            .is_err()
    );
    assert!(
        f.store
            .create_group(f.ctx(), "invalid".into(), vec![a.id])
            .await
            .is_err()
    );
    let other = f.group(vec![unused.id]).await;
    assert!(
        f.store
            .change_membership(
                f.ctx(),
                ChangeMembership {
                    conversation_id: other.id,
                    actor_id: a.id,
                    expected_revision: other.revision,
                    action: MembershipAction::Add
                }
            )
            .await
            .is_err()
    );
    let current = f.store.conversation(f.human.id, group.id).await.unwrap();
    f.store
        .change_membership(
            f.ctx(),
            ChangeMembership {
                conversation_id: group.id,
                actor_id: a.id,
                expected_revision: current.revision,
                action: MembershipAction::Remove,
            },
        )
        .await
        .unwrap();
    let restored = f
        .store
        .control_account(
            f.ctx(),
            AccountControl {
                actor_id: a.id,
                expected_revision: archived.actor.revision,
                action: AccountAction::Restore,
            },
        )
        .await
        .unwrap();
    assert_eq!(
        f.store
            .conversation(f.human.id, private.id)
            .await
            .unwrap()
            .status,
        EntityStatus::Archived
    );
    assert_eq!(f.direct(a.id).await.id, private.id);
    assert!(
        f.store
            .conversation_record(f.human.id, retained.id)
            .await
            .unwrap()
            .members
            .iter()
            .find(|m| m.actor_id == a.id)
            .unwrap()
            .paused
    );
    assert!(
        !f.store
            .conversation_record(f.human.id, group.id)
            .await
            .unwrap()
            .members
            .iter()
            .any(|m| m.actor_id == a.id),
        "restore must not rejoin a removed member"
    );
    let no_op = f
        .store
        .control_account(
            f.ctx(),
            AccountControl {
                actor_id: a.id,
                expected_revision: restored.actor.revision,
                action: AccountAction::Restore,
            },
        )
        .await
        .unwrap();
    assert_eq!(no_op.actor.revision, restored.actor.revision);
    let archived_unused = f
        .store
        .control_account(
            f.ctx(),
            AccountControl {
                actor_id: unused.id,
                expected_revision: unused.revision,
                action: AccountAction::Archive,
            },
        )
        .await
        .unwrap();
    assert!(
        f.store
            .open_direct(f.ctx(), archived_unused.actor.id)
            .await
            .is_err(),
        "an archived contact cannot create a new private chat"
    );
    f.close().await;
}

#[tokio::test]
async fn contact_archive_and_finish_keep_the_first_committed_outcome() {
    let f = Fixture::new().await;
    for archive_first in [false, true] {
        let ai = f.ai("Race").await;
        let c = f.direct(ai.id).await;
        let run = f.begin(ai.id, c.id, vec![]).await;
        f.store
            .checkpoint_run(f.human.id, run.id, MessageBody::text("draft"), None)
            .await
            .unwrap();
        let finish = f.store.finish_run(
            f.ctx(),
            FinishRun {
                run_id: run.id,
                expected_revision: run.revision,
                status: RunStatus::Succeeded,
                body: MessageBody::text("complete"),
                details: None,
            },
        );
        let archive = f.store.control_account(
            f.ctx(),
            AccountControl {
                actor_id: ai.id,
                expected_revision: ai.revision,
                action: AccountAction::Archive,
            },
        );
        let (finished, archived) = if archive_first {
            let (archived, finished) = tokio::join!(biased; archive, finish);
            (finished, archived)
        } else {
            tokio::join!(biased; finish, archive)
        };
        assert_eq!(archived.unwrap().actor.status, EntityStatus::Archived);
        let current = f.store.run(f.human.id, run.id).await.unwrap();
        let message = f.store.message(f.human.id, run.message_id).await.unwrap();
        if archive_first {
            assert!(matches!(finished, Err(SocialStorageError::Conflict(_))));
            assert_eq!(current.status, RunStatus::Cancelled);
            assert_eq!(message.status, MessageStatus::Interrupted);
            assert_eq!(message.body, MessageBody::text("draft"));
        } else {
            assert_eq!(finished.unwrap().status, RunStatus::Succeeded);
            assert_eq!(current.status, RunStatus::Succeeded);
            assert_eq!(message.status, MessageStatus::Committed);
            assert_eq!(message.body, MessageBody::text("complete"));
        }
    }
    f.close().await;
}

#[tokio::test]
async fn group_queue_is_atomic_idempotent_fifo_and_shares_only_its_committed_answers() {
    let f = Fixture::new().await;
    let a = f.ai("A").await;
    let b = f.ai("B").await;
    let c = f.group(vec![a.id, b.id]).await;
    let ctx = f.ctx();
    let input = SendMessage {
        reference: None,
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
        reference: None,
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
async fn revisions_and_withdrawal_preserve_later_history_and_revoke_old_replies_atomically() {
    let f = Fixture::new().await;
    let a = f.ai("A").await;
    let b = f.ai("B").await;
    let c = f.group(vec![a.id, b.id]).await;
    let sent = f.send_group(c.id, vec![a.id, b.id]).await;
    let run = f.begin_plan(&sent.plans[0]).await.unwrap();
    f.store
        .checkpoint_run(f.human.id, run.id, MessageBody::text("saved partial"), None)
        .await
        .unwrap();
    let later = f.send_group(c.id, vec![]).await.message;
    let edit_ctx = f.ctx();
    let edit = ReviseMessage {
        message_id: sent.message.id,
        text: "edited source".into(),
        expected_revision: sent.message.revision,
    };
    let revised = f
        .store
        .revise_message(edit_ctx.clone(), edit.clone())
        .await
        .unwrap();
    assert_eq!(
        f.store
            .revise_message(edit_ctx.clone(), edit.clone())
            .await
            .unwrap(),
        revised
    );
    assert_eq!(
        f.store.run(f.human.id, run.id).await.unwrap().status,
        RunStatus::Interrupted
    );
    assert_eq!(
        f.store
            .message(f.human.id, run.message_id)
            .await
            .unwrap()
            .body,
        MessageBody::text("saved partial")
    );
    assert_eq!(
        f.store
            .plans(f.human.id, c.id)
            .await
            .unwrap()
            .iter()
            .find(|p| p.id == sent.plans[1].id)
            .unwrap()
            .status,
        PlanStatus::Cancelled
    );
    assert!(
        f.store.revise_message(f.ctx(), edit.clone()).await.is_err(),
        "stale edits are rejected"
    );
    let mut changed = edit.clone();
    changed.text = "different retry".into();
    assert!(
        f.store
            .revise_message(edit_ctx.clone(), changed)
            .await
            .is_err()
    );
    let quoted = f
        .store
        .send_message(
            f.ctx(),
            SendMessage {
                reference: None,
                conversation_id: c.id,
                body: MessageBody::text("replying to revised source"),
                reply_to: Some(revised.id),
                mentions: vec![a.id],
            },
        )
        .await
        .unwrap();
    let input = WithdrawMessage {
        message_id: revised.id,
        expected_revision: revised.revision,
    };
    let ctx = f.ctx();
    let cursor = f.store.overview(f.human.id).await.unwrap().cursor;
    f.sql().execute_batch("CREATE TRIGGER reject_withdraw BEFORE UPDATE ON messages WHEN NEW.status='withdrawn' BEGIN SELECT RAISE(ABORT,'synthetic'); END;").unwrap();
    assert!(
        f.store
            .withdraw_message(ctx.clone(), input.clone())
            .await
            .is_err()
    );
    assert_eq!(
        f.store.message(f.human.id, revised.id).await.unwrap(),
        revised
    );
    assert_eq!(f.store.overview(f.human.id).await.unwrap().cursor, cursor);
    f.sql()
        .execute_batch("DROP TRIGGER reject_withdraw")
        .unwrap();
    let withdrawn = f
        .store
        .withdraw_message(ctx.clone(), input.clone())
        .await
        .unwrap();
    assert_eq!(withdrawn.status, MessageStatus::Withdrawn);
    assert!(withdrawn.body.0.is_empty());
    assert_eq!(
        f.store.withdraw_message(ctx, input).await.unwrap(),
        withdrawn
    );
    assert_eq!(
        f.store.revise_message(edit_ctx, edit).await.unwrap(),
        withdrawn,
        "old receipts read the current placeholder"
    );
    let history = f
        .store
        .messages(f.human.id, c.id, Sequence::ZERO, 100)
        .await
        .unwrap();
    assert_eq!(
        history.iter().map(|m| m.id).collect::<Vec<_>>(),
        vec![sent.message.id, run.message_id, later.id, quoted.message.id]
    );
    assert_eq!(
        f.store
            .plans(f.human.id, c.id)
            .await
            .unwrap()
            .iter()
            .find(|p| p.id == quoted.plans[0].id)
            .unwrap()
            .status,
        PlanStatus::Cancelled,
        "withdrawing an explicit reply source revokes waiting replies to it"
    );
    let old_texts:i64=f.sql().query_row("SELECT count(*) FROM message_revisions WHERE message_id=?1 AND json_array_length(json_extract(payload,'$.body'))>0",[withdrawn.id.to_string()],|r|r.get(0)).unwrap();
    assert_eq!(
        old_texts, 0,
        "withdrawn original revisions cannot revive text"
    );
    let material = f
        .store
        .context_material(
            f.human.id,
            ContextScope {
                actor_id: a.id,
                conversation_id: c.id,
            },
            later.id,
        )
        .await
        .unwrap();
    assert!(
        !material
            .messages
            .iter()
            .any(|m| m.id == withdrawn.id || m.id == run.message_id)
    );
    assert!(
        f.store
            .withdraw_message(
                f.ctx(),
                WithdrawMessage {
                    message_id: run.message_id,
                    expected_revision: history[1].revision
                }
            )
            .await
            .is_err(),
        "human cannot withdraw another sender's message"
    );
    f.store.close().await.unwrap();
}

#[tokio::test]
async fn participation_controls_preserve_shared_history_and_never_revive_cancelled_plans() {
    let f = Fixture::new().await;
    let a = f.ai("A").await;
    let b = f.ai("B").await;
    let c = f.group(vec![a.id, b.id]).await;
    let saved = f.send_group(c.id, vec![a.id, b.id]).await;
    let run = f.begin_plan(&saved.plans[0]).await.unwrap();
    f.store
        .pause_member(
            f.ctx(),
            PauseMember {
                conversation_id: c.id,
                actor_id: b.id,
                expected_revision: Revision::ONE,
                paused: true,
            },
        )
        .await
        .unwrap();
    assert_eq!(
        f.store.run(f.human.id, run.id).await.unwrap().status,
        RunStatus::Running,
        "pausing B does not stop A"
    );
    assert_eq!(
        f.store
            .plans(f.human.id, c.id)
            .await
            .unwrap()
            .iter()
            .find(|p| p.id == saved.plans[1].id)
            .unwrap()
            .status,
        PlanStatus::Cancelled
    );
    assert!(
        f.store
            .context_material(
                f.human.id,
                ContextScope {
                    actor_id: b.id,
                    conversation_id: c.id
                },
                saved.message.id
            )
            .await
            .unwrap()
            .messages
            .iter()
            .any(|m| m.id == saved.message.id)
    );
    let cancelled = f.store.cancel_run(f.human.id, run.id).await.unwrap();
    assert_eq!(cancelled.status, RunStatus::Cancelled);
    assert_eq!(
        f.store.cancel_run(f.human.id, run.id).await.unwrap(),
        cancelled
    );
    let next = f.send_group(c.id, vec![b.id, a.id]).await;
    assert_eq!(
        next.plans.iter().map(|p| p.actor_id).collect::<Vec<_>>(),
        vec![a.id]
    );
    let run2 = f.begin_plan(&next.plans[0]).await.unwrap();
    let record = f.store.conversation_record(f.human.id, c.id).await.unwrap();
    let old_bout = record.current_bout.unwrap().id;
    let ctx = f.ctx();
    let end = EndConversation {
        conversation_id: c.id,
        expected_revision: record.conversation.revision,
    };
    f.store
        .end_conversation(ctx.clone(), end.clone())
        .await
        .unwrap();
    assert_eq!(
        f.store.run(f.human.id, run2.id).await.unwrap().status,
        RunStatus::Cancelled
    );
    f.store.end_conversation(ctx, end).await.unwrap();
    assert!(
        f.store
            .conversation_record(f.human.id, c.id)
            .await
            .unwrap()
            .current_bout
            .is_none()
    );
    let fresh = f.send_group(c.id, vec![a.id]).await;
    assert_ne!(
        f.begin_plan(&fresh.plans[0]).await.unwrap().bout_id,
        old_bout
    );
    let current = f.store.conversation(f.human.id, c.id).await.unwrap();
    f.store
        .control_conversation(
            f.ctx(),
            ConversationControl {
                conversation_id: c.id,
                expected_revision: current.revision,
                action: ConversationAction::Pause,
            },
        )
        .await
        .unwrap();
    assert!(f.send_group(c.id, vec![a.id, b.id]).await.plans.is_empty());
    let current = f.store.conversation(f.human.id, c.id).await.unwrap();
    let archived = f
        .store
        .control_conversation(
            f.ctx(),
            ConversationControl {
                conversation_id: c.id,
                expected_revision: current.revision,
                action: ConversationAction::Archive,
            },
        )
        .await
        .unwrap();
    let restored = f
        .store
        .control_conversation(
            f.ctx(),
            ConversationControl {
                conversation_id: c.id,
                expected_revision: archived.revision,
                action: ConversationAction::Restore,
            },
        )
        .await
        .unwrap();
    assert!(
        restored.paused,
        "restoring a chat preserves participation settings"
    );
    f.store
        .control_conversation(
            f.ctx(),
            ConversationControl {
                conversation_id: c.id,
                expected_revision: restored.revision,
                action: ConversationAction::Resume,
            },
        )
        .await
        .unwrap();
    let b_member = f
        .store
        .conversation_record(f.human.id, c.id)
        .await
        .unwrap()
        .members
        .into_iter()
        .find(|m| m.actor_id == b.id)
        .unwrap();
    let resumed = f
        .store
        .pause_member(
            f.ctx(),
            PauseMember {
                conversation_id: c.id,
                actor_id: b.id,
                expected_revision: b_member.revision,
                paused: false,
            },
        )
        .await
        .unwrap();
    assert!(!resumed.paused);
    assert_eq!(
        f.store
            .plans(f.human.id, c.id)
            .await
            .unwrap()
            .iter()
            .find(|p| p.id == saved.plans[1].id)
            .unwrap()
            .status,
        PlanStatus::Cancelled
    );
    assert_eq!(f.send_group(c.id, vec![a.id, b.id]).await.plans.len(), 2);
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
                action: ConversationAction::Archive,
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
                action: ConversationAction::Restore,
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
    assert_eq!(
        f.store
            .messages(ai.id, c.id, Sequence::ZERO, 20)
            .await
            .unwrap()
            .len(),
        2
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
                text: "corrected".into(),
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
                    bio: String::new(),
                    avatar_ref: None,
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
                    text: "failed".into(),
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
                    action: ConversationAction::Archive,
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
    let saved = f
        .store
        .send_message(
            f.ctx(),
            SendMessage {
                conversation_id: c.id,
                body: MessageBody::text("continue"),
                reply_to: None,
                mentions: vec![],
                reference: Some(ReferenceSelection {
                    conversation_id: g.id,
                    sources: vec![source(&gm)],
                }),
            },
        )
        .await
        .unwrap();
    let run = f
        .begin(
            ai.id,
            c.id,
            vec![
                source(&gm),
                SourceRef::Reference {
                    message_id: saved.message.id,
                    revision: saved.message.revision,
                },
            ],
        )
        .await;
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
                text: "changed".into(),
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
    assert_eq!(
        f.store.run(f.human.id, run.id).await.unwrap().status,
        RunStatus::Interrupted
    );
    assert_eq!(
        f.store
            .run_details(f.human.id, run.id)
            .await
            .unwrap()
            .unwrap()
            .error
            .unwrap()
            .code,
        "context_changed"
    );
    let run = f.begin(ai.id, c.id, vec![]).await;
    let current = f.store.conversation(f.human.id, c.id).await.unwrap();
    f.store
        .control_conversation(
            f.ctx(),
            ConversationControl {
                conversation_id: c.id,
                expected_revision: current.revision,
                action: ConversationAction::Pause,
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
                action: ConversationAction::Resume,
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
        matches!(f.store.create_ai(f.ctx(),CreateAi{display_name:"busy".into(),bio:String::new(),avatar_ref:None,persona:String::new(),binding:None}).await,Err(SocialStorageError::Sqlite{source:rusqlite::Error::SqliteFailure(error,_),..}) if error.code==rusqlite::ErrorCode::DatabaseBusy)
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
async fn legacy_import_selection_keeps_unselected_sources_and_rejects_forged_previews() {
    let f = Fixture::new().await;
    let first = record(vec![Message::user("selected")]);
    let second = record(vec![Message::user("unselected")]);
    let (first_path, first_bytes) = save_legacy(f.dir.path(), &first);
    let (second_path, second_bytes) = save_legacy(f.dir.path(), &second);
    let all = f.store.preview_legacy().await.unwrap();
    let mut selected = all.clone();
    selected
        .sessions
        .retain(|p| p.session_id == first.id.to_string());
    let mut forged = selected.clone();
    forged.sessions[0].title = Some("forged".into());
    assert!(f.store.import_legacy(f.human.id, forged).await.is_err());
    let mut repeated = selected.clone();
    repeated.sessions.push(selected.sessions[0].clone());
    assert!(f.store.import_legacy(f.human.id, repeated).await.is_err());
    let imported = f
        .store
        .import_legacy(f.human.id, selected.clone())
        .await
        .unwrap();
    assert_eq!(imported.imported, 1);
    assert_eq!(
        f.store
            .import_legacy(f.human.id, selected)
            .await
            .unwrap()
            .already_imported,
        1
    );
    let rest = f.store.import_legacy(f.human.id, all).await.unwrap();
    assert_eq!((rest.imported, rest.already_imported), (1, 1));
    assert_eq!(fs::read(first_path).unwrap(), first_bytes);
    assert_eq!(fs::read(second_path).unwrap(), second_bytes);
    f.close().await;
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
    let export = f
        .store
        .conversation_transcript(f.human.id, c, false, false)
        .await
        .unwrap();
    assert!(export.legacy);
    assert_eq!(export.messages.len(), history.len());
    assert_eq!(export.messages[1].id, history[1].id);
    assert!(matches!(
        export.messages[0].parts[0],
        TranscriptPart::File { byte_length: 3, .. }
    ));
    assert!(
        !serde_json::to_string(&export)
            .unwrap()
            .contains("private old settings")
    );
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
            "SELECT read_through FROM conversation_list_state WHERE conversation_id=?1",
            [c.to_string()],
            |r| r.get(0)
        )
        .unwrap(),
        sql.query_row::<i64, _, _>(
            "SELECT MAX(position) FROM message_deliveries WHERE conversation_id=?1",
            [c.to_string()],
            |r| r.get(0)
        )
        .unwrap()
    );
    assert_eq!(
        f.store
            .conversation_record(f.human.id, c)
            .await
            .unwrap()
            .list_state
            .unread_count,
        0
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
    let store = SqliteSocialStore::open(guard.clone(), Arc::new(SystemClock))
        .await
        .unwrap();
    assert!(DataDirectoryGuard::acquire(dir.path()).is_err());
    drop(guard);
    assert!(DataDirectoryGuard::acquire(dir.path()).is_err());
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
                    bio: String::new(),
                    avatar_ref: None,
                    persona: String::new(),
                    binding: Some(BindingCandidate {
                        provider_id: "mock".into(),
                        model_id: "mock".into(),
                        reasoning_effort: None,
                        temperature: None,
                        max_tokens: None,
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
