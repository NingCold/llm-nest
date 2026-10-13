use super::*;

async fn state(f: &Fixture, c: ConversationId) -> ConversationListState {
    f.store
        .conversation_record(f.human.id, c)
        .await
        .unwrap()
        .list_state
}
async fn change(
    f: &Fixture,
    c: ConversationId,
    action: ConversationListAction,
) -> ConversationListState {
    f.store
        .update_conversation_list(
            f.ctx(),
            UpdateConversationList {
                conversation_id: c,
                expected_revision: state(f, c).await.revision,
                action,
            },
        )
        .await
        .unwrap()
}
async fn finish(f: &Fixture, run: AiRun, status: RunStatus) {
    f.store
        .finish_run(
            f.ctx(),
            FinishRun {
                run_id: run.id,
                expected_revision: run.revision,
                status,
                body: MessageBody::text("迟到的答复"),
                details: None,
            },
        )
        .await
        .unwrap();
}
fn query(text: &str, c: Option<ConversationId>) -> SearchMessages {
    SearchMessages {
        query: text.into(),
        conversation_id: c,
        before: None,
    }
}

#[tokio::test]
async fn late_completion_is_unread_even_after_a_later_message_was_read() {
    let f = Fixture::new().await;
    let a = f.ai("A").await;
    let c = f.direct(a.id).await;
    f.append(c.id, "first").await;
    let run = f.begin(a.id, c.id, vec![]).await;
    let later = f.append(c.id, "later while generating").await;
    assert_eq!(state(&f, c.id).await.unread_count, 0);
    let (_, _, through) = f.store.message_page(f.human.id, c.id, None).await.unwrap();
    change(&f, c.id, ConversationListAction::Read { through }).await;
    finish(&f, run, RunStatus::Succeeded).await;
    let list = state(&f, c.id).await;
    assert_eq!(list.unread_count, 1);
    assert!(list.latest_delivery > through);
    let page = f.store.message_page(f.human.id, c.id, None).await.unwrap();
    assert_eq!(page.0.last().unwrap().id, later.id);
    change(&f, c.id, ConversationListAction::Read { through: page.2 }).await;
    assert_eq!(state(&f, c.id).await.unread_count, 0);
    let run = f.begin(a.id, c.id, vec![]).await;
    finish(&f, run, RunStatus::Cancelled).await;
    assert_eq!(
        state(&f, c.id).await.unread_count,
        1,
        "saved interrupted replies also arrive once"
    );
    f.close().await;
}

#[tokio::test]
async fn list_actions_are_personal_versioned_atomic_and_survive_restart() {
    let f = Fixture::new().await;
    let a = f.ai("A").await;
    let outsider = f.ai("outsider").await;
    let c = f.direct(a.id).await;
    let before = f.store.conversation(f.human.id, c.id).await.unwrap();
    let ctx = f.ctx();
    let input = UpdateConversationList {
        conversation_id: c.id,
        expected_revision: Revision::ONE,
        action: ConversationListAction::Pin { pinned: true },
    };
    f.inject();
    let counts = f.counts();
    assert!(
        f.store
            .update_conversation_list(ctx.clone(), input.clone())
            .await
            .is_err()
    );
    assert_eq!(f.counts(), counts);
    assert!(!state(&f, c.id).await.pinned);
    f.clear();
    let pinned = f
        .store
        .update_conversation_list(ctx.clone(), input.clone())
        .await
        .unwrap();
    assert!(pinned.pinned);
    assert_eq!(
        f.store
            .update_conversation_list(ctx, input.clone())
            .await
            .unwrap()
            .revision,
        pinned.revision
    );
    assert!(matches!(
        f.store
            .update_conversation_list(f.ctx(), input.clone())
            .await,
        Err(SocialStorageError::Conflict(_))
    ));
    assert!(matches!(
        f.store
            .update_conversation_list(
                CommandContext {
                    operator: outsider.id,
                    request_id: CommandId::new()
                },
                input
            )
            .await,
        Err(SocialStorageError::Forbidden(_))
    ));
    change(&f, c.id, ConversationListAction::Unread).await;
    assert!(state(&f, c.id).await.marked_unread);
    assert!(
        !f.store
            .conversation_record(a.id, c.id)
            .await
            .unwrap()
            .list_state
            .pinned
    );
    assert_eq!(
        f.store.conversation(f.human.id, c.id).await.unwrap(),
        before,
        "personal actions cannot alter context or participation"
    );
    let expected = state(&f, c.id).await.revision;
    assert!(matches!(
        f.store
            .update_conversation_list(
                f.ctx(),
                UpdateConversationList {
                    conversation_id: c.id,
                    expected_revision: expected,
                    action: ConversationListAction::Read {
                        through: Counter::new(999).unwrap()
                    }
                }
            )
            .await,
        Err(SocialStorageError::Invalid(_))
    ));
    let dir = f.dir.path().to_owned();
    f.store.close().await.unwrap();
    let reopened =
        SqliteSocialStore::open(DataDirectoryGuard::acquire(&dir).unwrap(), f.clock.clone())
            .await
            .unwrap();
    let list = reopened
        .conversation_record(f.human.id, c.id)
        .await
        .unwrap()
        .list_state;
    assert!(list.pinned && list.marked_unread);
    assert_eq!(list.revision, expected);
    reopened.close().await.unwrap();
}

#[tokio::test]
async fn search_uses_current_full_text_and_respects_membership_and_paging() {
    let f = Fixture::new().await;
    let a = f.ai("A").await;
    let b = f.ai("B").await;
    let c = f.direct(a.id).await;
    let other = f.direct(b.id).await;
    let first = f.append(c.id, "中文 needle %_ emoji 🪶").await;
    for i in 0..205 {
        f.append(c.id, &format!("中文 needle {i}")).await;
    }
    let secret = f.append(other.id, "中文 needle other window").await;
    let page = f
        .store
        .search_messages(a.id, query("NEEDLE", None))
        .await
        .unwrap();
    assert_eq!(page.hits.len(), 50);
    assert!(page.hits.iter().all(|h| h.message.conversation_id == c.id));
    let mut all = page.hits;
    let mut cursor = page.next_before;
    while let Some(before) = cursor {
        let mut input = query("needle", Some(c.id));
        input.before = Some(before);
        let next = f.store.search_messages(f.human.id, input).await.unwrap();
        cursor = next.next_before;
        all.extend(next.hits);
    }
    assert_eq!(all.len(), 206);
    assert_eq!(all.last().unwrap().message.id, first.id);
    assert_eq!(
        all.iter()
            .map(|h| h.message.id)
            .collect::<std::collections::HashSet<_>>()
            .len(),
        206
    );
    let context = f.store.message_context(f.human.id, first.id).await.unwrap();
    assert_eq!(context.messages[0].id, first.id);
    assert!(!context.has_more && context.has_newer);
    assert!(matches!(
        f.store.message_context(a.id, secret.id).await,
        Err(SocialStorageError::Forbidden(_))
    ));
    assert!(matches!(
        f.store
            .search_messages(a.id, query("needle", Some(other.id)))
            .await,
        Err(SocialStorageError::Forbidden(_))
    ));
    assert_eq!(
        f.store
            .search_messages(f.human.id, query("%_", None))
            .await
            .unwrap()
            .hits
            .len(),
        1,
        "wildcard characters are literal"
    );
    let edited = f
        .store
        .revise_message(
            f.ctx(),
            ReviseMessage {
                message_id: first.id,
                body: MessageBody::text("replacement"),
                expected_revision: first.revision,
            },
        )
        .await
        .unwrap();
    assert!(
        f.store
            .search_messages(f.human.id, query("%_", None))
            .await
            .unwrap()
            .hits
            .is_empty()
    );
    assert_eq!(
        f.store
            .search_messages(f.human.id, query("replacement", None))
            .await
            .unwrap()
            .hits[0]
            .message
            .id,
        first.id
    );
    f.store
        .withdraw_message(
            f.ctx(),
            WithdrawMessage {
                message_id: first.id,
                expected_revision: edited.revision,
            },
        )
        .await
        .unwrap();
    assert!(
        f.store
            .search_messages(f.human.id, query("replacement", None))
            .await
            .unwrap()
            .hits
            .is_empty()
    );
    assert_eq!(
        f.store
            .message_context(f.human.id, first.id)
            .await
            .unwrap()
            .messages[0]
            .status,
        MessageStatus::Withdrawn
    );
    for text in [" ".into(), "x".repeat(201)] {
        assert!(matches!(
            f.store
                .search_messages(f.human.id, query(&text, None))
                .await,
            Err(SocialStorageError::Invalid(_))
        ));
    }
    f.close().await;
}

#[tokio::test]
async fn migration_starts_existing_history_read_and_recovers_new_delivery_once() {
    let f = Fixture::new().await;
    let a = f.ai("A").await;
    let c = f.direct(a.id).await;
    let m = f.append(c.id, "preserved history").await;
    let run = f.begin(a.id, c.id, vec![]).await;
    let path = f.dir.path().to_owned();
    f.store.close().await.unwrap();
    let sql = Connection::open(path.join("social.sqlite3")).unwrap();
    downgrade_list_schema(&sql);
    drop(sql);
    let store =
        SqliteSocialStore::open(DataDirectoryGuard::acquire(&path).unwrap(), f.clock.clone())
            .await
            .unwrap();
    assert_eq!(store.message(f.human.id, m.id).await.unwrap(), m);
    assert_eq!(
        store
            .message(f.human.id, run.message_id)
            .await
            .unwrap()
            .status,
        MessageStatus::Interrupted
    );
    let list = store
        .conversation_record(f.human.id, c.id)
        .await
        .unwrap()
        .list_state;
    assert_eq!(list.unread_count, 1);
    assert!(list.read_through > Counter::ZERO);
    let sql = Connection::open(store.path()).unwrap();
    assert_eq!(
        sql.pragma_query_value::<i64, _>(None, "user_version", |r| r.get(0))
            .unwrap(),
        13
    );
    assert_eq!(
        sql.query_row::<i64, _, _>("SELECT COUNT(*) FROM message_deliveries", [], |r| r.get(0))
            .unwrap(),
        2
    );
    drop(sql);
    store.close().await.unwrap();
    let store =
        SqliteSocialStore::open(DataDirectoryGuard::acquire(&path).unwrap(), f.clock.clone())
            .await
            .unwrap();
    assert_eq!(
        store
            .conversation_record(f.human.id, c.id)
            .await
            .unwrap()
            .list_state
            .latest_delivery,
        list.latest_delivery
    );
    assert!(
        fs::read_dir(&path)
            .unwrap()
            .filter_map(|e| e.ok())
            .any(|e| e
                .file_name()
                .to_string_lossy()
                .starts_with("social.before-v12-"))
    );
    store.close().await.unwrap();
}
