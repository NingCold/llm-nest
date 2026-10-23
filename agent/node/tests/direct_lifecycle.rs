//! Real managed executable: offline local execution and cloud history acceptance.
use connector::direct::{LocalOperation, LocalReply, LocalRequest, ipc};
use serde_json::{Value, json};
use std::{
    path::Path,
    process::Stdio,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
fn command(verb: &str, dir: &Path) -> tokio::process::Command {
    let mut c = tokio::process::Command::new(env!("CARGO_BIN_EXE_llmn-agent"));
    c.env("LLMN_DIRECT_TEST_KEY", "fixture")
        .arg(verb)
        .arg(dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .kill_on_drop(true);
    #[cfg(windows)]
    c.creation_flags(0x08000000);
    c
}
async fn rpc(dir: &Path, owner: &str, operation: LocalOperation) -> LocalReply {
    tokio::time::timeout(
        Duration::from_secs(5),
        ipc::call(
            dir,
            &LocalRequest {
                owner_id: owner.into(),
                operation,
            },
        ),
    )
    .await
    .unwrap()
    .unwrap()
}
#[tokio::test]
async fn offline_text_retry_and_sync_keep_message_ids_without_cloud_start() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let ai = uuid::Uuid::new_v4().to_string();
    let owner = uuid::Uuid::new_v4().to_string();
    let cid = uuid::Uuid::new_v4().to_string();
    let account = |id: &str, kind: &str| json!({"id":id,"name":kind,"bio":"","kind":kind});
    let bootstrap = json!({"ai":account(&ai,"ai"),"human":account(&owner,"human"),"conversation":{"id":cid,"kind":"direct","name":"","revision":"1","members":[account(&owner,"human"),account(&ai,"ai")]},"bindingVersion":"1","base":null,"messages":[],"messageStatuses":{},"hasMore":false});
    let bootstrap = Arc::new(Mutex::new(bootstrap));
    let offline = Arc::new(AtomicBool::new(false));
    let hold = Arc::new(AtomicBool::new(false));
    let calls = Arc::new(AtomicUsize::new(0));
    let starts = Arc::new(AtomicUsize::new(0));
    let accepted = Arc::new(Mutex::new(Vec::<Value>::new()));
    let h = hold.clone();
    let lose_reply = Arc::new(AtomicBool::new(true));
    let (o, n, s, a) = (
        offline.clone(),
        calls.clone(),
        starts.clone(),
        accepted.clone(),
    );
    let server = tokio::spawn(async move {
        loop {
            let (mut stream, _) = listener.accept().await.unwrap();
            let h = h.clone();
            let lose_reply = lose_reply.clone();
            let (o, n, s, a, b) = (
                o.clone(),
                n.clone(),
                s.clone(),
                a.clone(),
                bootstrap.clone(),
            );
            tokio::spawn(async move {
                let mut bytes = Vec::new();
                let end = loop {
                    let mut buf = [0; 4096];
                    let len = stream.read(&mut buf).await.unwrap();
                    if len == 0 {
                        return;
                    }
                    bytes.extend_from_slice(&buf[..len]);
                    if let Some(i) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                        break i + 4;
                    }
                };
                let headers = String::from_utf8_lossy(&bytes[..end]);
                let path = headers
                    .lines()
                    .next()
                    .unwrap()
                    .split_whitespace()
                    .nth(1)
                    .unwrap()
                    .to_string();
                let length = headers
                    .lines()
                    .find_map(|l| {
                        l.to_lowercase()
                            .strip_prefix("content-length:")
                            .and_then(|v| v.trim().parse::<usize>().ok())
                    })
                    .unwrap_or(0);
                while bytes.len() < end + length {
                    let mut buf = [0; 4096];
                    let len = stream.read(&mut buf).await.unwrap();
                    if len == 0 {
                        return;
                    }
                    bytes.extend_from_slice(&buf[..len]);
                }
                let body: Value =
                    serde_json::from_slice(&bytes[end..end + length]).unwrap_or(Value::Null);
                let (status, mime, response) = if path.contains("chat/completions") {
                    n.fetch_add(1, Ordering::SeqCst);
                    if h.load(Ordering::SeqCst) {
                        tokio::time::sleep(Duration::from_secs(30)).await;
                    }
                    (200,"text/event-stream","data: {\"id\":\"fixture\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"offline reply\"},\"finish_reason\":null}]}\n\ndata: {\"id\":\"fixture\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n".into())
                } else if o.load(Ordering::SeqCst) {
                    (503, "application/json", "{}".into())
                } else if path.contains("/direct/prepare") {
                    (200, "application/json", b.lock().unwrap().to_string())
                } else if path.contains("/direct/sync") {
                    a.lock().unwrap().push(body.clone());
                    let msgs=body["messages"].as_array().unwrap().iter().enumerate().map(|(i,m)|json!({"message":{"id":m["id"],"senderId":m["senderId"],"senderName":"fixture","text":m["text"],"position":(i+1).to_string(),"revision":"1","createdAt":m["createdAt"],"replyTo":null,"mentions":[],"attachments":[]},"status":m["status"]})).collect::<Vec<_>>();
                    let tail = &msgs.last().unwrap()["message"];
                    let head = json!({"messageId":tail["id"],"revision":tail["revision"],"position":tail["position"]});
                    {
                        let mut current = b.lock().unwrap();
                        current["messages"] = json!(
                            msgs.iter()
                                .map(|m| m["message"].clone())
                                .collect::<Vec<_>>()
                        );
                        current["base"] = head.clone();
                        current["messageStatuses"] = Value::Object(
                            msgs.iter()
                                .map(|m| {
                                    (
                                        m["message"]["id"].as_str().unwrap().to_owned(),
                                        m["status"].clone(),
                                    )
                                })
                                .collect(),
                        );
                    }
                    if lose_reply.swap(false, Ordering::SeqCst) {
                        let _ = stream.write_all(b"HTTP/1.1 503 Unavailable\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}").await;
                        return;
                    }
                    (
                        200,
                        "application/json",
                        json!({"messages":msgs,"head":head}).to_string(),
                    )
                } else if path.contains("/changes") {
                    (
                        200,
                        "application/json",
                        json!({"cursor":"0","hasMore":false,"changes":[]}).to_string(),
                    )
                } else if path.contains("/validate") {
                    (200, "application/json", json!({"valid":true}).to_string())
                } else if path.contains("/materials") {
                    (200, "application/json", "{}".into())
                } else {
                    if path.contains("/start") {
                        s.fetch_add(1, Ordering::SeqCst);
                    }
                    (200, "application/json", "{}".into())
                };
                let response = format!(
                    "HTTP/1.1 {status} OK\r\nContent-Type: {mime}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{response}",
                    response.len()
                );
                let _ = stream.write_all(response.as_bytes()).await;
            });
        }
    });
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("node");
    assert!(
        command("init", &dir)
            .output()
            .await
            .unwrap()
            .status
            .success()
    );
    let config_path = dir.join("node.toml");
    let mut config: toml::Value =
        toml::from_str(&std::fs::read_to_string(&config_path).unwrap()).unwrap();
    config["server"] = url.clone().into();
    config["profile"]=toml::Value::try_from(json!({"revision":"1","persona":"Be helpful","selection":{"provider":"fixture","model":"fixture"},"max_tokens":128})).unwrap();
    config["providers"]=toml::Value::try_from(json!({"fixture":{"protocol":"openai","api_key":{"env":"LLMN_DIRECT_TEST_KEY"},"base_url":format!("{url}/v1"),"models":{"fixture":{"model":"fixture","context_window":8192,"max_tokens":1024}}}})).unwrap();
    std::fs::write(config_path, toml::to_string(&config).unwrap()).unwrap();
    {
        let db = rusqlite::Connection::open(dir.join("node.sqlite")).unwrap();
        db.execute("INSERT INTO bindings VALUES(?1,'1','fixture')", [&ai])
            .unwrap();
        db.execute(
            "INSERT INTO profiles VALUES(?1,?2)",
            rusqlite::params![ai, serde_json::to_string(&config["profile"]).unwrap()],
        )
        .unwrap();
    }
    let mut child = command("run-managed", &dir).spawn().unwrap();
    let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
    tokio::time::timeout(Duration::from_secs(5), lines.next_line())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert!(matches!(
        rpc(
            &dir,
            &owner,
            LocalOperation::Prepare {
                ai_id: ai,
                grant: "fixture".into()
            }
        )
        .await,
        LocalReply::View { .. }
    ));
    offline.store(true, Ordering::SeqCst);
    let request = uuid::Uuid::new_v4().to_string();
    for _ in 0..2 {
        assert!(matches!(
            rpc(
                &dir,
                &owner,
                LocalOperation::Send {
                    key_messages: vec![],
                    sealed: None,
                    message_id: None,
                    conversation_id: cid.clone(),
                    request_id: request.clone(),
                    text: "hello offline".into()
                }
            )
            .await,
            LocalReply::View { .. }
        ));
    }
    let ids = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if let LocalReply::View { view } = rpc(
                &dir,
                &owner,
                LocalOperation::View {
                    conversation_id: cid.clone(),
                },
            )
            .await
            {
                if !view.running
                    && !view
                        .messages
                        .iter()
                        .any(|m| m.message.text == "offline reply")
                {
                    let db = rusqlite::Connection::open(dir.join("node.sqlite")).unwrap();
                    let payload: String = db
                        .query_row("SELECT payload FROM direct_turns LIMIT 1", [], |r| r.get(0))
                        .unwrap();
                    panic!("generation failed: {payload}");
                }
                if !view.running
                    && view
                        .messages
                        .iter()
                        .any(|m| m.message.text == "offline reply")
                {
                    break view
                        .messages
                        .iter()
                        .map(|m| m.message.id.clone())
                        .collect::<Vec<_>>();
                }
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert!(matches!(
        rpc(
            &dir,
            "other",
            LocalOperation::View {
                conversation_id: cid.clone()
            }
        )
        .await,
        LocalReply::Error { .. }
    ));
    offline.store(false, Ordering::SeqCst);
    let _ = rpc(
        &dir,
        &owner,
        LocalOperation::Sync {
            conversation_id: cid.clone(),
        },
    )
    .await;
    tokio::time::timeout(Duration::from_secs(8), async {
        loop {
            if accepted.lock().unwrap().len() >= 2 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
    .await
    .unwrap();
    {
        let accepted = accepted.lock().unwrap();
        assert_eq!(
            accepted[0], accepted[1],
            "lost sync reply must retry the frozen payload"
        );
        assert_eq!(
            accepted[0]["messages"]
                .as_array()
                .unwrap()
                .iter()
                .map(|m| m["id"].as_str().unwrap().to_string())
                .collect::<Vec<_>>(),
            ids
        );
        assert_eq!(starts.load(Ordering::SeqCst), 0);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }
    // Refresh from the authoritative accepted cloud tail, retaining both records.
    if let LocalReply::View { view } = rpc(
        &dir,
        &owner,
        LocalOperation::Sync {
            conversation_id: cid.clone(),
        },
    )
    .await
    {
        assert_eq!(
            view.messages
                .iter()
                .map(|m| m.message.id.clone())
                .collect::<Vec<_>>(),
            ids
        );
    } else {
        panic!("accepted history refresh failed");
    }
    offline.store(true, Ordering::SeqCst);
    hold.store(true, Ordering::SeqCst);
    let second = uuid::Uuid::new_v4().to_string();
    assert!(matches!(
        rpc(
            &dir,
            &owner,
            LocalOperation::Send {
                key_messages: vec![],
                sealed: None,
                message_id: None,
                conversation_id: cid.clone(),
                request_id: second.clone(),
                text: "second run".into()
            }
        )
        .await,
        LocalReply::View { .. }
    ));
    tokio::time::timeout(Duration::from_secs(5), async {
        while calls.load(Ordering::SeqCst) < 2 {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .unwrap();
    // An actual process crash leaves a persisted started invocation and never reopens it.
    child.kill().await.unwrap();
    let mut restarted = command("run-managed", &dir).spawn().unwrap();
    let mut lines = BufReader::new(restarted.stdout.take().unwrap()).lines();
    tokio::time::timeout(Duration::from_secs(5), lines.next_line())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    if let LocalReply::View { view } = rpc(
        &dir,
        &owner,
        LocalOperation::View {
            conversation_id: cid.clone(),
        },
    )
    .await
    {
        assert!(!view.running);
        assert!(view.messages.iter().any(|m| m.status == "interrupted"));
    } else {
        panic!("restart view failed");
    }
    assert!(matches!(
        rpc(
            &dir,
            &owner,
            LocalOperation::Send {
                key_messages: vec![],
                sealed: None,
                message_id: None,
                conversation_id: cid.clone(),
                request_id: second,
                text: "second run".into()
            }
        )
        .await,
        LocalReply::View { .. }
    ));
    tokio::time::sleep(Duration::from_millis(700)).await;
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert!(matches!(
        rpc(
            &dir,
            &owner,
            LocalOperation::Stop {
                conversation_id: cid
            }
        )
        .await,
        LocalReply::View { .. }
    ));
    restarted.kill().await.unwrap();
    server.abort();
}
