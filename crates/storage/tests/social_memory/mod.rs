use super::*;

fn input(
    ai: ActorId,
    conversation: ConversationId,
    source: Option<&SocialMessage>,
) -> CreateMemory {
    CreateMemory {
        scope: ContextScope {
            actor_id: ai,
            conversation_id: conversation,
        },
        content: MemoryContent {
            kind: MemoryKind::Preference,
            text: "喜欢短段落".into(),
            source: source.map(|m| MemoryMessageSource {
                message_id: m.id,
                revision: m.revision,
            }),
        },
    }
}

#[tokio::test]
async fn explicit_memory_permissions_receipts_and_transaction_rollback() {
    let f = Fixture::new().await;
    let a = f.ai("A").await;
    let b = f.ai("B").await;
    let c = f.direct(a.id).await;
    let message = f.append(c.id, "source").await;
    let input = input(a.id, c.id, Some(&message));
    let before = f.counts();
    f.inject();
    assert!(f.store.create_memory(f.ctx(), input.clone()).await.is_err());
    assert_eq!(f.counts(), before);
    f.clear();
    let ctx = f.ctx();
    let saved = f
        .store
        .create_memory(ctx.clone(), input.clone())
        .await
        .unwrap();
    assert_eq!(
        f.store
            .create_memory(ctx.clone(), input.clone())
            .await
            .unwrap(),
        saved
    );
    let mut changed = input.clone();
    changed.content.text = "changed".into();
    assert!(matches!(
        f.store.create_memory(ctx, changed).await,
        Err(SocialStorageError::Conflict(_))
    ));
    let outsider = CommandContext {
        operator: b.id,
        request_id: CommandId::new(),
    };
    assert!(matches!(
        f.store.memories(b.id, input.scope).await,
        Err(SocialStorageError::Forbidden(_))
    ));
    assert!(matches!(
        f.store
            .forget_memory(outsider, saved.id, saved.revision)
            .await,
        Err(SocialStorageError::Forbidden(_))
    ));
    let mut other = input.clone();
    other.scope.actor_id = b.id;
    assert!(matches!(
        f.store.create_memory(f.ctx(), other).await,
        Err(SocialStorageError::Forbidden(_))
    ));
    let group = f
        .store
        .create_group(f.ctx(), "group".into(), vec![a.id, b.id])
        .await
        .unwrap();
    let mut other = input.clone();
    other.scope.conversation_id = group.id;
    assert!(matches!(
        f.store.create_memory(f.ctx(), other).await,
        Err(SocialStorageError::Forbidden(_))
    ));
    assert!(
        f.store
            .memories(
                f.human.id,
                ContextScope {
                    actor_id: a.id,
                    conversation_id: group.id
                }
            )
            .await
            .unwrap()
            .is_empty()
    );
    let group_message = f.append(group.id, "group source").await;
    let group_memory = f
        .store
        .create_memory(f.ctx(), self::input(a.id, group.id, Some(&group_message)))
        .await
        .unwrap();
    let material = f
        .store
        .context_material(
            f.human.id,
            ContextScope {
                actor_id: b.id,
                conversation_id: group.id,
            },
            group_message.id,
        )
        .await
        .unwrap();
    assert!(material.memories.is_empty());
    let mut snapshot = f
        .snapshot(
            b.id,
            group.id,
            vec![SourceRef::Memory {
                memory_id: group_memory.id,
                revision: group_memory.revision,
            }],
        )
        .await;
    let current = f.store.conversation(f.human.id, group.id).await.unwrap();
    assert!(matches!(
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
        Err(SocialStorageError::Forbidden(_))
    ));
    snapshot.scope.actor_id = a.id;
    snapshot.dependencies[1].actor_id = a.id;
    snapshot.sources = vec![SourceRef::Memory {
        memory_id: saved.id,
        revision: saved.revision,
    }];
    assert!(matches!(
        f.store
            .begin_run(
                f.ctx(),
                BeginRun {
                    snapshot,
                    expected_revision: current.revision,
                    details: None
                }
            )
            .await,
        Err(SocialStorageError::Forbidden(_))
    ));
    f.close().await;
}

#[tokio::test]
async fn source_revision_requires_reconfirmation_and_cancels_late_commit() {
    let f = Fixture::new().await;
    let ai = f.ai("A").await;
    let c = f.direct(ai.id).await;
    let message = f.append(c.id, "original").await;
    let input = input(ai.id, c.id, Some(&message));
    let memory = f.store.create_memory(f.ctx(), input.clone()).await.unwrap();
    let run = f
        .begin(
            ai.id,
            c.id,
            vec![SourceRef::Memory {
                memory_id: memory.id,
                revision: memory.revision,
            }],
        )
        .await;
    let updated = f
        .store
        .revise_message(
            f.ctx(),
            ReviseMessage {
                message_id: message.id,
                body: MessageBody::text("corrected"),
                expected_revision: message.revision,
            },
        )
        .await
        .unwrap();
    let stale = f.store.memory(f.human.id, memory.id).await.unwrap();
    assert_eq!(stale.status, MaterialStatus::Stale);
    assert_eq!(
        f.store.run(f.human.id, run.id).await.unwrap().status,
        RunStatus::Interrupted
    );
    assert!(
        f.store
            .context_material(f.human.id, input.scope, message.id)
            .await
            .unwrap()
            .memories
            .is_empty()
    );
    assert!(
        f.store
            .update_memory(
                f.ctx(),
                UpdateMemory {
                    id: memory.id,
                    expected_revision: stale.revision,
                    content: input.content.clone()
                }
            )
            .await
            .is_err()
    );
    let mut content = input.content;
    content.source = Some(MemoryMessageSource {
        message_id: message.id,
        revision: updated.revision,
    });
    let confirmed = f
        .store
        .update_memory(
            f.ctx(),
            UpdateMemory {
                id: memory.id,
                expected_revision: stale.revision,
                content,
            },
        )
        .await
        .unwrap();
    assert_eq!(confirmed.status, MaterialStatus::Confirmed);
    let late = f
        .store
        .finish_run(
            f.ctx(),
            FinishRun {
                run_id: run.id,
                expected_revision: run.revision,
                status: RunStatus::Succeeded,
                body: MessageBody::text("obsolete answer"),
                details: None,
            },
        )
        .await;
    assert!(late.is_err());
    assert_eq!(
        f.store
            .message(f.human.id, run.message_id)
            .await
            .unwrap()
            .status,
        MessageStatus::Interrupted
    );
    f.store
        .withdraw_message(
            f.ctx(),
            WithdrawMessage {
                message_id: message.id,
                expected_revision: updated.revision,
            },
        )
        .await
        .unwrap();
    assert_eq!(
        f.store.memory(f.human.id, memory.id).await.unwrap().status,
        MaterialStatus::Stale
    );
    f.close().await;
}

#[tokio::test]
async fn forgetting_is_persistent_redacts_revisions_and_preserves_original_history() {
    let f = Fixture::new().await;
    let a = f.ai("A").await;
    let c = f.direct(a.id).await;
    let message = f.append(c.id, "original remains").await;
    let input = input(a.id, c.id, Some(&message));
    let memory = f.store.create_memory(f.ctx(), input.clone()).await.unwrap();
    let run = f
        .begin(
            a.id,
            c.id,
            vec![SourceRef::Memory {
                memory_id: memory.id,
                revision: memory.revision,
            }],
        )
        .await;
    let ctx = f.ctx();
    let counts = f.counts();
    f.inject();
    assert!(
        f.store
            .forget_memory(ctx.clone(), memory.id, memory.revision)
            .await
            .is_err()
    );
    assert_eq!(f.counts(), counts);
    assert_eq!(f.store.memory(f.human.id, memory.id).await.unwrap(), memory);
    assert_eq!(
        f.store.run(f.human.id, run.id).await.unwrap().status,
        RunStatus::Running
    );
    f.clear();
    let forgotten = f
        .store
        .forget_memory(ctx.clone(), memory.id, memory.revision)
        .await
        .unwrap();
    assert_eq!(forgotten.status, MaterialStatus::Forgotten);
    assert!(forgotten.text.is_empty());
    assert_eq!(
        f.store
            .forget_memory(ctx, memory.id, memory.revision)
            .await
            .unwrap(),
        forgotten
    );
    assert_eq!(
        f.store.run(f.human.id, run.id).await.unwrap().status,
        RunStatus::Interrupted
    );
    assert!(
        f.store
            .update_memory(
                f.ctx(),
                UpdateMemory {
                    id: memory.id,
                    expected_revision: forgotten.revision,
                    content: input.content.clone()
                }
            )
            .await
            .is_err()
    );
    let leak: i64 = f
        .sql()
        .query_row(
            "SELECT count(*) FROM memory_revisions WHERE json_extract(payload,'$.text')!=''",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(leak, 0);
    assert!(
        forgotten.sources.iter().any(
            |s| matches!(s, SourceRef::Message { message_id, .. } if *message_id == message.id)
        )
    );
    f.store.close().await.unwrap();
    let reopened =
        SqliteSocialStore::open(DataDirectoryGuard::acquire(f.dir.path()).unwrap(), f.clock)
            .await
            .unwrap();
    assert!(
        reopened
            .memories(f.human.id, input.scope)
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        reopened.memory(f.human.id, memory.id).await.unwrap(),
        forgotten
    );
    assert_eq!(
        reopened.message(f.human.id, message.id).await.unwrap(),
        message
    );
    reopened.close().await.unwrap();
}

#[tokio::test]
async fn v6_upgrade_keeps_accounts_history_and_creates_backup() {
    let f = Fixture::new().await;
    let a = f.ai("A").await;
    let c = f.direct(a.id).await;
    let message = f.append(c.id, "v6 history").await;
    let path = f.store.path().to_owned();
    f.store.close().await.unwrap();
    let sql = Connection::open(&path).unwrap();
    sql.execute_batch("DROP TABLE memory_message_sources; DROP TABLE memory_revisions; DROP TABLE memories; DELETE FROM schema_migrations WHERE version=7; PRAGMA user_version=6;").unwrap();
    drop(sql);
    let store =
        SqliteSocialStore::open(DataDirectoryGuard::acquire(f.dir.path()).unwrap(), f.clock)
            .await
            .unwrap();
    assert_eq!(
        store.message(f.human.id, message.id).await.unwrap(),
        message
    );
    assert_eq!(store.actor(f.human.id, a.id).await.unwrap(), a);
    let current = Connection::open(&path).unwrap();
    assert_eq!(
        current
            .pragma_query_value::<i64, _>(None, "user_version", |r| r.get(0))
            .unwrap(),
        7
    );
    let preserved = fs::read_dir(f.dir.path())
        .unwrap()
        .filter_map(|entry| {
            let path = entry.unwrap().path();
            if !path
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("social.before-v7-")
            {
                return None;
            }
            let backup = Connection::open(path).unwrap();
            let payload: Option<String> = backup
                .query_row(
                    "SELECT payload FROM messages WHERE id=?1",
                    [message.id.to_string()],
                    |r| r.get(0),
                )
                .ok();
            payload.map(|payload| serde_json::from_str::<SocialMessage>(&payload).unwrap())
        })
        .any(|m| m == message);
    assert!(
        preserved,
        "the v6-to-v7 backup must contain the original history"
    );
    store.close().await.unwrap();
}
