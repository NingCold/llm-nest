use super::*;

#[tokio::test]
async fn full_history_export_is_ordered_isolated_and_read_only() {
    let f = Fixture::new().await;
    let a = f.ai("中文联系人 🐈").await;
    let c = f.direct(a.id).await;
    let other = f.group(vec![a.id]).await;
    f.append(other.id, "other-window-secret").await;
    for n in 0..225 {
        f.append(c.id, &format!("message {n}")).await;
    }
    assert_eq!(
        f.store
            .messages(f.human.id, c.id, Sequence::ZERO, 1000)
            .await
            .unwrap()
            .len(),
        200
    );
    let before = f.store.overview(f.human.id).await.unwrap();
    let counts = f.counts();
    let snapshot = f
        .store
        .conversation_transcript(f.human.id, c.id)
        .await
        .unwrap();
    assert_eq!(snapshot.title, a.display_name);
    assert_eq!(snapshot.captured_at, f.clock.now());
    assert_eq!(snapshot.messages.len(), 225);
    for (n, m) in snapshot.messages.iter().enumerate() {
        assert_eq!(m.sequence, Sequence::new(n as u64 + 1).unwrap());
        assert!(
            matches!(&m.parts[0], TranscriptPart::Text { text } if text == &format!("message {n}"))
        );
    }
    let json = serde_json::to_string(&snapshot).unwrap();
    for excluded in [
        "other-window-secret",
        "角色设定",
        "same-model",
        "execution_binding",
    ] {
        assert!(!json.contains(excluded));
    }
    let after = f.store.overview(f.human.id).await.unwrap();
    assert_eq!(before.cursor, after.cursor);
    assert_eq!(
        serde_json::to_value(
            before
                .conversations
                .iter()
                .map(|c| &c.list_state)
                .collect::<Vec<_>>()
        )
        .unwrap(),
        serde_json::to_value(
            after
                .conversations
                .iter()
                .map(|c| &c.list_state)
                .collect::<Vec<_>>()
        )
        .unwrap()
    );
    assert_eq!(counts, f.counts());
    f.store.close().await.unwrap();
}

#[tokio::test]
async fn archived_blocked_and_empty_conversations_remain_exportable() {
    let f = Fixture::new().await;
    let a = f.ai("A").await;
    let c = f.direct(a.id).await;
    assert!(
        f.store
            .conversation_transcript(f.human.id, c.id)
            .await
            .unwrap()
            .messages
            .is_empty()
    );
    f.append(c.id, "preserved history").await;
    f.store
        .set_contact_block(
            f.ctx(),
            SetContactBlock {
                account_id: a.id,
                blocked: true,
                expected_revision: Some(
                    f.store
                        .overview(f.human.id)
                        .await
                        .unwrap()
                        .address_book
                        .into_iter()
                        .find(|entry| entry.account_id == a.id)
                        .unwrap()
                        .revision,
                ),
            },
        )
        .await
        .unwrap();
    let c = f.store.conversation(f.human.id, c.id).await.unwrap();
    f.store
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
    assert_eq!(
        f.store
            .conversation_transcript(f.human.id, c.id)
            .await
            .unwrap()
            .messages
            .len(),
        1
    );
    f.store.close().await.unwrap();
}

#[tokio::test]
async fn export_requires_current_membership_even_for_account_administrators() {
    let f = Fixture::new().await;
    let a = f.ai("A").await;
    let b = f.ai("B").await;
    let direct = f.direct(a.id).await;
    f.store
        .set_administrator(
            f.ctx(),
            a.id,
            SetAdministrator {
                account_id: b.id,
                granted: true,
                expected_revision: a.revision,
            },
        )
        .await
        .unwrap();
    assert!(matches!(
        f.store.conversation_transcript(b.id, direct.id).await,
        Err(SocialStorageError::Forbidden(_))
    ));
    let c = f.group(vec![a.id]).await;
    let old = f.append(c.id, "before joining").await;
    for action in [MembershipAction::Add, MembershipAction::Remove] {
        let c = f.store.conversation(f.human.id, c.id).await.unwrap();
        f.store
            .change_membership(
                f.ctx(),
                ChangeMembership {
                    conversation_id: c.id,
                    actor_id: b.id,
                    expected_revision: c.revision,
                    action,
                },
            )
            .await
            .unwrap();
        match action {
            MembershipAction::Add => assert_eq!(
                f.store
                    .conversation_transcript(b.id, c.id)
                    .await
                    .unwrap()
                    .messages[0]
                    .id,
                old.id
            ),
            MembershipAction::Remove => assert!(matches!(
                f.store.conversation_transcript(b.id, c.id).await,
                Err(SocialStorageError::Forbidden(_))
            )),
        }
    }
    f.store.close().await.unwrap();
}

#[tokio::test]
async fn snapshots_keep_current_revisions_statuses_and_attachment_placeholders() {
    let f = Fixture::new().await;
    let a = f.ai("A").await;
    let c = f.direct(a.id).await;
    let m = f
        .store
        .append_message(
            f.ctx(),
            AppendMessage {
                conversation_id: c.id,
                body: MessageBody(vec![
                    SocialContent::Text {
                        text: "old-secret".into(),
                    },
                    SocialContent::Image {
                        mime: Some("image/png".into()),
                        data: b"image-secret".to_vec(),
                    },
                    SocialContent::File {
                        mime: "application/pdf".into(),
                        data: b"file-secret".to_vec(),
                    },
                ]),
                reply_to: None,
                expected_revision: None,
            },
        )
        .await
        .unwrap();
    let withdrawn = f.append(c.id, "withdrawn-secret").await;
    let revised = f
        .store
        .revise_message(
            f.ctx(),
            ReviseMessage {
                message_id: m.id,
                text: "现在的正文\n🙂".into(),
                expected_revision: m.revision,
            },
        )
        .await
        .unwrap();
    f.store
        .withdraw_message(
            f.ctx(),
            WithdrawMessage {
                message_id: withdrawn.id,
                expected_revision: withdrawn.revision,
            },
        )
        .await
        .unwrap();
    let run = f.begin(a.id, c.id, vec![]).await;
    f.store
        .checkpoint_run(f.human.id, run.id, MessageBody::text("saved draft"), None)
        .await
        .unwrap();
    let snapshot = f
        .store
        .conversation_transcript(f.human.id, c.id)
        .await
        .unwrap();
    assert_eq!(snapshot.messages[0].revision, revised.revision);
    assert!(matches!(
        snapshot.messages[0].parts[1],
        TranscriptPart::Image {
            byte_length: 12,
            ..
        }
    ));
    assert!(matches!(
        snapshot.messages[0].parts[2],
        TranscriptPart::File {
            byte_length: 11,
            ..
        }
    ));
    assert_eq!(snapshot.messages[1].status, MessageStatus::Withdrawn);
    assert!(snapshot.messages[1].parts.is_empty());
    assert_eq!(snapshot.messages[2].status, MessageStatus::Generating);
    let json = serde_json::to_string(&snapshot).unwrap();
    for excluded in [
        "old-secret",
        "withdrawn-secret",
        "image-secret",
        "file-secret",
        "\"data\"",
    ] {
        assert!(!json.contains(excluded));
    }
    f.store
        .finish_run(
            f.ctx(),
            FinishRun {
                run_id: run.id,
                expected_revision: run.revision,
                status: RunStatus::Interrupted,
                body: MessageBody::text("saved draft plus more"),
                details: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(
        snapshot.messages[2].status,
        MessageStatus::Generating,
        "an earlier snapshot does not change"
    );
    assert_eq!(
        f.store
            .conversation_transcript(f.human.id, c.id)
            .await
            .unwrap()
            .messages[2]
            .status,
        MessageStatus::Interrupted
    );
    f.store.close().await.unwrap();
}

#[tokio::test]
async fn export_keeps_reference_ids_without_expanding_other_windows() {
    let f = Fixture::new().await;
    let a = f.ai("A").await;
    let group = f.group(vec![a.id]).await;
    let source = f.append(group.id, "group source secret").await;
    let c = f.direct(a.id).await;
    let sent = f
        .store
        .send_message(
            f.ctx(),
            SendMessage {
                conversation_id: c.id,
                body: MessageBody::text("continue here"),
                mentions: vec![],
                reply_to: None,
                reference: Some(ReferenceSelection {
                    conversation_id: group.id,
                    sources: vec![SourceRef::Message {
                        conversation_id: group.id,
                        message_id: source.id,
                        revision: source.revision,
                    }],
                }),
            },
        )
        .await
        .unwrap();
    let exported = f
        .store
        .conversation_transcript(f.human.id, c.id)
        .await
        .unwrap();
    assert_eq!(
        exported.messages[0].context_reference,
        sent.message.context_reference
    );
    let json = serde_json::to_string(&exported).unwrap();
    assert!(json.contains(&source.id.to_string()));
    assert!(!json.contains("group source secret"));
    f.store.close().await.unwrap();
}
