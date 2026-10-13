use super::*;
use chrono::{Duration as Delta, FixedOffset};

async fn view(f: &Fixture, cid: ConversationId) -> ContactScheduleView {
    f.store
        .conversation_record(f.human.id, cid)
        .await
        .unwrap()
        .contact_schedule
}
async fn settings(f: &Fixture, cid: ConversationId, edit: impl FnOnce(&mut ContactSettings)) {
    let mut s = view(f, cid).await.settings;
    edit(&mut s);
    f.store
        .update_contact_settings(
            f.ctx(),
            UpdateContactSettings {
                conversation_id: cid,
                settings: s,
            },
        )
        .await
        .unwrap();
}
fn input(f: &Fixture, cid: ConversationId, actor: ActorId, seconds: i64) -> CreateContactIntent {
    CreateContactIntent {
        conversation_id: cid,
        actor_id: actor,
        text: "ask about reading".into(),
        due_at: (f.clock.now() + Delta::seconds(seconds))
            .with_timezone(&FixedOffset::east_opt(8 * 3600).unwrap()),
        expires_at: f.clock.now() + Delta::seconds(seconds + 300),
    }
}
async fn intent(f: &Fixture, cid: ConversationId, actor: ActorId, seconds: i64) -> ContactIntent {
    f.store
        .create_contact_intent(f.ctx(), input(f, cid, actor, seconds))
        .await
        .unwrap()
}
async fn tick(f: &Fixture, cid: ConversationId) -> ParticipationTick {
    f.store.participation_tick(f.human.id, cid).await.unwrap()
}
async fn result(f: &Fixture, r: &AiRun) -> Result<AiRun, SocialStorageError> {
    f.store
        .finish_run(
            f.ctx(),
            FinishRun {
                run_id: r.id,
                expected_revision: r.revision,
                status: RunStatus::Succeeded,
                body: MessageBody::text("约定问候"),
                details: None,
            },
        )
        .await
}
async fn current(f: &Fixture, cid: ConversationId, id: IntentId) -> ContactIntent {
    view(f, cid)
        .await
        .intents
        .into_iter()
        .find(|i| i.id == id)
        .unwrap()
}

#[tokio::test]
async fn explicit_consent_idempotency_validation_and_creation_rollback() {
    let f = Fixture::new().await;
    let a = f.ai("A").await;
    let c = f.direct(a.id).await;
    assert!(matches!(
        f.store
            .create_contact_intent(f.ctx(), input(&f, c.id, a.id, 60))
            .await,
        Err(SocialStorageError::Forbidden(_))
    ));
    settings(&f, c.id, |s| s.enabled = true).await;
    let ctx = f.ctx();
    let i = input(&f, c.id, a.id, 60);
    f.inject();
    assert!(
        f.store
            .create_contact_intent(ctx.clone(), i.clone())
            .await
            .is_err()
    );
    assert!(view(&f, c.id).await.intents.is_empty());
    f.clear();
    let created = f
        .store
        .create_contact_intent(ctx.clone(), i.clone())
        .await
        .unwrap();
    assert_eq!(created.utc_offset_minutes, 480);
    assert_eq!(
        f.store
            .create_contact_intent(ctx.clone(), i.clone())
            .await
            .unwrap(),
        created
    );
    assert_eq!(view(&f, c.id).await.intents.len(), 1);
    let mut bad = i;
    bad.text = "changed".into();
    assert!(matches!(
        f.store.create_contact_intent(ctx, bad).await,
        Err(SocialStorageError::Conflict(_))
    ));
    for seconds in [-1, 0, 366 * 86400] {
        assert!(
            f.store
                .create_contact_intent(f.ctx(), input(&f, c.id, a.id, seconds))
                .await
                .is_err()
        );
    }
    let outside = f.ai("outside").await;
    assert!(
        f.store
            .create_contact_intent(f.ctx(), input(&f, c.id, outside.id, 30))
            .await
            .is_err()
    );
    assert!(
        f.store
            .messages(f.human.id, c.id, Sequence::ZERO, 200)
            .await
            .unwrap()
            .is_empty()
    );
    f.close().await;
}

#[tokio::test]
async fn due_claim_is_unique_has_no_fake_message_and_finishes_atomically() {
    let f = Fixture::new().await;
    let a = f.ai("A").await;
    let b = f.ai("B").await;
    let c = f.group(vec![a.id, b.id]).await;
    settings(&f, c.id, |s| s.enabled = true).await;
    participation_tests::policy(&f, c.id, |p| p.limited_auto = true).await;
    let i = intent(&f, c.id, a.id, 60).await;
    assert_eq!(tick(&f, c.id).await.wake_at, Some(i.due_at));
    f.clock.set(i.due_at - Delta::seconds(70));
    assert!(tick(&f, c.id).await.plan.is_none());
    f.clock.set(i.due_at);
    let p = tick(&f, c.id).await.plan.unwrap();
    assert_eq!(tick(&f, c.id).await.plan.unwrap().id, p.id);
    assert_eq!(p.trigger_id, None);
    assert!(
        f.store
            .plan_material(f.human.id, p.id)
            .await
            .unwrap()
            .messages
            .is_empty()
    );
    let r = f.begin_plan(&p).await.unwrap();
    assert!(f.begin_plan(&p).await.is_err());
    assert_eq!(
        current(&f, c.id, i.id).await.status,
        ContactIntentStatus::Executing
    );
    assert_eq!(
        f.store
            .conversation_record(f.human.id, c.id)
            .await
            .unwrap()
            .participation
            .auto_turns_today,
        1
    );
    f.inject();
    assert!(result(&f, &r).await.is_err());
    assert_eq!(
        current(&f, c.id, i.id).await.status,
        ContactIntentStatus::Executing
    );
    assert_eq!(
        f.store.run(f.human.id, r.id).await.unwrap().status,
        RunStatus::Running
    );
    f.clear();
    result(&f, &r).await.unwrap();
    assert_eq!(
        current(&f, c.id, i.id).await.status,
        ContactIntentStatus::Delivered
    );
    let messages = f
        .store
        .messages(f.human.id, c.id, Sequence::ZERO, 200)
        .await
        .unwrap();
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].reply_to, None);
    assert!(
        tick(&f, c.id).await.plan.is_none(),
        "one-shot contact must not start an automatic group chain"
    );
    f.close().await;
}

#[tokio::test]
async fn quiet_defer_skip_expiry_and_release_do_not_block_explicit_replies() {
    let f = Fixture::new().await;
    let a = f.ai("A").await;
    let c = f.direct(a.id).await;
    settings(&f, c.id, |s| s.enabled = true).await;
    let i = intent(&f, c.id, a.id, 10).await;
    f.clock.set(i.due_at);
    let p = tick(&f, c.id).await.plan.unwrap();
    let until = f.clock.now() + Delta::seconds(100);
    settings(&f, c.id, |s| s.quiet_until = Some(until)).await;
    assert!(tick(&f, c.id).await.plan.is_none());
    let revision = current(&f, c.id, i.id).await.revision;
    tick(&f, c.id).await;
    let stable = current(&f, c.id, i.id).await.revision;
    tick(&f, c.id).await;
    assert_eq!(current(&f, c.id, i.id).await.revision, stable);
    assert!(stable >= revision);
    let sent = f.send_group(c.id, vec![]).await;
    f.clock.set(f.clock.now() + Delta::seconds(1));
    let explicit = tick(&f, c.id).await.plan.unwrap();
    assert_eq!(explicit.id, sent.plans[0].id);
    let run = f.begin_plan(&explicit).await.unwrap();
    result(&f, &run).await.unwrap();
    settings(&f, c.id, |s| s.quiet_until = None).await;
    assert_eq!(tick(&f, c.id).await.plan.unwrap().id, p.id);
    let run = f.begin_plan(&p).await.unwrap();
    result(&f, &run).await.unwrap();
    let skip = intent(&f, c.id, a.id, 10).await;
    settings(&f, c.id, |s| {
        s.quiet_until = Some(until);
        s.quiet_action = QuietAction::Skip
    })
    .await;
    f.clock.set(skip.due_at);
    assert!(tick(&f, c.id).await.plan.is_none());
    assert_eq!(
        current(&f, c.id, skip.id).await.status,
        ContactIntentStatus::Skipped
    );
    let expires = intent(&f, c.id, a.id, 10).await;
    settings(&f, c.id, |s| {
        s.quiet_action = QuietAction::Defer;
        s.quiet_until = Some(expires.expires_at + Delta::seconds(1))
    })
    .await;
    f.clock.set(expires.due_at);
    assert!(tick(&f, c.id).await.plan.is_none());
    f.clock.set(expires.expires_at);
    tick(&f, c.id).await;
    assert_eq!(
        current(&f, c.id, expires.id).await.status,
        ContactIntentStatus::Expired
    );
    f.close().await;
}

#[tokio::test]
async fn ending_preserves_future_authorization_but_disable_cancels_and_does_not_replay() {
    let f = Fixture::new().await;
    let a = f.ai("A").await;
    let c = f.direct(a.id).await;
    settings(&f, c.id, |s| s.enabled = true).await;
    let i = intent(&f, c.id, a.id, 60).await;
    f.store
        .end_conversation(
            f.ctx(),
            EndConversation {
                conversation_id: c.id,
                expected_revision: c.revision,
            },
        )
        .await
        .unwrap();
    assert_eq!(
        current(&f, c.id, i.id).await.status,
        ContactIntentStatus::Pending
    );
    f.clock.set(i.due_at);
    let p = tick(&f, c.id).await.plan.unwrap();
    let r = f.begin_plan(&p).await.unwrap();
    let until = f.clock.now() + Delta::seconds(300);
    settings(&f, c.id, |s| s.quiet_until = Some(until)).await;
    assert_eq!(
        current(&f, c.id, i.id).await.status,
        ContactIntentStatus::Cancelled
    );
    assert_eq!(
        f.store.run(f.human.id, r.id).await.unwrap().status,
        RunStatus::Cancelled
    );
    let future = intent(&f, c.id, a.id, 60).await;
    let original = view(&f, c.id).await.settings;
    settings(&f, c.id, |s| s.enabled = false).await;
    assert_eq!(
        current(&f, c.id, i.id).await.status,
        ContactIntentStatus::Cancelled
    );
    assert_eq!(
        current(&f, c.id, future.id).await.status,
        ContactIntentStatus::Cancelled
    );
    assert_eq!(
        f.store.run(f.human.id, r.id).await.unwrap().status,
        RunStatus::Cancelled
    );
    assert!(result(&f, &r).await.is_err());
    assert!(matches!(
        f.store
            .update_contact_settings(
                f.ctx(),
                UpdateContactSettings {
                    conversation_id: c.id,
                    settings: original
                }
            )
            .await,
        Err(SocialStorageError::Conflict(_))
    ));
    settings(&f, c.id, |s| s.enabled = true).await;
    f.clock.set(future.due_at);
    assert!(tick(&f, c.id).await.plan.is_none());
    f.close().await;
}

#[tokio::test]
async fn cancellation_is_versioned_atomic_and_cannot_cross_windows() {
    let f = Fixture::new().await;
    let a = f.ai("A").await;
    let b = f.ai("B").await;
    let c = f.direct(a.id).await;
    let other = f.direct(b.id).await;
    settings(&f, c.id, |s| s.enabled = true).await;
    let i = intent(&f, c.id, a.id, 60).await;
    let cmd = CancelContactIntent {
        conversation_id: c.id,
        intent_id: i.id,
        expected_revision: i.revision,
    };
    let mut wrong = cmd.clone();
    wrong.conversation_id = other.id;
    assert!(f.store.cancel_contact_intent(f.ctx(), wrong).await.is_err());
    f.inject();
    assert!(
        f.store
            .cancel_contact_intent(f.ctx(), cmd.clone())
            .await
            .is_err()
    );
    assert_eq!(
        current(&f, c.id, i.id).await.status,
        ContactIntentStatus::Pending
    );
    f.clear();
    let ctx = f.ctx();
    let saved = f
        .store
        .cancel_contact_intent(ctx.clone(), cmd.clone())
        .await
        .unwrap();
    assert_eq!(
        f.store.cancel_contact_intent(ctx, cmd).await.unwrap(),
        saved
    );
    f.close().await;
}

#[tokio::test]
async fn budgets_failure_and_source_expiry_never_publish_or_replay() {
    let f = Fixture::new().await;
    let a = f.ai("A").await;
    let c = f.direct(a.id).await;
    settings(&f, c.id, |s| s.enabled = true).await;
    participation_tests::policy(&f, c.id, |p| p.daily_token_budget = 1024).await;
    let i = intent(&f, c.id, a.id, 1).await;
    f.clock.set(i.due_at);
    let p = tick(&f, c.id).await.plan.unwrap();
    assert!(matches!(
        f.begin_plan(&p).await,
        Err(SocialStorageError::ParticipationLimit { .. })
    ));
    assert!(
        f.store
            .messages(f.human.id, c.id, Sequence::ZERO, 200)
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        f.store
            .conversation_record(f.human.id, c.id)
            .await
            .unwrap()
            .participation
            .auto_turns_today,
        0
    );
    f.store
        .fail_plan(
            f.human.id,
            p.id,
            "budget".into(),
            Some(ParticipationReason::TokenLimit),
        )
        .await
        .unwrap();
    assert_eq!(
        current(&f, c.id, i.id).await.status,
        ContactIntentStatus::Failed
    );
    participation_tests::policy(&f, c.id, |p| p.daily_token_budget = 262144).await;
    let i = intent(&f, c.id, a.id, 1).await;
    f.clock.set(i.due_at);
    let p = tick(&f, c.id).await.plan.unwrap();
    let r = f.begin_plan(&p).await.unwrap();
    f.clock.set(i.expires_at);
    assert!(result(&f, &r).await.is_err());
    assert_ne!(
        current(&f, c.id, i.id).await.status,
        ContactIntentStatus::Delivered
    );
    f.close().await;
}

#[tokio::test]
async fn pause_resume_archive_and_membership_change_keep_authorization_boundaries() {
    let f = Fixture::new().await;
    let a = f.ai("A").await;
    let c = f.group(vec![a.id]).await;
    settings(&f, c.id, |s| s.enabled = true).await;
    let i = intent(&f, c.id, a.id, 10).await;
    let paused = f
        .store
        .control_conversation(
            f.ctx(),
            ConversationControl {
                conversation_id: c.id,
                expected_revision: c.revision,
                action: ConversationAction::Pause,
            },
        )
        .await
        .unwrap();
    f.clock.set(i.due_at);
    f.store
        .control_conversation(
            f.ctx(),
            ConversationControl {
                conversation_id: c.id,
                expected_revision: paused.revision,
                action: ConversationAction::Resume,
            },
        )
        .await
        .unwrap();
    assert_eq!(
        current(&f, c.id, i.id).await.status,
        ContactIntentStatus::Skipped
    );
    let left = intent(&f, c.id, a.id, 60).await;
    let c = f.store.conversation(f.human.id, c.id).await.unwrap();
    let removed = f
        .store
        .change_membership(
            f.ctx(),
            ChangeMembership {
                conversation_id: c.id,
                actor_id: a.id,
                action: MembershipAction::Remove,
                expected_revision: c.revision,
            },
        )
        .await
        .unwrap();
    assert_eq!(
        current(&f, c.id, left.id).await.status,
        ContactIntentStatus::Cancelled
    );
    f.store
        .change_membership(
            f.ctx(),
            ChangeMembership {
                conversation_id: c.id,
                actor_id: a.id,
                action: MembershipAction::Add,
                expected_revision: removed.revision,
            },
        )
        .await
        .unwrap();
    let archived = intent(&f, c.id, a.id, 60).await;
    f.store
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
    assert_eq!(
        current(&f, c.id, archived.id).await.status,
        ContactIntentStatus::Cancelled
    );
    f.close().await;
}

#[tokio::test]
async fn restart_keeps_unstarted_work_but_marks_started_contact_uncertain() {
    let f = Fixture::new().await;
    let a = f.ai("A").await;
    let c = f.direct(a.id).await;
    settings(&f, c.id, |s| s.enabled = true).await;
    let started = intent(&f, c.id, a.id, 1).await;
    let claimed = intent(&f, c.id, a.id, 1).await;
    let pending = intent(&f, c.id, a.id, 60).await;
    f.clock.set(started.due_at);
    let p = tick(&f, c.id).await.plan.unwrap();
    let r = f.begin_plan(&p).await.unwrap();
    assert_eq!(p.contact_intent_id, Some(started.id));
    let path = f.dir.path().to_owned();
    f.store.close().await.unwrap();
    let store =
        SqliteSocialStore::open(DataDirectoryGuard::acquire(&path).unwrap(), f.clock.clone())
            .await
            .unwrap();
    let view = store
        .conversation_record(f.human.id, c.id)
        .await
        .unwrap()
        .contact_schedule;
    assert_eq!(
        view.intents
            .iter()
            .find(|i| i.id == started.id)
            .unwrap()
            .status,
        ContactIntentStatus::Uncertain
    );
    assert_eq!(
        store.run(f.human.id, r.id).await.unwrap().status,
        RunStatus::Interrupted
    );
    assert_eq!(
        view.intents
            .iter()
            .find(|i| i.id == pending.id)
            .unwrap()
            .status,
        ContactIntentStatus::Pending
    );
    assert_eq!(
        store
            .participation_tick(f.human.id, c.id)
            .await
            .unwrap()
            .plan
            .unwrap()
            .contact_intent_id,
        Some(claimed.id)
    );
    assert!(
        store
            .queued_conversations(f.human.id)
            .await
            .unwrap()
            .contains(&c.id)
    );
    store.close().await.unwrap();
}

#[tokio::test]
async fn v10_migration_preserves_queued_message_plans_and_defaults_to_no_consent() {
    let f = Fixture::new().await;
    let a = f.ai("A").await;
    let c = f.direct(a.id).await;
    let sent = f.send_group(c.id, vec![]).await;
    let path = f.dir.path().to_owned();
    f.store.close().await.unwrap();
    let sql = Connection::open(path.join("social.sqlite3")).unwrap();
    downgrade_contact_schema(&sql);
    drop(sql);
    let store =
        SqliteSocialStore::open(DataDirectoryGuard::acquire(&path).unwrap(), f.clock.clone())
            .await
            .unwrap();
    let v = store.conversation_record(f.human.id, c.id).await.unwrap();
    assert!(!v.contact_schedule.settings.enabled);
    assert!(v.contact_schedule.intents.is_empty());
    assert_eq!(v.pending_plans[0].trigger_id, Some(sent.message.id));
    assert_eq!(v.pending_plans[0].id, sent.plans[0].id);
    let sql = Connection::open(store.path()).unwrap();
    assert_eq!(
        sql.query_row::<i64, _, _>("SELECT count(*) FROM pragma_foreign_key_check", [], |r| r
            .get(0))
            .unwrap(),
        0
    );
    drop(sql);
    store.close().await.unwrap();
}

#[tokio::test]
async fn overdue_idle_bout_closes_before_a_future_contact_after_downtime() {
    let f = Fixture::new().await;
    let a = f.ai("A").await;
    let b = f.ai("B").await;
    let c = f.group(vec![a.id, b.id]).await;
    settings(&f, c.id, |s| s.enabled = true).await;
    participation_tests::policy(&f, c.id, |p| p.limited_auto = true).await;
    f.send_group(c.id, vec![]).await;
    for _ in 0..3 {
        let p = f.store.next_plan(f.human.id, c.id).await.unwrap().unwrap();
        let r = f.begin_plan(&p).await.unwrap();
        result(&f, &r).await.unwrap();
    }
    let before = f.store.conversation_record(f.human.id, c.id).await.unwrap();
    assert_eq!(before.participation.state.consecutive_ai, 3);
    let i = intent(&f, c.id, a.id, 600).await;
    f.clock.set(i.due_at);
    let p = tick(&f, c.id).await.plan.unwrap();
    let run = f.begin_plan(&p).await.unwrap();
    assert_ne!(run.bout_id, before.current_bout.unwrap().id);
    result(&f, &run).await.unwrap();
    assert_eq!(
        current(&f, c.id, i.id).await.status,
        ContactIntentStatus::Delivered
    );
    f.close().await;
}
