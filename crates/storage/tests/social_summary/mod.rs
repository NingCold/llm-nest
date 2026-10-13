use super::*;

async fn prepare(
    f: &Fixture,
    c: ConversationId,
    count: usize,
) -> (SummarySettings, Vec<SocialMessage>) {
    let settings = f
        .store
        .configure_summaries(
            f.ctx(),
            c,
            ConfigureSummaries {
                enabled: true,
                binding: Some(SummaryBinding {
                    provider_id: "mock".into(),
                    model_id: "summary".into(),
                    reasoning_effort: None,
                    max_tokens: 1024,
                }),
                expected_revision: Revision::ZERO,
            },
        )
        .await
        .unwrap();
    let mut messages = Vec::new();
    for i in 0..count {
        messages.push(f.append(c, &format!("source {i}")).await);
    }
    (settings, messages)
}
fn queue(c: ConversationId, revision: Revision) -> QueueSummary {
    QueueSummary {
        conversation_id: c,
        retry: None,
        automatic: true,
        settings_revision: revision,
        input_budget: 65536,
    }
}
fn finish(s: &ConversationSummary) -> FinishSummary {
    FinishSummary {
        id: s.id,
        expected_revision: s.revision,
        status: SummaryStatus::Ready,
        text: "compressed background".into(),
        error: None,
    }
}

#[tokio::test]
async fn segment_boundary_waits_for_unfinished_messages_in_its_range() {
    let f = Fixture::new().await;
    let ai = f.ai("A").await;
    let c = f.direct(ai.id).await;
    let (settings, _) = prepare(&f, c.id, 20).await;
    let run = f.begin(ai.id, c.id, vec![]).await;
    for _ in 0..20 {
        f.append(c.id, "new input while reply is generating").await;
    }
    let q = f
        .store
        .queue_summary(f.ctx(), queue(c.id, settings.revision))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        q.through_sequence.get(),
        20,
        "a later commit must not fall into an already summarized range"
    );
    let work = f
        .store
        .begin_summary(f.human.id, q.id)
        .await
        .unwrap()
        .unwrap();
    assert!(!work.messages.iter().any(|m| m.id == run.message_id));
    f.close().await;
}

#[tokio::test]
async fn segments_are_atomic_nonoverlapping_idempotent_and_source_checked() {
    let f = Fixture::new().await;
    let a = f.ai("A").await;
    let c = f.direct(a.id).await;
    let (settings, messages) = prepare(&f, c.id, 28).await;
    let input = queue(c.id, settings.revision);
    let ctx = f.ctx();
    let before = f.counts();
    f.inject();
    assert!(
        f.store
            .queue_summary(ctx.clone(), input.clone())
            .await
            .is_err()
    );
    assert_eq!(f.counts(), before);
    f.clear();
    let queued = f
        .store
        .queue_summary(ctx.clone(), input.clone())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        (queued.from_sequence.get(), queued.through_sequence.get()),
        (1, 16)
    );
    assert_eq!(queued.sources.len(), 16);
    assert_eq!(
        f.store.queue_summary(ctx, input.clone()).await.unwrap(),
        Some(queued.clone())
    );
    assert!(f.store.queue_summary(f.ctx(), input.clone()).await.is_err());
    let work = f
        .store
        .begin_summary(f.human.id, queued.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(work.messages, messages[..16]);
    let ready = f
        .store
        .finish_summary(f.human.id, finish(&work.summary))
        .await
        .unwrap();
    assert_eq!(ready.status, SummaryStatus::Ready);
    assert!(
        f.store
            .queue_summary(f.ctx(), input.clone())
            .await
            .unwrap()
            .is_none()
    );
    let run = f
        .begin(
            a.id,
            c.id,
            vec![SourceRef::Summary {
                summary_id: ready.id,
                revision: ready.revision,
            }],
        )
        .await;
    f.store
        .revise_message(
            f.ctx(),
            ReviseMessage {
                message_id: messages[0].id,
                expected_revision: messages[0].revision,
                body: MessageBody::text("corrected source"),
            },
        )
        .await
        .unwrap();
    let stale = f
        .store
        .summaries(f.human.id, c.id, None)
        .await
        .unwrap()
        .segments
        .remove(0);
    assert_eq!(stale.status, SummaryStatus::Stale);
    assert!(stale.text.is_empty());
    assert_eq!(
        f.store.run(f.human.id, run.id).await.unwrap().status,
        RunStatus::Interrupted
    );
    let late = f
        .store
        .finish_summary(f.human.id, finish(&work.summary))
        .await
        .unwrap();
    assert_eq!(late, stale);
    let mut retry = input.clone();
    retry.retry = Some((stale.id, stale.revision));
    retry.automatic = false;
    let q = f
        .store
        .queue_summary(f.ctx(), retry)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(q.id, ready.id);
    assert_eq!(q.attempt.get(), 2);
    let work = f
        .store
        .begin_summary(f.human.id, q.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(work.messages[0].body, MessageBody::text("corrected source"));
    f.store
        .withdraw_message(
            f.ctx(),
            WithdrawMessage {
                message_id: messages[1].id,
                expected_revision: messages[1].revision,
            },
        )
        .await
        .unwrap();
    let stale = f
        .store
        .finish_summary(f.human.id, finish(&work.summary))
        .await
        .unwrap();
    assert_eq!(stale.status, SummaryStatus::Stale);
    let leaked: i64 = f
        .sql()
        .query_row(
            "SELECT count(*) FROM summary_revisions WHERE json_extract(payload,'$.text')!=''",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(leaked, 0);
    assert_eq!(
        f.store
            .messages(f.human.id, c.id, Sequence::ZERO, 100)
            .await
            .unwrap()
            .len(),
        29,
        "original messages and interrupted reply remain"
    );
    let material = f
        .store
        .context_material(
            f.human.id,
            ContextScope {
                actor_id: a.id,
                conversation_id: c.id,
            },
            messages.last().unwrap().id,
        )
        .await
        .unwrap();
    assert!(material.summaries.is_empty());
    f.close().await;
}

#[tokio::test]
async fn shared_group_summaries_do_not_grant_other_window_access() {
    let f = Fixture::new().await;
    let a = f.ai("A").await;
    let b = f.ai("B").await;
    let group = f.group(vec![a.id]).await;
    let private = f.direct(a.id).await;
    let (settings, messages) = prepare(&f, group.id, 28).await;
    let q = f
        .store
        .queue_summary(f.ctx(), queue(group.id, settings.revision))
        .await
        .unwrap()
        .unwrap();
    let work = f
        .store
        .begin_summary(f.human.id, q.id)
        .await
        .unwrap()
        .unwrap();
    let ready = f
        .store
        .finish_summary(f.human.id, finish(&work.summary))
        .await
        .unwrap();
    assert!(matches!(
        f.store.summaries(b.id, group.id, None).await,
        Err(SocialStorageError::Forbidden(_))
    ));
    let c = f.store.conversation(f.human.id, group.id).await.unwrap();
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
    assert_eq!(
        f.store.summaries(b.id, c.id, None).await.unwrap().segments[0],
        ready
    );
    let material = f
        .store
        .context_material(
            f.human.id,
            ContextScope {
                actor_id: b.id,
                conversation_id: c.id,
            },
            messages.last().unwrap().id,
        )
        .await
        .unwrap();
    assert_eq!(material.summaries[0], ready);
    let snapshot = f
        .snapshot(
            a.id,
            private.id,
            vec![SourceRef::Summary {
                summary_id: ready.id,
                revision: ready.revision,
            }],
        )
        .await;
    assert!(matches!(
        f.store
            .begin_run(
                f.ctx(),
                BeginRun {
                    snapshot,
                    expected_revision: private.revision,
                    details: None
                }
            )
            .await,
        Err(SocialStorageError::Forbidden(_))
    ));
    let c = f.store.conversation(f.human.id, group.id).await.unwrap();
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
    assert!(f.store.summaries(b.id, c.id, None).await.is_err());
    f.close().await;
}

#[tokio::test]
async fn settings_stop_inflight_tasks_and_restart_never_replays_them() {
    let f = Fixture::new().await;
    let a = f.ai("A").await;
    let c = f.direct(a.id).await;
    let (settings, _) = prepare(&f, c.id, 44).await;
    let q = f
        .store
        .queue_summary(f.ctx(), queue(c.id, settings.revision))
        .await
        .unwrap()
        .unwrap();
    let work = f
        .store
        .begin_summary(f.human.id, q.id)
        .await
        .unwrap()
        .unwrap();
    let disabled = f
        .store
        .configure_summaries(
            f.ctx(),
            c.id,
            ConfigureSummaries {
                enabled: false,
                binding: settings.binding.clone(),
                expected_revision: settings.revision,
            },
        )
        .await
        .unwrap();
    assert_eq!(
        f.store
            .finish_summary(f.human.id, finish(&work.summary))
            .await
            .unwrap()
            .status,
        SummaryStatus::Interrupted
    );
    let settings = f
        .store
        .configure_summaries(
            f.ctx(),
            c.id,
            ConfigureSummaries {
                enabled: true,
                binding: settings.binding,
                expected_revision: disabled.revision,
            },
        )
        .await
        .unwrap();
    let old = f
        .store
        .summaries(f.human.id, c.id, None)
        .await
        .unwrap()
        .segments
        .remove(0);
    let mut input = queue(c.id, settings.revision);
    input.retry = Some((old.id, old.revision));
    let q = f
        .store
        .queue_summary(f.ctx(), input)
        .await
        .unwrap()
        .unwrap();
    f.store.close().await.unwrap();
    let store =
        SqliteSocialStore::open(DataDirectoryGuard::acquire(f.dir.path()).unwrap(), f.clock)
            .await
            .unwrap();
    let state = store.summaries(f.human.id, c.id, None).await.unwrap();
    assert_eq!(state.segments[0].status, SummaryStatus::Interrupted);
    assert!(
        store
            .begin_summary(f.human.id, q.id)
            .await
            .unwrap()
            .is_none()
    );
    store.close().await.unwrap();
}

#[tokio::test]
async fn v7_upgrade_preserves_explicit_memory_and_backup() {
    let f = Fixture::new().await;
    let a = f.ai("A").await;
    let c = f.direct(a.id).await;
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
                    kind: MemoryKind::Fact,
                    text: "v7 memory".into(),
                    source: None,
                },
            },
        )
        .await
        .unwrap();
    let path = f.store.path().to_owned();
    f.store.close().await.unwrap();
    let sql = Connection::open(&path).unwrap();
    sql.execute_batch("DROP TABLE summary_message_sources; DROP TABLE summary_revisions; DROP TABLE conversation_summaries; DROP TABLE summary_settings; DELETE FROM schema_migrations WHERE version=8; PRAGMA user_version=7;").unwrap();
    drop(sql);
    let store =
        SqliteSocialStore::open(DataDirectoryGuard::acquire(f.dir.path()).unwrap(), f.clock)
            .await
            .unwrap();
    assert_eq!(store.memory(f.human.id, memory.id).await.unwrap(), memory);
    assert!(
        !store
            .summaries(f.human.id, c.id, None)
            .await
            .unwrap()
            .settings
            .enabled
    );
    let backup = fs::read_dir(f.dir.path())
        .unwrap()
        .map(|e| e.unwrap().path())
        .find(|p| {
            p.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("social.before-v8-")
                && Connection::open(p)
                    .unwrap()
                    .query_row::<i64, _, _>("SELECT count(*) FROM memories", [], |r| r.get(0))
                    .unwrap()
                    == 1
        })
        .unwrap();
    assert_eq!(
        Connection::open(backup)
            .unwrap()
            .pragma_query_value::<i64, _>(None, "user_version", |r| r.get(0))
            .unwrap(),
        7
    );
    store.close().await.unwrap();
}
