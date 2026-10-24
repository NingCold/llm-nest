use super::*;
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
fn uid() -> String {
    crate::id()
}
fn config() -> Config {
    toml::from_str(
        r#"
server="http://127.0.0.1:1"
node_id="node"
node_name="test"
daily_calls=20
daily_tokens=200000
[profile]
revision="1"
persona="test"
[profile.selection]
provider="test"
model="small"
[providers.test]
protocol="openai"
base_url="http://127.0.0.1:1/v1"
api_key={env="UNUSED_TEST_KEY"}
[providers.test.models.small]
model="small"
context_window=8192
max_tokens=4096
"#,
    )
    .unwrap()
}
fn conversation() -> NodeConversation {
    NodeConversation {
        encryption: None,
        id: uid(),
        kind: "group".into(),
        name: "group".into(),
        revision: "1".into(),
        members: vec![],
    }
}
fn memory(id: &str, source: Option<SourceRevision>) -> MemoryItem {
    MemoryItem {
        id: id.into(),
        revision: "1".into(),
        kind: "fact".into(),
        text: "private material".into(),
        status: "confirmed".into(),
        source,
        created_at: now(),
        updated_at: now(),
    }
}
fn record(id: &str) -> SummaryRecord {
    SummaryRecord {
        item: SummaryItem {
            import_origin: None,
            id: id.into(),
            revision: "1".into(),
            status: "ready".into(),
            text: "private summary".into(),
            from_position: "1".into(),
            through_position: "3".into(),
            sources: vec![],
            error: None,
            created_at: now(),
            updated_at: now(),
        },
        binding: SummaryModel {
            provider_id: "test".into(),
            model_id: "small".into(),
            reasoning_effort: None,
            max_tokens: 512,
        },
        settings_revision: "1".into(),
        conversation_revision: "1".into(),
        epoch: uid(),
        requested_by: uid(),
        start_request: uid(),
        intent: false,
        confirmation: None,
        finished: false,
    }
}
fn message(pos: usize) -> NodeMessage {
    NodeMessage {
        sealed: None,
        history_migration: None,
        audience_revision: None,
        context_reference: None,
        import_origin: None,
        id: uid(),
        position: pos.to_string(),
        sender_id: uid(),
        sender_name: "Human".into(),
        reply_to: None,
        mentions: vec![],
        text: format!("message {pos}"),
        revision: "1".into(),
        created_at: now(),
        attachments: vec![],
    }
}
fn event(cursor: &str, cid: &str, mid: Option<&str>, reason: &str) -> ConsciousnessChanges {
    ConsciousnessChanges {
        cursor: cursor.into(),
        has_more: false,
        changes: vec![ConsciousnessChange {
            id: cursor.into(),
            conversation_id: cid.into(),
            message_id: mid.map(Into::into),
            reason: reason.into(),
        }],
    }
}
async fn mock(
    c: NodeConversation,
    messages: Vec<NodeMessage>,
    delay: bool,
) -> (
    NodeClient,
    tokio::task::JoinHandle<()>,
    Arc<Mutex<Vec<String>>>,
) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let paths = Arc::new(Mutex::new(Vec::new()));
    let logged = paths.clone();
    let server = tokio::spawn(async move {
        loop {
            let (mut socket, _) = listener.accept().await.unwrap();
            let c = c.clone();
            let messages = messages.clone();
            let logged = logged.clone();
            tokio::spawn(async move {
                let mut input = Vec::new();
                let mut buf = [0; 4096];
                loop {
                    let n = socket.read(&mut buf).await.unwrap();
                    if n == 0 {
                        return;
                    }
                    input.extend_from_slice(&buf[..n]);
                    if let Some(end) = input.windows(4).position(|b| b == b"\r\n\r\n") {
                        let header = String::from_utf8_lossy(&input[..end]);
                        let length = header
                            .lines()
                            .find_map(|line| {
                                line.to_ascii_lowercase()
                                    .strip_prefix("content-length:")
                                    .map(|n| n.trim().parse::<usize>().unwrap())
                            })
                            .unwrap_or(0);
                        if input.len() >= end + 4 + length {
                            break;
                        }
                    }
                }
                let request = String::from_utf8_lossy(&input);
                let path = request
                    .lines()
                    .next()
                    .unwrap()
                    .split_whitespace()
                    .nth(1)
                    .unwrap()
                    .to_string();
                logged.lock().unwrap().push(path.clone());
                let body = if path.contains("/changes?") {
                    if delay {
                        tokio::time::sleep(std::time::Duration::from_millis(80)).await;
                    }
                    serde_json::json!({"cursor":"0","hasMore":false,"changes":[]})
                } else if path.contains("/validate") {
                    serde_json::json!({"valid":true})
                } else if path.contains("/history?") {
                    let after = path
                        .split("after=")
                        .nth(1)
                        .unwrap()
                        .split('&')
                        .next()
                        .unwrap()
                        .parse::<usize>()
                        .unwrap();
                    let boundary = messages
                        .get(messages.len().saturating_sub(12))
                        .map(|m| m.position.clone());
                    serde_json::json!({"conversation":c,"messages":messages.iter().filter(|m|m.position.parse::<usize>().unwrap()>after).take(100).cloned().collect::<Vec<_>>(),"recentBoundary":boundary})
                } else if path.ends_with("/summaries/start") {
                    serde_json::json!({"id":"confirmation-row","confirmationId":"confirmation"})
                } else {
                    serde_json::json!({"ok":true})
                };
                let body = body.to_string();
                socket.write_all(format!("HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",body.len(),body).as_bytes()).await.unwrap();
            });
        }
    });
    (
        NodeClient::new(&format!("http://{address}"), "test", None).unwrap(),
        server,
        paths,
    )
}
#[test]
fn ai_window_isolation_and_recent_material_order() {
    let store = Store::open(std::path::Path::new(":memory:")).unwrap();
    let ai = uid();
    let other = uid();
    let cid = uid();
    let second = uid();
    let mut old = memory(&uid(), None);
    old.updated_at = "2026-01-01T00:00:00Z".into();
    let mut new = memory(&uid(), None);
    new.updated_at = "2026-02-01T00:00:00Z".into();
    store.save_memory(&ai, &cid, &old).unwrap();
    store.save_memory(&ai, &cid, &new).unwrap();
    assert_eq!(store.memories(&ai, &cid).unwrap()[0].id, new.id);
    assert!(store.memories(&other, &cid).unwrap().is_empty());
    assert!(store.memories(&ai, &second).unwrap().is_empty());
    assert!(!store.settings(&ai, &cid).unwrap().enabled);
    assert_eq!(store.settings(&ai, &cid).unwrap().binding, None);
}
#[test]
fn membership_changes_preserve_ready_but_revocation_erases_and_fences() {
    let store = Store::open(std::path::Path::new(":memory:")).unwrap();
    let ai = uid();
    let cid = uid();
    let other = uid();
    store.save_memory(&ai, &cid, &memory(&uid(), None)).unwrap();
    let ready = record(&uid());
    let mut queued = record(&uid());
    queued.item.status = "queued".into();
    store.save_summary(&ai, &cid, &ready).unwrap();
    store.save_summary(&ai, &cid, &queued).unwrap();
    store
        .db
        .execute(
            "INSERT INTO conscious_settings VALUES(?1,?2,?3)",
            params![
                ai,
                cid,
                serde_json::to_string(&SummarySettings {
                    revision: "1".into(),
                    enabled: true,
                    binding: Some(ready.binding.clone())
                })
                .unwrap()
            ],
        )
        .unwrap();
    let old = store.material_scope(&ai, &cid).unwrap();
    let unrelated = store.material_scope(&ai, &other).unwrap();
    store
        .invalidate(&ai, &event("1", &cid, None, "window_changed"))
        .unwrap();
    assert_eq!(store.memories(&ai, &cid).unwrap()[0].status, "confirmed");
    assert_eq!(
        store
            .summaries(&ai, &cid)
            .unwrap()
            .iter()
            .find(|s| s.item.id == ready.item.id)
            .unwrap()
            .item
            .text,
        "private summary"
    );
    assert!(store.settings(&ai, &cid).unwrap().enabled);
    assert!(!store.scopes_current(&ai, &[old]).unwrap());
    assert!(store.scopes_current(&ai, &[unrelated]).unwrap());
    store
        .invalidate(&ai, &event("2", &cid, None, "permission_revoked"))
        .unwrap();
    assert!(!store.settings(&ai, &cid).unwrap().enabled);
    assert!(store.settings(&ai, &cid).unwrap().binding.is_none());
    assert!(store.memories(&ai, &cid).unwrap()[0].text.is_empty());
    assert!(
        store
            .summaries(&ai, &cid)
            .unwrap()
            .iter()
            .all(|s| s.item.text.is_empty() && s.item.status == "stale")
    );
    store
        .invalidate(&ai, &event("3", &cid, None, "window_changed"))
        .unwrap();
    assert!(!store.settings(&ai, &cid).unwrap().enabled);
    assert_eq!(store.memories(&ai, &cid).unwrap()[0].status, "stale");
}
#[test]
fn source_cursor_and_scope_survive_reconnect_without_global_fence() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("node.db");
    let ai = uid();
    let cid = uid();
    let source = uid();
    let untouched = uid();
    {
        let store = Store::open(&path).unwrap();
        store
            .save_memory(
                &ai,
                &cid,
                &memory(
                    &uid(),
                    Some(SourceRevision {
                        message_id: source.clone(),
                        revision: "1".into(),
                    }),
                ),
            )
            .unwrap();
        store
            .save_memory(&ai, &cid, &memory(&untouched, None))
            .unwrap();
        store
            .invalidate(&ai, &event("72", &cid, Some(&source), "message_changed"))
            .unwrap();
    }
    let store = Store::open(&path).unwrap();
    let cursor: String = store
        .db
        .query_row(
            "SELECT cursor FROM conscious_cursors WHERE ai=?1",
            [&ai],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(cursor, "72");
    assert_eq!(
        store
            .memories(&ai, &cid)
            .unwrap()
            .iter()
            .find(|m| m.id == untouched)
            .unwrap()
            .status,
        "confirmed"
    );
    assert!(
        store
            .memories(&ai, &cid)
            .unwrap()
            .iter()
            .any(|m| m.text.is_empty() && m.status == "stale")
    );
}
#[tokio::test]
async fn mutations_are_idempotent_receipts_have_no_body_and_forget_removes_text() {
    let store = Store::open(std::path::Path::new(":memory:")).unwrap();
    let ai = uid();
    let c = conversation();
    let operator = uid();
    let (api, server, _) = mock(c.clone(), vec![], false).await;
    let cfg = config();
    let request = uid();
    let create = ConsciousnessOperation::CreateMemory {
        kind: "fact".into(),
        text: "secret initial memory".into(),
        source: None,
    };
    let first = manage(
        &api,
        &store,
        &cfg,
        &ai,
        &request,
        &operator,
        &c,
        create.clone(),
    )
    .await
    .unwrap();
    manage(&api, &store, &cfg, &ai, &request, &operator, &c, create)
        .await
        .unwrap();
    assert_eq!(store.memories(&ai, &c.id).unwrap().len(), 1);
    let old = store.material_scope(&ai, &c.id).unwrap();
    manage(
        &api,
        &store,
        &cfg,
        &ai,
        &uid(),
        &operator,
        &c,
        ConsciousnessOperation::EditMemory {
            id: request.clone(),
            revision: first.memories[0].revision.clone(),
            kind: "preference".into(),
            text: "revised memory".into(),
            source: None,
        },
    )
    .await
    .unwrap();
    assert!(!store.scopes_current(&ai, &[old]).unwrap());
    let forget = uid();
    let operation = ConsciousnessOperation::ForgetMemory {
        id: request,
        revision: "2".into(),
    };
    manage(
        &api,
        &store,
        &cfg,
        &ai,
        &forget,
        &operator,
        &c,
        operation.clone(),
    )
    .await
    .unwrap();
    manage(&api, &store, &cfg, &ai, &forget, &operator, &c, operation)
        .await
        .unwrap();
    assert_eq!(store.memories(&ai, &c.id).unwrap()[0].text, "");
    let count: i64 = store
        .db
        .query_row("SELECT count(*) FROM conscious_receipts", [], |r| r.get(0))
        .unwrap();
    manage(
        &api,
        &store,
        &cfg,
        &ai,
        &uid(),
        &operator,
        &c,
        ConsciousnessOperation::Memories,
    )
    .await
    .unwrap();
    assert_eq!(
        store
            .db
            .query_row::<i64, _, _>("SELECT count(*) FROM conscious_receipts", [], |r| r.get(0))
            .unwrap(),
        count
    );
    let rows: Vec<String> = store
        .db
        .prepare("SELECT fingerprint FROM conscious_receipts")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    assert!(rows.iter().all(|r| r.len() == 64 && !r.contains("memory")));
    server.abort();
}
#[tokio::test]
async fn segmentation_paginates_preserves_latest_twelve_caps_thirtytwo_and_owns_ai() {
    let store = Store::open(std::path::Path::new(":memory:")).unwrap();
    let ai = uid();
    let c = conversation();
    let operator = uid();
    let (api, server, paths) = mock(c.clone(), (1..=120).map(message).collect(), false).await;
    let cfg = config();
    let settings = SummarySettings {
        revision: "1".into(),
        enabled: true,
        binding: Some(record(&uid()).binding),
    };
    store
        .db
        .execute(
            "INSERT INTO conscious_settings VALUES(?1,?2,?3)",
            params![ai, c.id, serde_json::to_string(&settings).unwrap()],
        )
        .unwrap();
    let seg = make_segment(&api, &store, &cfg, &ai, &c, &operator, None, false)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(seg.item.sources.len(), 32);
    assert_eq!(seg.item.through_position, "32");
    assert!(
        paths
            .lock()
            .unwrap()
            .iter()
            .any(|p| p.contains("after=100"))
    );
    store.save_summary(&ai, &c.id, &seg).unwrap();
    assert!(store.summaries(&uid(), &c.id).unwrap().is_empty());
    server.abort();
    let (api, server, _) = mock(c.clone(), (1..=20).map(message).collect(), false).await;
    assert!(
        make_segment(&api, &store, &cfg, &uid(), &c, &operator, None, false)
            .await
            .is_err()
    );
    let another = uid();
    store
        .db
        .execute(
            "INSERT INTO conscious_settings VALUES(?1,?2,?3)",
            params![another, c.id, serde_json::to_string(&settings).unwrap()],
        )
        .unwrap();
    assert!(
        make_segment(&api, &store, &cfg, &another, &c, &operator, None, false)
            .await
            .unwrap()
            .is_none()
    );
    let manual = make_segment(&api, &store, &cfg, &another, &c, &operator, None, true)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(manual.item.through_position, "8");
    server.abort();
}
#[tokio::test]
async fn restart_interrupts_queued_and_reconciles_confirmed_without_calling_provider() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("node.db");
    let ai = uid();
    let c = conversation();
    let id = uid();
    {
        let store = Store::open(&path).unwrap();
        let mut row = record(&id);
        row.item.status = "running".into();
        row.intent = true;
        row.confirmation = Some(SummaryConfirmation {
            id: "cloud".into(),
            confirmation_id: "confirmation".into(),
        });
        store.save_summary(&ai, &c.id, &row).unwrap();
        let mut queued = record(&uid());
        queued.item.status = "queued".into();
        store.save_summary(&ai, &c.id, &queued).unwrap();
    }
    let store = Store::open(&path).unwrap();
    assert!(
        store
            .summaries(&ai, &c.id)
            .unwrap()
            .iter()
            .all(|s| s.item.status == "interrupted")
    );
    let (api, server, paths) = mock(c.clone(), vec![], false).await;
    recover_summaries(&api, &store, &ai).await.unwrap();
    assert!(
        store
            .summaries(&ai, &c.id)
            .unwrap()
            .iter()
            .find(|s| s.item.id == id)
            .unwrap()
            .finished
    );
    assert_eq!(
        &*paths.lock().unwrap(),
        &["/api/node/summaries/cloud/finish"]
    );
    server.abort();
}

#[tokio::test]
async fn selected_management_frame_finishes_while_provider_keeps_producing_chunks() {
    selected_management_frame_finishes(false).await;
}
#[tokio::test]
async fn selected_management_frame_finishes_even_when_provider_deadline_passes() {
    selected_management_frame_finishes(true).await;
}
async fn selected_management_frame_finishes(provider_timeout: bool) {
    use futures_util::{SinkExt, StreamExt};
    let store = Store::open(std::path::Path::new(":memory:")).unwrap();
    let ai = uid();
    let c = conversation();
    let cfg = config();
    let request = uid();
    let (api, http_server, _) = mock(c.clone(), vec![], true).await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let expected = request.clone();
    let frame = ServerFrame::ConsciousnessRequest {
        relay_id: uid(),
        request_id: request,
        operator_id: uid(),
        conversation: c.clone(),
        operation: ConsciousnessOperation::CreateMemory {
            kind: "fact".into(),
            text: "saved despite provider progress".into(),
            source: None,
        },
    };
    let ws_server = tokio::spawn(async move {
        let (tcp, _) = listener.accept().await.unwrap();
        let mut ws = tokio_tungstenite::accept_async(tcp).await.unwrap();
        ws.send(tokio_tungstenite::tungstenite::Message::Text(
            serde_json::to_string(&frame).unwrap().into(),
        ))
        .await
        .unwrap();
        let reply = tokio::time::timeout(std::time::Duration::from_secs(3), ws.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        let reply: ClientFrame = serde_json::from_str(reply.to_text().unwrap()).unwrap();
        match reply {
            ClientFrame::ConsciousnessResponse {
                request_id,
                view,
                error,
                ..
            } => {
                assert_eq!(request_id, expected);
                assert!(view.is_some());
                assert!(error.is_none());
            }
            _ => panic!("unexpected frame"),
        }
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    });
    let ws_api = NodeClient::new(&format!("http://{address}"), "test", None).unwrap();
    let mut socket = ws_api.connect().await.unwrap();
    let binding = Binding {
        ai: ai.clone(),
        version: "1".into(),
        secret: "test".into(),
    };
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_millis(20);
    let work = async {
        if provider_timeout {
            tokio::time::sleep_until(deadline).await;
            bail!("generation_timeout");
        } else {
            for _ in 0..20 {
                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            }
        }
        Ok(42)
    };
    let output =
        crate::while_connected_with(&mut socket, &api, &store, &cfg, &binding, &[], "", work).await;
    if provider_timeout {
        assert_eq!(output.unwrap_err().to_string(), "generation_timeout");
    } else {
        assert_eq!(output.unwrap(), 42);
    }
    assert_eq!(store.memories(&ai, &c.id).unwrap().len(), 1);
    ws_server.await.unwrap();
    http_server.abort();
}
#[tokio::test]
async fn enrolled_ai_rejects_forged_plain_management_frames_even_when_not_ready() {
    use futures_util::{SinkExt, StreamExt};
    let store = Store::open(std::path::Path::new(":memory:")).unwrap();
    let ai = uid();
    let operator = uid();
    let c = conversation();
    let mut cfg = config();
    cfg.profile.persona = "NEVER_EXPOSE_PERSONA".into();
    let binding = Binding {
        ai: ai.clone(),
        version: "1".into(),
        secret: "test".into(),
    };
    store.bind(&binding, &cfg.profile).unwrap();
    assert!(!crate::crypto::enrolled(&store, &ai).unwrap());
    store.db.execute("INSERT INTO crypto_devices(ai,device,local_key,ready) VALUES(?1,'pending-device',X'01',0)",[&ai]).unwrap();
    let mut item = memory(&uid(), None);
    item.text = "NEVER_EXPOSE_MEMORY".into();
    store.save_memory(&ai, &c.id, &item).unwrap();
    let cid = c.id.clone();
    let (api, http_server, paths) = mock(c.clone(), vec![], false).await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let frames = [
        ServerFrame::ManagementRequest {
            relay_id: uid(),
            request_id: uid(),
            operator_id: operator.clone(),
            operation: AgentManagementOperation::View,
        },
        ServerFrame::ConsciousnessRequest {
            relay_id: uid(),
            request_id: uid(),
            operator_id: operator,
            conversation: c,
            operation: ConsciousnessOperation::Memories,
        },
    ];
    let server = tokio::spawn(async move {
        let (tcp, _) = listener.accept().await.unwrap();
        let mut ws = tokio_tungstenite::accept_async(tcp).await.unwrap();
        for frame in frames {
            ws.send(tokio_tungstenite::tungstenite::Message::Text(
                serde_json::to_string(&frame).unwrap().into(),
            ))
            .await
            .unwrap();
            let response = ws.next().await.unwrap().unwrap();
            let text = response.to_text().unwrap();
            let body: serde_json::Value = serde_json::from_str(text).unwrap();
            assert!(body["view"].is_null());
            assert_eq!(body["error"]["code"], "encrypted_management_required");
            assert_eq!(body["error"]["message"], "");
            assert!(!text.contains("NEVER_EXPOSE"));
        }
    });
    let client = NodeClient::new(&origin, "test", None).unwrap();
    let mut socket = client.connect().await.unwrap();
    for _ in 0..2 {
        let frame = socket.recv().await.unwrap();
        handle_frame(frame, &mut socket, &api, &store, &cfg, &binding, &[])
            .await
            .unwrap();
    }
    server.await.unwrap();
    assert!(paths.lock().unwrap().is_empty());
    assert_eq!(store.profile(&ai).unwrap().persona, "NEVER_EXPOSE_PERSONA");
    assert_eq!(
        store.memories(&ai, &cid).unwrap()[0].text,
        "NEVER_EXPOSE_MEMORY"
    );
    http_server.abort();
}

#[tokio::test]
async fn restart_preserves_completed_summary_pending_ciphertext_upload() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("node.db");
    let ai = uid();
    let cid = uid();
    let id = uid();
    {
        let store = Store::open(&path).unwrap();
        let mut row = record(&id);
        row.item.status = "pending_sync".into();
        row.item.text = "completed model output".into();
        row.intent = true;
        row.confirmation = Some(SummaryConfirmation {
            id: "cloud".into(),
            confirmation_id: "confirmation".into(),
        });
        store.save_summary(&ai, &cid, &row).unwrap();
    }
    let store = Store::open(&path).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let (mut tcp, _) = listener.accept().await.unwrap();
        let mut input = Vec::new();
        loop {
            let mut bytes = [0; 4096];
            let count = tcp.read(&mut bytes).await.unwrap();
            assert!(count > 0);
            input.extend_from_slice(&bytes[..count]);
            if let Some(end) = input.windows(4).position(|w| w == b"\r\n\r\n")
                && let Ok(body) = serde_json::from_slice::<serde_json::Value>(&input[end + 4..])
            {
                assert_eq!(body["status"], "ready");
                assert!(body["errorCode"].is_null());
                break;
            }
        }
        tcp.write_all(b"HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: 11\r\nconnection: close\r\n\r\n{\"ok\":true}").await.unwrap();
    });
    let api = NodeClient::new(&origin, "test", None).unwrap();
    recover_summaries(&api, &store, &ai).await.unwrap();
    recover_summaries(&api, &store, &ai).await.unwrap();
    server.await.unwrap();
    let row = store.summaries(&ai, &cid).unwrap().remove(0);
    assert!(row.finished);
    assert_eq!(row.item.status, "pending_sync");
    assert_eq!(row.item.text, "completed model output");
}
