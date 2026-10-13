use super::*;
use chrono::Duration as Delta;

async fn view(f: &Fixture, c: ConversationId) -> ParticipationView {
    f.store
        .conversation_record(f.human.id, c)
        .await
        .unwrap()
        .participation
}
async fn policy(f: &Fixture, c: ConversationId, edit: impl FnOnce(&mut ParticipationPolicy)) {
    let mut p = view(f, c).await.policy;
    edit(&mut p);
    f.store
        .update_participation(
            f.ctx(),
            UpdateParticipation {
                conversation_id: c,
                policy: p,
            },
        )
        .await
        .unwrap();
}
async fn finish(f: &Fixture, r: &AiRun) {
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
        .unwrap();
}
async fn next(f: &Fixture, c: ConversationId) -> ParticipationPlan {
    f.store.next_plan(f.human.id, c).await.unwrap().unwrap()
}

#[tokio::test]
async fn continuous_inputs_keep_originals_and_receipts_and_cancel_as_one_plan() {
    let f = Fixture::new().await;
    let a = f.ai("A").await;
    let c = f.direct(a.id).await;
    let first = f.send_group(c.id, vec![]).await;
    let input = SendMessage {
        conversation_id: c.id,
        body: MessageBody::text("second"),
        reply_to: None,
        mentions: vec![],
        reference: None,
    };
    let ctx = f.ctx();
    let second = f
        .store
        .send_message(ctx.clone(), input.clone())
        .await
        .unwrap();
    assert_ne!(first.message.id, second.message.id);
    assert_eq!(first.plans[0].id, second.plans[0].id);
    assert_eq!(
        second.plans[0].input_ids,
        vec![first.message.id, second.message.id]
    );
    let counts = f.counts();
    f.store.send_message(ctx, input).await.unwrap();
    assert_eq!(counts, f.counts());
    let tick = f.store.participation_tick(f.human.id, c.id).await.unwrap();
    assert!(tick.plan.is_none());
    assert_eq!(tick.wake_at, second.plans[0].not_before);
    f.store
        .withdraw_message(
            f.ctx(),
            WithdrawMessage {
                message_id: first.message.id,
                expected_revision: first.message.revision,
            },
        )
        .await
        .unwrap();
    assert!(f.store.next_plan(f.human.id, c.id).await.unwrap().is_none());
    assert_eq!(
        f.store
            .message(f.human.id, second.message.id)
            .await
            .unwrap()
            .status,
        MessageStatus::Committed
    );
    let plain = f.send_group(c.id, vec![]).await;
    let quote = f
        .store
        .send_message(
            f.ctx(),
            SendMessage {
                conversation_id: c.id,
                body: MessageBody::text("quoted"),
                reply_to: Some(second.message.id),
                mentions: vec![],
                reference: None,
            },
        )
        .await
        .unwrap();
    assert_ne!(plain.plans[0].id, quote.plans[0].id);
    f.close().await;
}

#[tokio::test]
async fn new_input_never_changes_a_running_snapshot_or_reply_target() {
    let f = Fixture::new().await;
    let a = f.ai("A").await;
    let c = f.direct(a.id).await;
    let first = f.send_group(c.id, vec![]).await;
    let r = f.begin_plan(&first.plans[0]).await.unwrap();
    let second = f.send_group(c.id, vec![]).await;
    assert_ne!(first.plans[0].id, second.plans[0].id);
    assert_eq!(
        f.store.run(f.human.id, r.id).await.unwrap().snapshot,
        r.snapshot
    );
    assert!(
        !r.snapshot.sources.iter().any(
            |s| matches!(s,SourceRef::Message{message_id,..} if *message_id==second.message.id)
        )
    );
    finish(&f, &r).await;
    assert_eq!(
        f.store
            .message(f.human.id, r.message_id)
            .await
            .unwrap()
            .reply_to,
        Some(first.message.id)
    );
    assert_eq!(next(&f, c.id).await.id, second.plans[0].id);
    f.close().await;
}

#[tokio::test]
async fn human_input_during_automatic_generation_keeps_a_later_authorized_plan() {
    let f = Fixture::new().await;
    let a = f.ai("A").await;
    let b = f.ai("B").await;
    let c = f.group(vec![a.id, b.id]).await;
    policy(&f, c.id, |p| p.limited_auto = true).await;
    f.send_group(c.id, vec![]).await;
    let first = next(&f, c.id).await;
    let r = f.begin_plan(&first).await.unwrap();
    let second = f.send_group(c.id, vec![]).await;
    let plans = f.store.plans(f.human.id, c.id).await.unwrap();
    assert!(plans.iter().any(|p| p.status == PlanStatus::Queued
        && p.automatic
        && p.trigger_id == second.message.id));
    finish(&f, &r).await;
    let following = next(&f, c.id).await;
    assert_eq!(following.trigger_id, second.message.id);
    let next_run = f.begin_plan(&following).await.unwrap();
    finish(&f, &next_run).await;
    assert_eq!(view(&f, c.id).await.auto_turns_today, 2);
    f.close().await;
}

#[tokio::test]
async fn automatic_group_turns_require_consent_and_stop_at_consecutive_and_bout_limits() {
    let f = Fixture::new().await;
    let a = f.ai("A").await;
    let b = f.ai("B").await;
    let c = f.group(vec![a.id, b.id]).await;
    f.send_group(c.id, vec![]).await;
    assert!(f.store.next_plan(f.human.id, c.id).await.unwrap().is_none());
    policy(&f, c.id, |p| {
        p.limited_auto = true;
        p.max_auto_per_bout = 4;
    })
    .await;
    assert!(f.store.next_plan(f.human.id, c.id).await.unwrap().is_none());
    f.send_group(c.id, vec![]).await;
    let mut previous = None;
    for _ in 0..3 {
        let p = next(&f, c.id).await;
        assert!(p.automatic);
        assert_ne!(Some(p.actor_id), previous);
        let r = f.begin_plan(&p).await.unwrap();
        finish(&f, &r).await;
        previous = Some(p.actor_id);
    }
    let v = view(&f, c.id).await;
    assert_eq!(v.state.reason, Some(ParticipationReason::TurnLimit));
    assert_eq!(v.auto_turns_today, 3);
    assert_eq!(v.reserved_tokens_today, 3 * 9216);
    assert!(f.store.next_plan(f.human.id, c.id).await.unwrap().is_none());
    f.send_group(c.id, vec![]).await;
    let r = f.begin_plan(&next(&f, c.id).await).await.unwrap();
    finish(&f, &r).await;
    assert_eq!(view(&f, c.id).await.state.bout_auto_turns, 4);
    assert!(f.store.next_plan(f.human.id, c.id).await.unwrap().is_none());
    // Explicit mentions still work after automatic budgets are spent.
    let explicit = f.send_group(c.id, vec![a.id]).await;
    assert!(!explicit.plans[0].automatic);
    let r = f.begin_plan(&explicit.plans[0]).await.unwrap();
    finish(&f, &r).await;
    assert_eq!(view(&f, c.id).await.auto_turns_today, 4);
    f.close().await;
}

#[tokio::test]
async fn reservations_roll_back_before_dispatch_and_survive_crashes_and_clock_rollback() {
    let f = Fixture::new().await;
    let a = f.ai("A").await;
    let b = f.ai("B").await;
    let c = f.group(vec![a.id, b.id]).await;
    policy(&f, c.id, |p| {
        p.limited_auto = true;
        p.daily_token_budget = 1024;
    })
    .await;
    f.send_group(c.id, vec![]).await;
    let p = next(&f, c.id).await;
    let counts = f.counts();
    assert!(f.begin_plan(&p).await.is_err());
    assert_eq!(counts, f.counts());
    assert_eq!(view(&f, c.id).await.reserved_tokens_today, 0);
    policy(&f, c.id, |p| {
        p.daily_token_budget = 18432;
        p.max_auto_per_day = 2;
    })
    .await;
    f.send_group(c.id, vec![]).await;
    let r = f.begin_plan(&next(&f, c.id).await).await.unwrap();
    let spent = view(&f, c.id).await;
    f.store.close().await.unwrap();
    let reopened = SqliteSocialStore::open(
        DataDirectoryGuard::acquire(f.dir.path()).unwrap(),
        f.clock.clone(),
    )
    .await
    .unwrap();
    assert_eq!(
        reopened.run(f.human.id, r.id).await.unwrap().status,
        RunStatus::Interrupted
    );
    f.clock.set(f.clock.now() - Delta::days(1));
    let v = reopened
        .conversation_record(f.human.id, c.id)
        .await
        .unwrap()
        .participation;
    assert_eq!(v.budget_day, spent.budget_day);
    assert_eq!(v.reserved_tokens_today, 9216);
    assert_eq!(v.auto_turns_today, 1);
    assert!(
        reopened
            .next_plan(f.human.id, c.id)
            .await
            .unwrap()
            .is_none()
    );
    reopened.close().await.unwrap();
}

#[tokio::test]
async fn policy_changes_cancel_automatic_runs_but_preserve_explicit_queue_and_daily_debits() {
    let f = Fixture::new().await;
    let a = f.ai("A").await;
    let b = f.ai("B").await;
    let c = f.group(vec![a.id, b.id]).await;
    policy(&f, c.id, |p| p.limited_auto = true).await;
    f.send_group(c.id, vec![]).await;
    let r = f.begin_plan(&next(&f, c.id).await).await.unwrap();
    let explicit = f.send_group(c.id, vec![b.id]).await;
    policy(&f, c.id, |p| p.limited_auto = false).await;
    assert_eq!(
        f.store.run(f.human.id, r.id).await.unwrap().status,
        RunStatus::Cancelled
    );
    assert_eq!(next(&f, c.id).await.id, explicit.plans[0].id);
    assert_eq!(view(&f, c.id).await.auto_turns_today, 1);
    let rev = f
        .store
        .conversation(f.human.id, c.id)
        .await
        .unwrap()
        .revision;
    f.store
        .end_conversation(
            f.ctx(),
            EndConversation {
                conversation_id: c.id,
                expected_revision: rev,
            },
        )
        .await
        .unwrap();
    assert!(f.store.next_plan(f.human.id, c.id).await.unwrap().is_none());
    assert_eq!(view(&f, c.id).await.auto_turns_today, 1);
    f.close().await;
}

#[tokio::test]
async fn idle_expiry_and_restart_need_no_model_and_never_end_a_running_bout() {
    let f = Fixture::new().await;
    let a = f.ai("A").await;
    let c = f.direct(a.id).await;
    policy(&f, c.id, |p| p.idle_seconds = 30).await;
    let sent = f.send_group(c.id, vec![]).await;
    let p = &sent.plans[0];
    f.clock.set(p.expires_at.unwrap());
    let tick = f.store.participation_tick(f.human.id, c.id).await.unwrap();
    assert!(tick.plan.is_none());
    assert_eq!(
        view(&f, c.id).await.state.reason,
        Some(ParticipationReason::Expired)
    );
    let sent = f.send_group(c.id, vec![]).await;
    let r = f.begin_plan(&sent.plans[0]).await.unwrap();
    f.clock.set(f.clock.now() + Delta::minutes(1));
    f.store.participation_tick(f.human.id, c.id).await.unwrap();
    assert_eq!(
        f.store.run(f.human.id, r.id).await.unwrap().status,
        RunStatus::Running
    );
    finish(&f, &r).await;
    assert!(
        f.store
            .participation_tick(f.human.id, c.id)
            .await
            .unwrap()
            .wake_at
            .is_some()
    );
    f.store.close().await.unwrap();
    f.clock.set(f.clock.now() + Delta::seconds(30));
    let reopened = SqliteSocialStore::open(
        DataDirectoryGuard::acquire(f.dir.path()).unwrap(),
        f.clock.clone(),
    )
    .await
    .unwrap();
    assert!(
        reopened
            .queued_conversations(f.human.id)
            .await
            .unwrap()
            .contains(&c.id)
    );
    reopened.participation_tick(f.human.id, c.id).await.unwrap();
    let record = reopened
        .conversation_record(f.human.id, c.id)
        .await
        .unwrap();
    assert!(record.current_bout.is_none());
    assert_eq!(
        record.participation.state.reason,
        Some(ParticipationReason::Idle)
    );
    assert_eq!(
        reopened
            .message_page(f.human.id, c.id, None)
            .await
            .unwrap()
            .0
            .len(),
        3
    );
    reopened.close().await.unwrap();
}

#[tokio::test]
async fn policy_and_coalescing_are_versioned_authorized_and_transactional() {
    let f = Fixture::new().await;
    let a = f.ai("A").await;
    let c = f.direct(a.id).await;
    let mut p = view(&f, c.id).await.policy;
    p.merge_delay_ms = 1200;
    let input = UpdateParticipation {
        conversation_id: c.id,
        policy: p,
    };
    let ctx = f.ctx();
    let counts = f.counts();
    f.inject();
    assert!(
        f.store
            .update_participation(ctx.clone(), input.clone())
            .await
            .is_err()
    );
    f.clear();
    assert_eq!(counts, f.counts());
    f.store
        .update_participation(ctx.clone(), input.clone())
        .await
        .unwrap();
    let counts = f.counts();
    f.store
        .update_participation(ctx, input.clone())
        .await
        .unwrap();
    assert_eq!(counts, f.counts());
    assert!(
        f.store
            .update_participation(f.ctx(), input.clone())
            .await
            .is_err()
    );
    assert!(
        f.store
            .update_participation(
                CommandContext {
                    operator: a.id,
                    request_id: CommandId::new()
                },
                input
            )
            .await
            .is_err()
    );
    let first = f.send_group(c.id, vec![]).await;
    let counts = f.counts();
    f.inject();
    assert!(
        f.store
            .send_message(
                f.ctx(),
                SendMessage {
                    conversation_id: c.id,
                    body: MessageBody::text("rollback"),
                    reply_to: None,
                    mentions: vec![],
                    reference: None
                }
            )
            .await
            .is_err()
    );
    f.clear();
    assert_eq!(counts, f.counts());
    assert_eq!(next(&f, c.id).await, first.plans[0]);
    f.close().await;
}

#[tokio::test]
async fn v9_upgrade_preserves_history_and_backs_up_before_new_participation_tables() {
    let f = Fixture::new().await;
    let a = f.ai("A").await;
    let c = f.direct(a.id).await;
    let m = f.append(c.id, "history").await;
    let path = f.store.path().to_owned();
    f.store.close().await.unwrap();
    let sql = Connection::open(&path).unwrap();
    sql.execute_batch("DROP TABLE participation_daily_budget; DROP TABLE participation_state; DROP TABLE participation_policies; DELETE FROM schema_migrations WHERE version=10; PRAGMA user_version=9;").unwrap();
    drop(sql);
    let reopened = SqliteSocialStore::open(
        DataDirectoryGuard::acquire(f.dir.path()).unwrap(),
        f.clock.clone(),
    )
    .await
    .unwrap();
    assert_eq!(reopened.message(f.human.id, m.id).await.unwrap(), m);
    assert!(
        !reopened
            .conversation_record(f.human.id, c.id)
            .await
            .unwrap()
            .participation
            .policy
            .limited_auto
    );
    assert!(fs::read_dir(f.dir.path()).unwrap().any(|e| {
        e.unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with("social.before-v10-")
    }));
    reopened.close().await.unwrap();
}
