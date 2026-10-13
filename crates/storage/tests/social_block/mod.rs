use super::*;
use chrono::Duration as Delta;

async fn entry(f: &Fixture, actor: ActorId) -> ContactEntry {
    f.store
        .overview(f.human.id)
        .await
        .unwrap()
        .address_book
        .into_iter()
        .find(|e| e.account_id == actor)
        .unwrap()
}
async fn block(f: &Fixture, actor: ActorId, blocked: bool) -> ContactEntry {
    f.store
        .set_contact_block(
            f.ctx(),
            SetContactBlock {
                account_id: actor,
                blocked,
                expected_revision: Some(entry(f, actor).await.revision),
            },
        )
        .await
        .unwrap()
}
fn send(cid: ConversationId) -> SendMessage {
    SendMessage {
        conversation_id: cid,
        body: MessageBody::text("new message"),
        mentions: vec![],
        reply_to: None,
        reference: None,
    }
}
async fn finish(f: &Fixture, r: &AiRun) -> Result<AiRun, SocialStorageError> {
    f.store
        .finish_run(
            f.ctx(),
            FinishRun {
                run_id: r.id,
                expected_revision: r.revision,
                status: RunStatus::Succeeded,
                body: MessageBody::text("answer"),
                details: None,
            },
        )
        .await
}
fn intent_input(
    f: &Fixture,
    cid: ConversationId,
    actor: ActorId,
    seconds: i64,
) -> CreateContactIntent {
    CreateContactIntent {
        conversation_id: cid,
        actor_id: actor,
        text: "check in".into(),
        due_at: (f.clock.now() + Delta::seconds(seconds)).fixed_offset(),
        expires_at: f.clock.now() + Delta::hours(1),
    }
}
async fn enable(f: &Fixture, cid: ConversationId) {
    let mut settings = f
        .store
        .conversation_record(f.human.id, cid)
        .await
        .unwrap()
        .contact_schedule
        .settings;
    settings.enabled = true;
    f.store
        .update_contact_settings(
            f.ctx(),
            UpdateContactSettings {
                conversation_id: cid,
                settings,
            },
        )
        .await
        .unwrap();
}

#[tokio::test]
async fn blocking_is_atomic_versioned_idempotent_and_independent_of_listing() {
    let f = Fixture::new().await;
    let a = f.ai("A").await;
    let c = f.direct(a.id).await;
    let m = f.append(c.id, "history stays").await;
    let memory = f
        .store
        .create_memory(
            f.ctx(),
            CreateMemory {
                scope: ContextScope {
                    actor_id: a.id,
                    conversation_id: c.id,
                },
                content: MemoryContent {
                    kind: MemoryKind::Preference,
                    text: "keep this memory".into(),
                    source: Some(MemoryMessageSource {
                        message_id: m.id,
                        revision: m.revision,
                    }),
                },
            },
        )
        .await
        .unwrap();
    let run = f.begin(a.id, c.id, vec![]).await;
    let queued = f.send_group(c.id, vec![]).await.plans.remove(0);
    let ctx = f.ctx();
    let input = SetContactBlock {
        account_id: a.id,
        blocked: true,
        expected_revision: Some(entry(&f, a.id).await.revision),
    };
    let before = f.counts();
    f.inject();
    assert!(
        f.store
            .set_contact_block(ctx.clone(), input.clone())
            .await
            .is_err()
    );
    assert_eq!(before, f.counts());
    assert!(!entry(&f, a.id).await.blocked);
    assert_eq!(
        f.store.run(f.human.id, run.id).await.unwrap().status,
        RunStatus::Running
    );
    f.clear();
    let blocked = f
        .store
        .set_contact_block(ctx.clone(), input.clone())
        .await
        .unwrap();
    assert!(blocked.blocked && blocked.listed);
    assert_eq!(
        blocked,
        f.store
            .set_contact_block(ctx.clone(), input.clone())
            .await
            .unwrap()
    );
    assert!(matches!(
        f.store.set_contact_block(f.ctx(), input.clone()).await,
        Err(SocialStorageError::Conflict(_))
    ));
    let view = f.store.conversation_record(f.human.id, c.id).await.unwrap();
    assert!(view.direct_blocked && view.pending_plans.is_empty() && view.current_bout.is_none());
    assert_eq!(
        f.store.run(f.human.id, run.id).await.unwrap().status,
        RunStatus::Cancelled
    );
    assert!(finish(&f, &run).await.is_err());
    assert!(f.begin_plan(&queued).await.is_err());
    assert_eq!(f.store.message(f.human.id, m.id).await.unwrap(), m);
    assert_eq!(
        f.store
            .memories(
                f.human.id,
                ContextScope {
                    actor_id: a.id,
                    conversation_id: c.id
                }
            )
            .await
            .unwrap(),
        vec![memory]
    );
    assert_eq!(
        f.store.actor(f.human.id, a.id).await.unwrap().status,
        EntityStatus::Active
    );
    for listed in [false, true] {
        let e = entry(&f, a.id).await;
        let next = f
            .store
            .set_contact(
                f.ctx(),
                SetContact {
                    account_id: a.id,
                    listed,
                    expected_revision: Some(e.revision),
                },
            )
            .await
            .unwrap();
        assert!(next.blocked);
        assert_eq!(next.listed, listed);
    }
    block(&f, a.id, false).await;
    // A retried old block receipt returns current state without reapplying it.
    assert!(!f.store.set_contact_block(ctx, input).await.unwrap().blocked);
    assert_eq!(f.direct(a.id).await.id, c.id);
    assert!(
        f.store
            .conversation_record(f.human.id, c.id)
            .await
            .unwrap()
            .pending_plans
            .is_empty()
    );
    let new = f.store.send_message(f.ctx(), send(c.id)).await.unwrap();
    finish(&f, &f.begin_plan(&new.plans[0]).await.unwrap())
        .await
        .unwrap();
    f.close().await;
}

#[tokio::test]
async fn direct_send_and_stale_start_are_denied_in_both_directions() {
    let f = Fixture::new().await;
    let a = f.ai("A").await;
    let c = f.direct(a.id).await;
    let snapshot = f.snapshot(a.id, c.id, vec![]).await;
    block(&f, a.id, true).await;
    assert!(matches!(
        f.store.send_message(f.ctx(), send(c.id)).await,
        Err(SocialStorageError::Forbidden(_))
    ));
    for operator in [f.human.id, a.id] {
        assert!(matches!(
            f.store
                .append_message(
                    CommandContext {
                        operator,
                        request_id: CommandId::new()
                    },
                    AppendMessage {
                        conversation_id: c.id,
                        body: MessageBody::text("blocked"),
                        reply_to: None,
                        expected_revision: None
                    }
                )
                .await,
            Err(SocialStorageError::Forbidden(_))
        ));
    }
    for blocked in [true, false] {
        if !blocked {
            block(&f, a.id, false).await;
        }
        let revision = f
            .store
            .conversation(f.human.id, c.id)
            .await
            .unwrap()
            .revision;
        assert!(
            f.store
                .begin_run(
                    f.ctx(),
                    BeginRun {
                        snapshot: snapshot.clone(),
                        expected_revision: revision,
                        details: None
                    }
                )
                .await
                .is_err()
        );
    }
    // A reverse directed block applies equally, without changing the human's entry.
    f.store
        .set_contact_block(
            CommandContext {
                operator: a.id,
                request_id: CommandId::new(),
            },
            SetContactBlock {
                account_id: f.human.id,
                blocked: true,
                expected_revision: Some(Revision::ONE),
            },
        )
        .await
        .unwrap();
    assert!(!entry(&f, a.id).await.blocked);
    assert!(
        f.store
            .conversation_record(f.human.id, c.id)
            .await
            .unwrap()
            .direct_blocked
    );
    assert!(f.store.send_message(f.ctx(), send(c.id)).await.is_err());
    assert!(
        f.store
            .set_contact_block(
                f.ctx(),
                SetContactBlock {
                    account_id: f.human.id,
                    blocked: true,
                    expected_revision: None
                }
            )
            .await
            .is_err()
    );
    f.close().await;
}

#[tokio::test]
async fn shared_groups_keep_history_mentions_and_running_responses() {
    let f = Fixture::new().await;
    let a = f.ai("A").await;
    let b = f.ai("B").await;
    let g = f.group(vec![a.id, b.id]).await;
    let saved = f.send_group(g.id, vec![a.id]).await;
    let run = f.begin_plan(&saved.plans[0]).await.unwrap();
    block(&f, a.id, true).await;
    finish(&f, &run).await.unwrap();
    let view = f.store.conversation_record(f.human.id, g.id).await.unwrap();
    assert!(!view.direct_blocked);
    assert_eq!(view.members.len(), 3);
    assert_eq!(
        f.store
            .messages(a.id, g.id, Sequence::ZERO, 200)
            .await
            .unwrap()
            .len(),
        2
    );
    let next = f.send_group(g.id, vec![a.id]).await;
    finish(&f, &f.begin_plan(&next.plans[0]).await.unwrap())
        .await
        .unwrap();
    f.close().await;
}

#[tokio::test]
async fn pending_claimed_and_executing_intents_cancel_without_affecting_other_accounts() {
    let f = Fixture::new().await;
    let a = f.ai("A").await;
    let b = f.ai("B").await;
    let c = f.direct(a.id).await;
    let g = f.group(vec![a.id, b.id]).await;
    enable(&f, c.id).await;
    enable(&f, g.id).await;
    let future = f
        .store
        .create_contact_intent(f.ctx(), intent_input(&f, c.id, a.id, 600))
        .await
        .unwrap();
    f.store
        .create_contact_intent(f.ctx(), intent_input(&f, g.id, a.id, 1))
        .await
        .unwrap();
    f.store
        .create_contact_intent(f.ctx(), intent_input(&f, g.id, a.id, 2))
        .await
        .unwrap();
    let other = f
        .store
        .create_contact_intent(f.ctx(), intent_input(&f, g.id, b.id, 600))
        .await
        .unwrap();
    f.clock.set(f.clock.now() + Delta::seconds(2));
    let p = f
        .store
        .participation_tick(f.human.id, g.id)
        .await
        .unwrap()
        .plan
        .unwrap();
    let run = f.begin_plan(&p).await.unwrap();
    block(&f, a.id, true).await;
    assert!(finish(&f, &run).await.is_err());
    for cid in [c.id, g.id] {
        let view = f.store.conversation_record(f.human.id, cid).await.unwrap();
        for i in view.contact_schedule.intents {
            assert_eq!(
                i.status,
                if i.id == other.id {
                    ContactIntentStatus::Pending
                } else {
                    ContactIntentStatus::Cancelled
                }
            );
        }
        assert!(
            f.store
                .create_contact_intent(f.ctx(), intent_input(&f, cid, a.id, 100))
                .await
                .is_err()
        );
    }
    block(&f, a.id, false).await;
    f.clock.set(future.due_at);
    assert!(
        f.store
            .participation_tick(f.human.id, c.id)
            .await
            .unwrap()
            .plan
            .is_none()
    );
    let other_plan = f
        .store
        .participation_tick(f.human.id, g.id)
        .await
        .unwrap()
        .plan
        .unwrap();
    assert_eq!(other_plan.actor_id, b.id);
    finish(&f, &f.begin_plan(&other_plan).await.unwrap())
        .await
        .unwrap();
    f.close().await;
}

#[tokio::test]
async fn blocking_an_unlisted_account_does_not_add_it_or_require_a_private_window() {
    let f = Fixture::new().await;
    let a = f.ai("A").await;
    f.sql()
        .execute(
            "DELETE FROM address_book WHERE owner_id=?1 AND account_id=?2",
            params![f.human.id.to_string(), a.id.to_string()],
        )
        .unwrap();
    let e = f
        .store
        .set_contact_block(
            f.ctx(),
            SetContactBlock {
                account_id: a.id,
                blocked: true,
                expected_revision: None,
            },
        )
        .await
        .unwrap();
    assert!(e.blocked && !e.listed);
    assert!(
        f.store
            .overview(f.human.id)
            .await
            .unwrap()
            .conversations
            .is_empty()
    );
    let c = f.direct(a.id).await;
    assert!(f.store.send_message(f.ctx(), send(c.id)).await.is_err());
    block(&f, a.id, false).await;
    assert!(!entry(&f, a.id).await.listed);
    f.store.send_message(f.ctx(), send(c.id)).await.unwrap();
    f.close().await;
}

#[tokio::test]
async fn v12_migration_preserves_history_and_block_survives_reopen() {
    let f = Fixture::new().await;
    let a = f.ai("A").await;
    let c = f.direct(a.id).await;
    let m = f.append(c.id, "preserved").await;
    let path = f.dir.path().to_owned();
    f.store.close().await.unwrap();
    let sql = Connection::open(path.join("social.sqlite3")).unwrap();
    downgrade_block_schema(&sql);
    drop(sql);
    let store =
        SqliteSocialStore::open(DataDirectoryGuard::acquire(&path).unwrap(), f.clock.clone())
            .await
            .unwrap();
    let old = store.overview(f.human.id).await.unwrap().address_book;
    assert!(old.iter().all(|e| !e.blocked));
    assert_eq!(store.message(f.human.id, m.id).await.unwrap(), m);
    store
        .set_contact_block(
            CommandContext {
                operator: f.human.id,
                request_id: CommandId::new(),
            },
            SetContactBlock {
                account_id: a.id,
                blocked: true,
                expected_revision: Some(Revision::ONE),
            },
        )
        .await
        .unwrap();
    store.close().await.unwrap();
    let store =
        SqliteSocialStore::open(DataDirectoryGuard::acquire(&path).unwrap(), f.clock.clone())
            .await
            .unwrap();
    assert!(
        store
            .conversation_record(f.human.id, c.id)
            .await
            .unwrap()
            .direct_blocked
    );
    assert!(
        store
            .send_message(
                CommandContext {
                    operator: f.human.id,
                    request_id: CommandId::new()
                },
                send(c.id)
            )
            .await
            .is_err()
    );
    assert!(fs::read_dir(&path).unwrap().any(|e| {
        e.unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with("social.before-v13-")
    }));
    store.close().await.unwrap();
}
