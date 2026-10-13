use super::*;

fn input(c: ConversationId, source_message: &SocialMessage) -> SendMessage {
    SendMessage {
        conversation_id: c,
        body: MessageBody::text("continue privately"),
        reply_to: None,
        mentions: vec![],
        reference: Some(ReferenceSelection {
            conversation_id: source_message.conversation_id,
            sources: vec![source(source_message)],
        }),
    }
}

#[tokio::test]
async fn selected_summary_tracks_all_originals_and_invalidation_rolls_back_atomically() {
    let f = Fixture::new().await;
    let a = f.ai("A").await;
    let g = f.group(vec![a.id]).await;
    let d = f.direct(a.id).await;
    let settings = f
        .store
        .configure_summaries(
            f.ctx(),
            g.id,
            ConfigureSummaries {
                enabled: true,
                binding: Some(SummaryBinding {
                    provider_id: "mock".into(),
                    model_id: "summary".into(),
                    reasoning_effort: None,
                    max_tokens: 512,
                }),
                expected_revision: Revision::ZERO,
            },
        )
        .await
        .unwrap();
    let mut originals = Vec::new();
    for i in 0..28 {
        originals.push(f.append(g.id, &format!("point {i}")).await);
    }
    let queued = f
        .store
        .queue_summary(
            f.ctx(),
            QueueSummary {
                conversation_id: g.id,
                retry: None,
                automatic: true,
                settings_revision: settings.revision,
                input_budget: 65536,
            },
        )
        .await
        .unwrap()
        .unwrap();
    let running = f
        .store
        .begin_summary(f.human.id, queued.id)
        .await
        .unwrap()
        .unwrap()
        .summary;
    let ready = f
        .store
        .finish_summary(
            f.human.id,
            FinishSummary {
                id: running.id,
                expected_revision: running.revision,
                status: SummaryStatus::Ready,
                text: "selected segment".into(),
                error: None,
            },
        )
        .await
        .unwrap();
    let mut request = input(d.id, &originals[0]);
    request
        .reference
        .as_mut()
        .unwrap()
        .sources
        .push(SourceRef::Summary {
            summary_id: ready.id,
            revision: ready.revision,
        });
    let saved = f.store.send_message(f.ctx(), request).await.unwrap();
    let proof = SourceRef::Reference {
        message_id: saved.message.id,
        revision: saved.message.revision,
    };
    let run = f
        .begin(
            a.id,
            d.id,
            vec![
                source(&originals[0]),
                proof,
                SourceRef::Summary {
                    summary_id: ready.id,
                    revision: ready.revision,
                },
            ],
        )
        .await;
    let change = ReviseMessage {
        message_id: originals[1].id,
        expected_revision: originals[1].revision,
        body: MessageBody::text("changed covered original"),
    };
    let before = f.counts();
    f.inject();
    assert!(
        f.store
            .revise_message(f.ctx(), change.clone())
            .await
            .is_err()
    );
    f.clear();
    assert_eq!(f.counts(), before);
    assert!(
        !f.store
            .message(f.human.id, saved.message.id)
            .await
            .unwrap()
            .context_reference
            .unwrap()
            .stale
    );
    assert_eq!(
        f.store.run(f.human.id, run.id).await.unwrap().status,
        RunStatus::Running
    );
    f.store.revise_message(f.ctx(), change).await.unwrap();
    assert!(
        f.store
            .message(f.human.id, saved.message.id)
            .await
            .unwrap()
            .context_reference
            .unwrap()
            .stale
    );
    assert_eq!(
        f.store.run(f.human.id, run.id).await.unwrap().status,
        RunStatus::Interrupted
    );
    f.close().await;
}

#[tokio::test]
async fn references_are_atomic_idempotent_and_reopen_without_copying_original_text() {
    let f = Fixture::new().await;
    let a = f.ai("A").await;
    let group = f.group(vec![a.id]).await;
    let direct = f.direct(a.id).await;
    let original = f.append(group.id, "group-only-source-body").await;
    let request = input(direct.id, &original);
    let before = f.counts();
    f.inject();
    assert!(
        f.store
            .send_message(f.ctx(), request.clone())
            .await
            .is_err()
    );
    assert_eq!(before, f.counts());
    f.clear();
    let ctx = f.ctx();
    let saved = f
        .store
        .send_message(ctx.clone(), request.clone())
        .await
        .unwrap();
    assert_eq!(
        f.store
            .send_message(ctx.clone(), request.clone())
            .await
            .unwrap()
            .message,
        saved.message
    );
    assert_eq!(f.direct(a.id).await.id, direct.id);
    let reference = saved.message.context_reference.as_ref().unwrap();
    assert_eq!(
        reference.target,
        ContextScope {
            actor_id: a.id,
            conversation_id: direct.id
        }
    );
    assert_eq!(reference.initiated_by, f.human.id);
    assert!(
        !serde_json::to_string(&saved.message)
            .unwrap()
            .contains("group-only-source-body")
    );
    let path = f.dir.path().to_owned();
    f.store.close().await.unwrap();
    let store =
        SqliteSocialStore::open(DataDirectoryGuard::acquire(path).unwrap(), f.clock.clone())
            .await
            .unwrap();
    assert_eq!(
        store.send_message(ctx, request).await.unwrap().message,
        saved.message
    );
    assert_eq!(
        store
            .reference_preview(f.human.id, a.id, reference.selection.clone())
            .await
            .unwrap()
            .messages,
        vec![original]
    );
    store.close().await.unwrap();
}

#[tokio::test]
async fn references_require_common_group_current_versions_and_explicit_snapshot_proof() {
    let f = Fixture::new().await;
    let a = f.ai("A").await;
    let b = f.ai("B").await;
    let group = f.group(vec![a.id]).await;
    let direct = f.direct(a.id).await;
    let other = f.direct(b.id).await;
    let original = f.append(group.id, "selected").await;
    let selection = input(direct.id, &original).reference.unwrap();
    assert!(
        f.store
            .reference_preview(f.human.id, b.id, selection.clone())
            .await
            .is_err(),
        "being the administrator does not grant the AI group access"
    );
    assert!(
        f.store
            .send_message(f.ctx(), input(other.id, &original))
            .await
            .is_err()
    );
    assert!(
        f.store
            .send_message(f.ctx(), input(group.id, &original))
            .await
            .is_err()
    );
    let private_source = f.append(direct.id, "private").await;
    assert!(
        f.store
            .send_message(f.ctx(), input(direct.id, &private_source))
            .await
            .is_err()
    );
    let mut wrong = selection.clone();
    wrong.sources.push(SourceRef::Manual {
        author_id: f.human.id,
        revision: Revision::ONE,
    });
    assert!(
        f.store
            .reference_preview(f.human.id, a.id, wrong)
            .await
            .is_err()
    );
    let snapshot = f.snapshot(a.id, direct.id, vec![source(&original)]).await;
    assert!(
        f.store
            .begin_run(
                f.ctx(),
                BeginRun {
                    snapshot,
                    expected_revision: direct.revision,
                    details: None
                }
            )
            .await
            .is_err(),
        "membership alone cannot import arbitrary group history"
    );
    f.store
        .revise_message(
            f.ctx(),
            ReviseMessage {
                message_id: original.id,
                expected_revision: original.revision,
                body: MessageBody::text("revised"),
            },
        )
        .await
        .unwrap();
    assert!(
        f.store
            .send_message(f.ctx(), input(direct.id, &original))
            .await
            .is_err()
    );
    f.close().await;
}

#[tokio::test]
async fn withdrawal_and_leaving_stop_dependent_private_runs_and_do_not_revive_on_rejoin() {
    let f = Fixture::new().await;
    let a = f.ai("A").await;
    let group = f.group(vec![a.id]).await;
    let direct = f.direct(a.id).await;
    for leave in [false, true] {
        let original = f.append(group.id, "new group point").await;
        let saved = f
            .store
            .send_message(f.ctx(), input(direct.id, &original))
            .await
            .unwrap();
        let run = f
            .begin(
                a.id,
                direct.id,
                vec![
                    source(&original),
                    SourceRef::Reference {
                        message_id: saved.message.id,
                        revision: saved.message.revision,
                    },
                ],
            )
            .await;
        if leave {
            let c = f.store.conversation(f.human.id, group.id).await.unwrap();
            f.store
                .change_membership(
                    f.ctx(),
                    ChangeMembership {
                        conversation_id: group.id,
                        actor_id: a.id,
                        expected_revision: c.revision,
                        action: MembershipAction::Remove,
                    },
                )
                .await
                .unwrap();
        } else {
            f.store
                .withdraw_message(
                    f.ctx(),
                    WithdrawMessage {
                        message_id: original.id,
                        expected_revision: original.revision,
                    },
                )
                .await
                .unwrap();
        }
        assert_eq!(
            f.store.run(f.human.id, run.id).await.unwrap().status,
            RunStatus::Interrupted
        );
        assert!(
            f.store
                .finish_run(
                    f.ctx(),
                    FinishRun {
                        run_id: run.id,
                        expected_revision: run.revision,
                        status: RunStatus::Succeeded,
                        body: MessageBody::text("late"),
                        details: None
                    }
                )
                .await
                .is_err()
        );
        let message = f.store.message(f.human.id, saved.message.id).await.unwrap();
        assert!(message.context_reference.unwrap().stale);
        if leave {
            let c = f.store.conversation(f.human.id, group.id).await.unwrap();
            f.store
                .change_membership(
                    f.ctx(),
                    ChangeMembership {
                        conversation_id: group.id,
                        actor_id: a.id,
                        expected_revision: c.revision,
                        action: MembershipAction::Add,
                    },
                )
                .await
                .unwrap();
            assert!(
                f.store
                    .message(f.human.id, saved.message.id)
                    .await
                    .unwrap()
                    .context_reference
                    .unwrap()
                    .stale
            );
        }
        assert!(
            f.store
                .context_material(
                    f.human.id,
                    ContextScope {
                        actor_id: a.id,
                        conversation_id: direct.id
                    },
                    saved.message.id
                )
                .await
                .is_err()
        );
        assert_eq!(
            f.store
                .message(f.human.id, saved.message.id)
                .await
                .unwrap()
                .body,
            saved.message.body
        );
    }
    f.close().await;
}

#[tokio::test]
async fn continuation_expires_after_twelve_messages_but_explicit_reply_can_resume() {
    let f = Fixture::new().await;
    let a = f.ai("A").await;
    let group = f.group(vec![a.id]).await;
    let direct = f.direct(a.id).await;
    let original = f.append(group.id, "topic").await;
    let attached = f
        .store
        .send_message(f.ctx(), input(direct.id, &original))
        .await
        .unwrap()
        .message;
    let scope = ContextScope {
        actor_id: a.id,
        conversation_id: direct.id,
    };
    let next = f.append(direct.id, "follow up").await;
    assert!(
        !f.store
            .context_material(f.human.id, scope, next.id)
            .await
            .unwrap()
            .reference
            .unwrap()
            .required
    );
    for _ in 0..11 {
        f.append(direct.id, "other talk").await;
    }
    let last = f.append(direct.id, "new topic").await;
    assert!(
        f.store
            .context_material(f.human.id, scope, last.id)
            .await
            .unwrap()
            .reference
            .is_none()
    );
    let quoted = f
        .store
        .send_message(
            f.ctx(),
            SendMessage {
                conversation_id: direct.id,
                body: MessageBody::text("resume topic"),
                mentions: vec![],
                reply_to: Some(attached.id),
                reference: None,
            },
        )
        .await
        .unwrap();
    let material = f
        .store
        .context_material(f.human.id, scope, quoted.message.id)
        .await
        .unwrap()
        .reference
        .unwrap();
    assert!(material.required);
    assert_eq!(material.preview.messages, vec![original]);
    f.close().await;
}

#[tokio::test]
async fn v8_migration_preserves_history_and_backs_up_before_creating_reference_indexes() {
    let f = Fixture::new().await;
    let a = f.ai("A").await;
    let c = f.direct(a.id).await;
    let m = f.append(c.id, "old history").await;
    let path = f.dir.path().to_owned();
    f.store.close().await.unwrap();
    let sql = Connection::open(path.join("social.sqlite3")).unwrap();
    downgrade_contact_schema(&sql);
    sql.execute_batch("DROP TABLE participation_daily_budget; DROP TABLE participation_state; DROP TABLE participation_policies; DROP TABLE context_reference_sources; DROP TABLE context_references; DELETE FROM schema_migrations WHERE version>=9; PRAGMA user_version=8;").unwrap();
    drop(sql);
    let store =
        SqliteSocialStore::open(DataDirectoryGuard::acquire(&path).unwrap(), f.clock.clone())
            .await
            .unwrap();
    assert_eq!(store.message(f.human.id, m.id).await.unwrap(), m);
    let backups: Vec<_> = fs::read_dir(&path)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| {
            p.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("social.before-v9-")
        })
        .collect();
    assert!(backups.iter().any(|p| {
        Connection::open(p)
            .unwrap()
            .query_row::<i64, _, _>("SELECT count(*) FROM messages", [], |r| r.get(0))
            .unwrap()
            == 1
    }));
    store.close().await.unwrap();
}
