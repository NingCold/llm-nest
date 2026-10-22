use super::*;
use reqwest::{Client, Method};

struct Fixture {
    state: Arc<AppState>,
    admin: sqlx::PgPool,
    schema: String,
    _dir: tempfile::TempDir,
    server: tokio::task::JoinHandle<()>,
    url: String,
    client: Client,
    humans: Vec<(Uuid, String)>,
}
impl Fixture {
    async fn new() -> Option<Self> {
        let url = match std::env::var("CLOUD_TEST_DATABASE_URL") {
            Ok(v) => v,
            Err(_) => {
                eprintln!("CLOUD_TEST_DATABASE_URL absent: node PostgreSQL test skipped");
                return None;
            }
        };
        let admin = sqlx::postgres::PgPoolOptions::new()
            .max_connections(2)
            .connect(&url)
            .await
            .unwrap();
        let schema = format!("node_test_{}", Uuid::new_v4().simple());
        sqlx::query(&format!("CREATE SCHEMA {schema}"))
            .execute(&admin)
            .await
            .unwrap();
        let path = schema.clone();
        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(8)
            .after_connect(move |c, _| {
                let q = format!("SET search_path TO {path}");
                Box::pin(async move {
                    sqlx::query(&q).execute(c).await?;
                    Ok(())
                })
            })
            .connect(&url)
            .await
            .unwrap();
        sqlx::migrator().run(&pool).await.unwrap();
        let dir = tempfile::tempdir().unwrap();
        let state = Arc::new(AppState {
            pool,
            attachments: dir.path().join("attachments"),
            public_url: "https://test.example".into(),
            mail: auth::Mailer::test(dir.path().to_owned()),
        });
        let mut humans = vec![];
        for i in 0..2 {
            let uid = Uuid::new_v4();
            let secret = Uuid::new_v4().to_string();
            sqlx::query("INSERT INTO accounts(id,name) VALUES($1,$2)")
                .bind(uid)
                .bind(format!("human{i}"))
                .execute(&state.pool)
                .await
                .unwrap();
            sqlx::query("INSERT INTO sessions(id,account_id,token_hash,expires_at) VALUES($1,$2,$3,now()+interval '1 day')").bind(Uuid::new_v4()).bind(uid).bind(hash(&secret)).execute(&state.pool).await.unwrap();
            humans.push((uid, secret));
        }
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let app = crate::router(state.clone(), dir.path().to_owned());
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        Some(Self {
            state,
            admin,
            schema,
            _dir: dir,
            server,
            url,
            client: Client::new(),
            humans,
        })
    }
    fn human(&self, index: usize, method: Method, path: &str) -> reqwest::RequestBuilder {
        self.client
            .request(method, format!("{}/api{}", self.url, path))
            .header("x-llmn-client", "1")
            .header("cookie", format!("nest_session={}", self.humans[index].1))
    }
    fn node(&self, secret: &str, method: Method, path: &str) -> reqwest::RequestBuilder {
        self.client
            .request(method, format!("{}/api{}", self.url, path))
            .header("x-llmn-client", "1")
            .bearer_auth(secret)
    }
    async fn ai(&self) -> Value {
        value(
            self.human(0, Method::POST, "/ai")
                .json(&json!({"requestId":Uuid::new_v4(),"name":"Test AI","bio":"public bio"})),
        )
        .await
    }
    async fn bind_ai(&self, ai: &str, version: &str) -> (String, Uuid, Value) {
        let secret = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
        let node = Uuid::new_v4();
        let pair = value(
            self.node(&secret, Method::POST, "/node/pairings")
                .json(&json!({"nodeId":node,"nodeName":"isolated node"})),
        )
        .await;
        let response=value(self.human(0,Method::POST,&format!("/ai/{ai}/node")).json(&json!({"requestId":Uuid::new_v4(),"pairingCode":pair["userCode"],"expectedBindingVersion":version}))).await;
        let status = value(self.node(
            &secret,
            Method::GET,
            &format!("/node/pairings/{}", pair["pairingId"].as_str().unwrap()),
        ))
        .await;
        assert_eq!(status["aiId"], ai);
        assert_eq!(status["bindingVersion"], response["bindingVersion"]);
        (secret, node, response)
    }
    async fn conversation(&self, ai: &str) -> String {
        value(
            self.human(0, Method::POST, "/chat/direct")
                .json(&json!({"accountId":ai})),
        )
        .await["id"]
            .as_str()
            .unwrap()
            .into()
    }
    async fn send(&self, cid: &str, text: &str) -> Value {
        value(
            self.human(
                0,
                Method::POST,
                &format!("/chat/conversations/{cid}/messages"),
            )
            .json(&json!({"requestId":Uuid::new_v4(),"text":text})),
        )
        .await
    }
    async fn prepare(&self, secret: &str) -> (Value, Value) {
        let ts = value(self.node(secret, Method::GET, "/node/tasks")).await;
        let task = ts
            .as_array()
            .unwrap()
            .iter()
            .find(|t| t["status"] == "queued")
            .unwrap()
            .clone();
        let mat = value(self.node(
            secret,
            Method::GET,
            &format!("/node/tasks/{}/materials", task["id"].as_str().unwrap()),
        ))
        .await;
        let req = json!({"requestId":Uuid::new_v4(),"taskRevision":task["revision"],"sources":mat["sources"],"configurationRevision":"test-v1"});
        (task, req)
    }
    async fn close(self) {
        self.server.abort();
        let _ = self.server.await;
        self.state.pool.close().await;
        sqlx::query(&format!("DROP SCHEMA {} CASCADE", self.schema))
            .execute(&self.admin)
            .await
            .unwrap();
    }
}
async fn value(request: reqwest::RequestBuilder) -> Value {
    let response = request.send().await.unwrap();
    let status = response.status();
    let body = response.text().await.unwrap();
    assert!(status.is_success(), "HTTP {status}: {body}");
    serde_json::from_str(&body).unwrap()
}
fn final_result(receipt: &Value, text: &str) -> Value {
    json!({"requestId":Uuid::new_v4(),"runId":receipt["runId"],"messageId":receipt["messageId"],"confirmationId":receipt["confirmationId"],"status":"completed","text":text,"usage":{"inputTokens":20,"outputTokens":4,"cachedTokens":0}})
}

#[tokio::test]
async fn pairing_authorization_and_idempotent_execution() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let create = json!({"requestId":Uuid::new_v4(),"name":"Initial","bio":""});
    let ai = value(f.human(0, Method::POST, "/ai").json(&create)).await;
    assert_eq!(
        ai,
        value(f.human(0, Method::POST, "/ai").json(&create)).await
    );
    let aid = ai["account"]["id"].as_str().unwrap();
    assert_eq!(value(f.human(1, Method::GET, "/ai")).await, json!([]));
    let (secret, _, bound) = f.bind_ai(aid, "0").await;
    assert_eq!(bound["bindingVersion"], "1");
    assert_eq!(
        f.human(1, Method::POST, &format!("/ai/{aid}/node/revoke"))
            .json(&json!({"requestId":Uuid::new_v4(),"expectedBindingVersion":"1"}))
            .send()
            .await
            .unwrap()
            .status(),
        403
    );
    assert_eq!(
        f.node(&secret, Method::GET, "/chat/init")
            .send()
            .await
            .unwrap()
            .status(),
        401
    );
    let cid = f.conversation(aid).await;
    let trigger = f.send(&cid, "hello").await;
    assert_eq!(trigger["aiResponse"]["status"], "queued");
    let (task, start) = f.prepare(&secret).await;
    let path = format!("/node/tasks/{}/start", task["id"].as_str().unwrap());
    let receipt = value(f.node(&secret, Method::POST, &path).json(&start)).await;
    assert_eq!(
        receipt,
        value(f.node(&secret, Method::POST, &path).json(&start)).await
    );
    let mut other = start.clone();
    other["requestId"] = json!(Uuid::new_v4());
    assert_eq!(
        f.node(&secret, Method::POST, &path)
            .json(&other)
            .send()
            .await
            .unwrap()
            .status(),
        409
    );
    let body = final_result(&receipt, "real test reply");
    let path = path.replace("/start", "/result");
    assert_eq!(
        value(f.node(&secret, Method::POST, &path).json(&body)).await["status"],
        "completed"
    );
    assert_eq!(
        value(f.node(&secret, Method::POST, &path).json(&body)).await["status"],
        "completed"
    );
    let mut conflicting = body.clone();
    conflicting["text"] = json!("different");
    assert_eq!(
        f.node(&secret, Method::POST, &path)
            .json(&conflicting)
            .send()
            .await
            .unwrap()
            .status(),
        409
    );
    let rows = value(f.human(
        0,
        Method::GET,
        &format!("/chat/conversations/{cid}/messages"),
    ))
    .await;
    assert_eq!(rows["messages"].as_array().unwrap().len(), 2);
    assert_eq!(rows["messages"][1]["senderId"], aid);
    let hashes: Vec<String> =
        sqlx::query_scalar("SELECT token_hash FROM cloud_ai_bindings WHERE token_hash IS NOT NULL")
            .fetch_all(&f.state.pool)
            .await
            .unwrap();
    assert!(hashes.iter().all(|h| h != &secret));
    value(f.human(0,Method::PATCH,&format!("/chat/messages/{}",trigger["id"].as_str().unwrap())).json(&json!({"requestId":Uuid::new_v4(),"expectedRevision":"1","text":"edited after completed result"}))).await;
    assert_eq!(
        value(f.node(&secret, Method::POST, &path).json(&body)).await["status"],
        "completed"
    );
    assert_eq!(
        f.node(&secret, Method::POST, &path.replace("/result", "/start"))
            .json(&start)
            .send()
            .await
            .unwrap()
            .status(),
        409
    );
    f.close().await;
}

#[tokio::test]
async fn source_edits_cancel_and_node_replacement_fences_results() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let ai = f.ai().await;
    let aid = ai["account"]["id"].as_str().unwrap();
    let (secret, _, _) = f.bind_ai(aid, "0").await;
    let cid = f.conversation(aid).await;
    let trigger = f.send(&cid, "original").await;
    let (task, start) = f.prepare(&secret).await;
    let tid = task["id"].as_str().unwrap();
    let receipt = value(
        f.node(&secret, Method::POST, &format!("/node/tasks/{tid}/start"))
            .json(&start),
    )
    .await;
    let frame = connector::node::ClientFrame::Delta {
        task_id: tid.into(),
        run_id: receipt["runId"].as_str().unwrap().into(),
        message_id: receipt["messageId"].as_str().unwrap().into(),
        confirmation_id: receipt["confirmationId"].as_str().unwrap().into(),
        sequence: 1,
        text: "partial".into(),
    };
    let mut h = HeaderMap::new();
    h.insert(
        header::AUTHORIZATION,
        format!("Bearer {secret}").parse().unwrap(),
    );
    delta(&f.state, &h, frame.clone()).await.unwrap();
    delta(&f.state, &h, frame).await.unwrap();
    value(
        f.human(
            0,
            Method::PATCH,
            &format!("/chat/messages/{}", trigger["id"].as_str().unwrap()),
        )
        .json(&json!({"requestId":Uuid::new_v4(),"expectedRevision":"1","text":"revised"})),
    )
    .await;
    assert_eq!(
        value(f.node(&secret, Method::GET, &format!("/node/tasks/{tid}"))).await["status"],
        "cancelled"
    );
    assert_eq!(
        f.node(&secret, Method::POST, &format!("/node/tasks/{tid}/result"))
            .json(&final_result(&receipt, "late"))
            .send()
            .await
            .unwrap()
            .status(),
        409
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM cloud_messages WHERE status='generating'"
        )
        .fetch_one(&f.state.pool)
        .await
        .unwrap(),
        0
    );
    f.send(&cid, "new trigger").await;
    let (task, start) = f.prepare(&secret).await;
    let tid = task["id"].as_str().unwrap();
    let receipt = value(
        f.node(&secret, Method::POST, &format!("/node/tasks/{tid}/start"))
            .json(&start),
    )
    .await;
    let (new_secret, _, _) = f.bind_ai(aid, "1").await;
    assert_eq!(
        f.node(&secret, Method::POST, &format!("/node/tasks/{tid}/result"))
            .json(&final_result(&receipt, "revoked"))
            .send()
            .await
            .unwrap()
            .status(),
        401
    );
    assert_eq!(
        f.node(&new_secret, Method::GET, &format!("/node/tasks/{tid}"))
            .send()
            .await
            .unwrap()
            .status(),
        404
    );
    f.close().await;
}

#[tokio::test]
async fn websocket_drafts_recovery_and_terminal_delivery_position() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let ai = f.ai().await;
    let aid = ai["account"]["id"].as_str().unwrap();
    let (secret, node, _) = f.bind_ai(aid, "0").await;
    let cid = f.conversation(aid).await;
    let client = connector::node::NodeClient::new(&f.url, &secret, None).unwrap();
    let mut ws = client.connect().await.unwrap();
    ws.send(&connector::node::ClientFrame::Hello {
        protocol_version: 1,
        node_id: node.to_string(),
        ai_id: aid.into(),
        binding_version: "1".into(),
    })
    .await
    .unwrap();
    assert_eq!(
        ws.recv().await.unwrap(),
        Some(connector::node::ServerFrame::Ready {
            protocol_version: 1
        })
    );
    let (outgoing, mut commands) = tokio::sync::mpsc::unbounded_channel();
    let (notifications, mut incoming) = tokio::sync::mpsc::unbounded_channel();
    let pump = tokio::spawn(async move {
        loop {
            tokio::select! {
                frame = commands.recv() => match frame {
                    Some(frame) => ws.send(&frame).await.unwrap(),
                    None => { ws.close().await.unwrap(); break; }
                },
                frame = ws.recv() => match frame.unwrap() {
                    Some(frame) => { if notifications.send(frame).is_err() { break; } },
                    None => break,
                }
            }
        }
    });
    let trigger = f.send(&cid, "draft trigger").await;
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(30), incoming.recv())
            .await
            .unwrap(),
        Some(connector::node::ServerFrame::TasksChanged)
    );
    let (task, start) = f.prepare(&secret).await;
    let tid = task["id"].as_str().unwrap();
    let receipt = value(
        f.node(&secret, Method::POST, &format!("/node/tasks/{tid}/start"))
            .json(&start),
    )
    .await;
    outgoing
        .send(connector::node::ClientFrame::Delta {
            task_id: tid.into(),
            run_id: receipt["runId"].as_str().unwrap().into(),
            message_id: receipt["messageId"].as_str().unwrap().into(),
            confirmation_id: receipt["confirmationId"].as_str().unwrap().into(),
            sequence: 1,
            text: "partial answer".into(),
        })
        .unwrap();
    loop {
        if matches!(
            tokio::time::timeout(Duration::from_secs(30), incoming.recv())
                .await
                .unwrap(),
            Some(connector::node::ServerFrame::DeltaAccepted { sequence: 1, .. })
        ) {
            break;
        }
    }
    let page = value(f.human(
        0,
        Method::GET,
        &format!("/chat/conversations/{cid}/messages"),
    ))
    .await;
    assert_eq!(page["latestPosition"], trigger["position"]);
    assert_eq!(page["messages"][1]["status"], "generating");
    let draft_position = page["messages"][1]["position"]
        .as_str()
        .unwrap()
        .parse::<i64>()
        .unwrap();
    let init = value(f.human(0, Method::GET, "/chat/init")).await;
    assert_eq!(init["conversations"][0]["unreadCount"], 0);
    assert_eq!(
        init["conversations"][0]["latestPosition"],
        trigger["position"]
    );
    let search = value(f.human(0, Method::GET, "/chat/search?query=partial")).await;
    assert_eq!(search["messages"], json!([]));
    assert_eq!(f.human(0,Method::PUT,&format!("/chat/conversations/{cid}/list")).json(&json!({"requestId":Uuid::new_v4(),"expectedRevision":"1","readPosition":draft_position.to_string()})).send().await.unwrap().status(),400);
    drop(outgoing);
    pump.await.unwrap();
    recover(&f.state.pool).await.unwrap();
    assert_eq!(
        value(f.node(&secret, Method::GET, &format!("/node/tasks/{tid}"))).await["status"],
        "uncertain"
    );
    assert_eq!(
        value(f.node(&secret, Method::GET, "/node/tasks")).await,
        json!([])
    );
    let page = value(f.human(
        0,
        Method::GET,
        &format!("/chat/conversations/{cid}/messages"),
    ))
    .await;
    assert_eq!(page["messages"][1]["status"], "interrupted");
    assert!(
        page["messages"][1]["position"]
            .as_str()
            .unwrap()
            .parse::<i64>()
            .unwrap()
            > draft_position
    );
    let response = value(
        f.node(&secret, Method::POST, &format!("/node/tasks/{tid}/result"))
            .json(&final_result(&receipt, "saved complete result")),
    )
    .await;
    assert_eq!(response["status"], "completed");
    let init = value(f.human(0, Method::GET, "/chat/init")).await;
    assert_eq!(init["conversations"][0]["unreadCount"], 1);
    f.close().await;
}

#[tokio::test]
async fn preparation_failure_expiry_and_authorized_images() {
    use base64::Engine;
    let Some(f) = Fixture::new().await else {
        return;
    };
    let ai = f.ai().await;
    let aid = ai["account"]["id"].as_str().unwrap();
    let (secret, _, _) = f.bind_ai(aid, "0").await;
    let cid = f.conversation(aid).await;
    let mut png = std::io::Cursor::new(vec![]);
    image::DynamicImage::new_rgb8(1, 1)
        .write_to(&mut png, image::ImageFormat::Png)
        .unwrap();
    let trigger=value(f.human(0,Method::POST,&format!("/chat/conversations/{cid}/messages")).json(&json!({"requestId":Uuid::new_v4(),"text":"image and file","images":[{"mime":"image/png","data":base64::engine::general_purpose::STANDARD.encode(png.get_ref())}],"files":[{"name":"private.txt","mime":"text/plain","data":"aGVsbG8="}]}))).await;
    let (task, start) = f.prepare(&secret).await;
    let tid = task["id"].as_str().unwrap();
    let mid = trigger["id"].as_str().unwrap();
    let path = format!("/node/tasks/{tid}/messages/{mid}/attachments/0?revision=1");
    assert_eq!(
        f.node(&secret, Method::GET, &path)
            .send()
            .await
            .unwrap()
            .bytes()
            .await
            .unwrap()
            .as_ref(),
        png.get_ref()
    );
    assert_eq!(
        f.node(
            &secret,
            Method::GET,
            &path.replace("revision=1", "revision=2")
        )
        .send()
        .await
        .unwrap()
        .status(),
        409
    );
    assert_eq!(
        f.node(
            &secret,
            Method::GET,
            &path.replace("attachments/0", "attachments/1")
        )
        .send()
        .await
        .unwrap()
        .status(),
        403
    );
    assert_eq!(
        f.human(1, Method::GET, &path)
            .send()
            .await
            .unwrap()
            .status(),
        401
    );
    let reject = json!({"requestId":Uuid::new_v4(),"taskRevision":task["revision"],"code":"image_input_unsupported","message":"selected model does not accept images"});
    assert_eq!(
        value(
            f.node(&secret, Method::POST, &format!("/node/tasks/{tid}/reject"))
                .json(&reject)
        )
        .await["status"],
        "failed"
    );
    assert_eq!(
        value(
            f.node(&secret, Method::POST, &format!("/node/tasks/{tid}/reject"))
                .json(&reject)
        )
        .await["status"],
        "failed"
    );
    assert_eq!(
        f.node(&secret, Method::POST, &format!("/node/tasks/{tid}/start"))
            .json(&start)
            .send()
            .await
            .unwrap()
            .status(),
        409
    );
    f.send(&cid, "expire before model call").await;
    let (task, start) = f.prepare(&secret).await;
    let tid = task["id"].as_str().unwrap();
    sqlx::query("UPDATE cloud_node_tasks SET expires_at=now()-interval '1 second' WHERE id=$1")
        .bind(tid.parse::<Uuid>().unwrap())
        .execute(&f.state.pool)
        .await
        .unwrap();
    expire_pending(&f.state.pool).await.unwrap();
    let page = value(f.human(
        0,
        Method::GET,
        &format!("/chat/conversations/{cid}/messages"),
    ))
    .await;
    assert_eq!(
        page["messages"].as_array().unwrap().last().unwrap()["aiResponse"]["status"],
        "expired"
    );
    assert_eq!(
        f.node(&secret, Method::POST, &format!("/node/tasks/{tid}/start"))
            .json(&start)
            .send()
            .await
            .unwrap()
            .status(),
        409
    );
    assert_eq!(
        value(f.node(&secret, Method::GET, &format!("/node/tasks/{tid}"))).await["status"],
        "expired"
    );
    f.send(&cid, "expire after confirmed start").await;
    let (task, start) = f.prepare(&secret).await;
    let tid = task["id"].as_str().unwrap();
    let receipt = value(
        f.node(&secret, Method::POST, &format!("/node/tasks/{tid}/start"))
            .json(&start),
    )
    .await;
    sqlx::query("UPDATE cloud_node_tasks SET expires_at=now()-interval '1 second' WHERE id=$1")
        .bind(tid.parse::<Uuid>().unwrap())
        .execute(&f.state.pool)
        .await
        .unwrap();
    assert_eq!(
        f.node(&secret, Method::POST, &format!("/node/tasks/{tid}/start"))
            .json(&start)
            .send()
            .await
            .unwrap()
            .status(),
        409
    );
    assert_eq!(
        f.node(&secret, Method::POST, &format!("/node/tasks/{tid}/result"))
            .json(&final_result(&receipt, "expired late result"))
            .send()
            .await
            .unwrap()
            .status(),
        409
    );
    value(f.node(&secret, Method::GET, "/node/tasks")).await;
    f.close().await;
}

#[tokio::test]
async fn explicit_cancel_preserves_partial_and_revoke_invalidates_credential() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let ai = f.ai().await;
    let aid = ai["account"]["id"].as_str().unwrap();
    let (secret, _, _) = f.bind_ai(aid, "0").await;
    let cid = f.conversation(aid).await;
    f.send(&cid, "cancel this reply").await;
    let (task, start) = f.prepare(&secret).await;
    let tid = task["id"].as_str().unwrap();
    let receipt = value(
        f.node(&secret, Method::POST, &format!("/node/tasks/{tid}/start"))
            .json(&start),
    )
    .await;
    let mut h = HeaderMap::new();
    h.insert(
        header::AUTHORIZATION,
        format!("Bearer {secret}").parse().unwrap(),
    );
    delta(
        &f.state,
        &h,
        connector::node::ClientFrame::Delta {
            task_id: tid.into(),
            run_id: receipt["runId"].as_str().unwrap().into(),
            message_id: receipt["messageId"].as_str().unwrap().into(),
            confirmation_id: receipt["confirmationId"].as_str().unwrap().into(),
            sequence: 1,
            text: "retained partial".into(),
        },
    )
    .await
    .unwrap();
    let request = json!({"requestId":Uuid::new_v4()});
    let path = format!("/chat/ai-tasks/{tid}/cancel");
    assert_eq!(
        f.human(1, Method::POST, &path)
            .json(&request)
            .send()
            .await
            .unwrap()
            .status(),
        404
    );
    assert_eq!(
        value(f.human(0, Method::POST, &path).json(&request)).await["status"],
        "cancelled"
    );
    assert_eq!(
        value(f.human(0, Method::POST, &path).json(&request)).await["status"],
        "cancelled"
    );
    let page = value(f.human(
        0,
        Method::GET,
        &format!("/chat/conversations/{cid}/messages"),
    ))
    .await;
    assert_eq!(page["messages"][1]["text"], "retained partial");
    assert_eq!(page["messages"][1]["status"], "interrupted");
    assert_eq!(
        f.node(&secret, Method::POST, &format!("/node/tasks/{tid}/result"))
            .json(&final_result(&receipt, "cancelled late result"))
            .send()
            .await
            .unwrap()
            .status(),
        409
    );
    value(
        f.human(0, Method::POST, &format!("/ai/{aid}/node/revoke"))
            .json(&json!({"requestId":Uuid::new_v4(),"expectedBindingVersion":"1"})),
    )
    .await;
    assert_eq!(
        f.node(&secret, Method::GET, "/node/tasks")
            .send()
            .await
            .unwrap()
            .status(),
        401
    );
    f.close().await;
}
