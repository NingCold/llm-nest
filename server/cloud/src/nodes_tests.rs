use super::*;
use reqwest::{Client, Method, StatusCode};

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
            relay: Default::default(),
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
        let cid = value(
            self.human(0, Method::POST, "/chat/direct")
                .json(&json!({"accountId":ai})),
        )
        .await["id"]
            .as_str()
            .unwrap()
            .to_owned();
        sqlx::query("INSERT INTO cloud_participation(conversation,policy) VALUES($1,jsonb_set('{\"mode\":\"mentionedOnly\",\"directMergeMs\":800,\"maxConsecutiveAi\":3,\"maxBoutCalls\":12,\"maxDailyCalls\":48,\"idleSeconds\":300}'::jsonb,'{directMergeMs}','0')) ON CONFLICT(conversation) DO UPDATE SET policy=jsonb_set(cloud_participation.policy,'{directMergeMs}','0')").bind(cid.parse::<Uuid>().unwrap()).execute(&self.state.pool).await.unwrap();
        cid
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
        let req = self.start_request(secret, &task).await;
        (task, req)
    }
    async fn scope(&self, secret: &str, cid: &str) -> Value {
        let old:Option<Uuid>=sqlx::query_scalar("SELECT e.revision FROM cloud_material_epochs e JOIN cloud_ai_bindings b ON b.ai_id=e.ai_id WHERE b.token_hash=$1 AND e.conversation=$2").bind(hash(secret)).bind(cid.parse::<Uuid>().unwrap()).fetch_optional(&self.state.pool).await.unwrap();
        let scope = json!({"conversationId":cid,"revision":old.unwrap_or_else(Uuid::new_v4)});
        value(
            self.node(secret, Method::POST, "/node/consciousness/state")
                .json(&scope),
        )
        .await;
        scope
    }
    async fn start_request(&self, secret: &str, task: &Value) -> Value {
        let mat = value(self.node(
            secret,
            Method::GET,
            &format!("/node/tasks/{}/materials", task["id"].as_str().unwrap()),
        ))
        .await;
        let scope = self
            .scope(secret, task["conversationId"].as_str().unwrap())
            .await;
        let derived=mat["reference"].get("sources").and_then(Value::as_array).map(|sources|sources.iter().map(|source|json!({"conversationId":mat["reference"]["conversation"]["id"],"messageId":source["messageId"],"revision":source["revision"]})).collect::<Vec<_>>()).unwrap_or_default();
        json!({"requestId":Uuid::new_v4(),"taskRevision":task["revision"],"sources":mat["sources"],"configurationRevision":"test-v3","conversationRevision":mat["conversation"]["revision"],"derivedSources":derived,"materialScopes":[scope]})
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
    assert_eq!(trigger["aiResponses"][0]["status"], "queued");
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
        sealed: None,
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
    let mut obsolete = client.connect().await.unwrap();
    obsolete
        .send(&connector::node::ClientFrame::Hello {
            capabilities: vec![],
            protocol_version: 1,
            node_id: node.to_string(),
            ai_id: aid.into(),
            binding_version: "1".into(),
        })
        .await
        .unwrap();
    assert!(
        matches!(obsolete.recv().await.unwrap(),Some(connector::node::ServerFrame::Error{code,..}) if code=="invalid_hello")
    );
    let mut ws = client.connect().await.unwrap();
    ws.send(&connector::node::ClientFrame::Hello {
        capabilities: vec![],
        protocol_version: 4,
        node_id: node.to_string(),
        ai_id: aid.into(),
        binding_version: "1".into(),
    })
    .await
    .unwrap();
    assert_eq!(
        ws.recv().await.unwrap(),
        Some(connector::node::ServerFrame::Ready {
            protocol_version: 4
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
            sealed: None,
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
        page["messages"].as_array().unwrap().last().unwrap()["aiResponses"][0]["status"],
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
            sealed: None,
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

#[tokio::test]
async fn groups_fifo_multiple_nodes_frozen_sources_and_member_permissions() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let a = f.ai().await;
    let b = f.ai().await;
    let aid = a["account"]["id"].as_str().unwrap();
    let bid = b["account"]["id"].as_str().unwrap();
    let (sa, _, _) = f.bind_ai(aid, "0").await;
    let (sb, _, _) = f.bind_ai(bid, "0").await;
    let create =
        json!({"requestId":Uuid::new_v4(),"name":"真实群","memberIds":[f.humans[1].0,aid,bid]});
    let group = value(f.human(0, Method::POST, "/chat/groups").json(&create)).await;
    let cid = group["id"].as_str().unwrap();
    assert_eq!(group["kind"], "group");
    assert!(group["peer"].is_null());
    assert_eq!(group["members"].as_array().unwrap().len(), 4);
    assert_eq!(
        value(f.human(0, Method::POST, "/chat/groups").json(&create)).await,
        group
    );
    let send_path = format!("/chat/conversations/{cid}/messages");
    let plain = f.send(cid, "Test AI 普通名字不触发").await;
    assert_eq!(plain["aiResponses"], json!([]));
    let input = json!({"requestId":Uuid::new_v4(),"text":"一起回答","mentions":[aid,bid,aid]});
    let trigger = value(f.human(0, Method::POST, &send_path).json(&input)).await;
    assert_eq!(trigger["aiResponses"].as_array().unwrap().len(), 2);
    assert_eq!(trigger["aiResponses"][0]["aiId"], aid);
    assert_eq!(trigger["senderName"], "human0");
    let duplicate = value(f.human(0, Method::POST, &send_path).json(&input)).await;
    assert_eq!(duplicate["id"], trigger["id"]);
    assert_eq!(
        value(f.node(&sb, Method::GET, "/node/tasks")).await,
        json!([])
    );
    let second = trigger["aiResponses"][1]["taskId"].as_str().unwrap();
    let mb = value(f.node(&sb, Method::GET, &format!("/node/tasks/{second}/materials"))).await;
    let premature = f.start_request(&sb, &mb["task"]).await;
    let response = f
        .node(&sb, Method::POST, &format!("/node/tasks/{second}/start"))
        .json(&premature)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 409);
    assert_eq!(
        response.json::<Value>().await.unwrap()["code"],
        "task_not_ready"
    );
    let (first, start) = f.prepare(&sa).await;
    let first_id = first["id"].as_str().unwrap();
    f.send(cid, "准备后未来无关输入").await;
    let still_prepared = value(f.node(
        &sa,
        Method::GET,
        &format!("/node/tasks/{first_id}/materials"),
    ))
    .await;
    assert_eq!(still_prepared["sources"], start["sources"]);
    assert!(
        !still_prepared["messages"]
            .as_array()
            .unwrap()
            .iter()
            .any(|m| m["text"] == "准备后未来无关输入")
    );

    let (first_started, second_started) = tokio::join!(
        f.node(&sa, Method::POST, &format!("/node/tasks/{first_id}/start"))
            .json(&start)
            .send(),
        f.node(&sb, Method::POST, &format!("/node/tasks/{second}/start"))
            .json(&premature)
            .send()
    );
    let first_started = first_started.unwrap();
    assert_eq!(first_started.status(), 200);
    let receipt = first_started.json::<Value>().await.unwrap();
    assert_eq!(second_started.unwrap().status(), 409);
    assert_eq!(
        value(f.node(&sb, Method::GET, "/node/tasks")).await,
        json!([])
    );
    value(
        f.node(&sa, Method::POST, &format!("/node/tasks/{first_id}/result"))
            .json(&final_result(&receipt, "第一位回答")),
    )
    .await;
    let (second_task, start2) = f.prepare(&sb).await;
    assert_eq!(second_task["id"], second);
    let mat2 = value(f.node(&sb, Method::GET, &format!("/node/tasks/{second}/materials"))).await;
    assert!(
        mat2["messages"]
            .as_array()
            .unwrap()
            .iter()
            .any(|m| m["text"] == "第一位回答")
    );
    assert!(
        !mat2["messages"]
            .as_array()
            .unwrap()
            .iter()
            .any(|m| m["text"] == "准备后未来无关输入")
    );
    let receipt2 = value(
        f.node(&sb, Method::POST, &format!("/node/tasks/{second}/start"))
            .json(&start2),
    )
    .await;
    f.send(cid, "开始后新的真人消息不改变冻结材料").await;
    value(
        f.node(&sb, Method::POST, &format!("/node/tasks/{second}/result"))
            .json(&final_result(&receipt2, "第二位回答")),
    )
    .await;
    let running: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM cloud_node_tasks WHERE conversation=$1 AND state='running'",
    )
    .bind(cid.parse::<Uuid>().unwrap())
    .fetch_one(&f.state.pool)
    .await
    .unwrap();
    assert_eq!(running, 0);
    let denied = f
        .human(1, Method::PUT, &format!("/chat/groups/{cid}"))
        .json(&json!({"requestId":Uuid::new_v4(),"expectedRevision":"1","name":"无权限"}))
        .send()
        .await
        .unwrap();
    assert_eq!(denied.status(), 403);
    let leave = json!({"requestId":Uuid::new_v4(),"expectedRevision":"1"});
    value(
        f.human(1, Method::POST, &format!("/chat/groups/{cid}/leave"))
            .json(&leave),
    )
    .await;
    assert_eq!(
        value(
            f.human(1, Method::POST, &format!("/chat/groups/{cid}/leave"))
                .json(&leave)
        )
        .await,
        json!({"ok":true})
    );
    for path in [
        &send_path,
        &format!("/chat/conversations/{cid}/export?format=json"),
        &format!("/chat/messages/{}/context", trigger["id"].as_str().unwrap()),
    ] {
        assert_eq!(
            f.human(1, Method::GET, path).send().await.unwrap().status(),
            404
        );
    }
    assert_eq!(
        f.human(
            1,
            Method::GET,
            &format!("/chat/search?query=一起&conversationId={cid}")
        )
        .send()
        .await
        .unwrap()
        .status(),
        404
    );
    let search = value(f.human(1, Method::GET, "/chat/search?query=一起")).await;
    assert!(search["messages"].as_array().unwrap().is_empty());
    // A removed member cannot replay an earlier command to retrieve the current message.
    assert_eq!(
        f.human(1, Method::POST, &send_path)
            .json(&json!({"requestId":Uuid::new_v4(),"text":"离群后"}))
            .send()
            .await
            .unwrap()
            .status(),
        404
    );
    f.close().await;
}

#[tokio::test]
async fn groups_roles_versions_block_and_ai_disable_do_not_replay() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let ai = f.ai().await;
    let aid = ai["account"]["id"].as_str().unwrap();
    let (secret, _, _) = f.bind_ai(aid, "0").await;
    let cid = f.conversation(aid).await;
    f.send(&cid, "排队私聊").await;
    let blocked = json!({"requestId":Uuid::new_v4(),"expectedRevision":"0","blocked":true});
    let relation = value(
        f.human(0, Method::PUT, &format!("/chat/contacts/{aid}/block"))
            .json(&blocked),
    )
    .await;
    assert_eq!(relation["directBlocked"], true);
    assert_eq!(relation["listed"], false);
    assert_eq!(
        value(f.node(&secret, Method::GET, "/node/tasks")).await,
        json!([])
    );
    assert_eq!(
        f.human(
            0,
            Method::POST,
            &format!("/chat/conversations/{cid}/messages")
        )
        .json(&json!({"requestId":Uuid::new_v4(),"text":"blocked"}))
        .send()
        .await
        .unwrap()
        .status(),
        403
    );
    // Listing/removing preserves the independent block bit.
    value(
        f.human(0, Method::POST, "/chat/contacts")
            .json(&json!({"accountId":aid,"listed":true})),
    )
    .await;
    value(
        f.human(0, Method::POST, "/chat/contacts")
            .json(&json!({"accountId":aid,"listed":false})),
    )
    .await;
    assert_eq!(
        value(f.human(0, Method::GET, &format!("/chat/contacts/{aid}"))).await["blocked"],
        true
    );
    let group=value(f.human(0,Method::POST,"/chat/groups").json(&json!({"requestId":Uuid::new_v4(),"name":"群不受私聊屏蔽影响","memberIds":[aid,f.humans[1].0]}))).await;
    let gid = group["id"].as_str().unwrap();
    let trigger = value(
        f.human(
            1,
            Method::POST,
            &format!("/chat/conversations/{gid}/messages"),
        )
        .json(&json!({"requestId":Uuid::new_v4(),"text":"群中提及","mentions":[aid]})),
    )
    .await;
    assert_eq!(trigger["aiResponses"].as_array().unwrap().len(), 1);
    let (task, start) = f.prepare(&secret).await;
    let tid = task["id"].as_str().unwrap();
    let receipt = value(
        f.node(&secret, Method::POST, &format!("/node/tasks/{tid}/start"))
            .json(&start),
    )
    .await;
    value(
        f.human(0, Method::PUT, &format!("/chat/contacts/{aid}/block"))
            .json(&json!({"requestId":Uuid::new_v4(),"expectedRevision":"3","blocked":false})),
    )
    .await;
    let mut node_headers = HeaderMap::new();
    node_headers.insert(
        header::AUTHORIZATION,
        format!("Bearer {secret}").parse().unwrap(),
    );
    delta(
        &f.state,
        &node_headers,
        connector::node::ClientFrame::Delta {
            sealed: None,
            task_id: tid.into(),
            run_id: receipt["runId"].as_str().unwrap().into(),
            message_id: receipt["messageId"].as_str().unwrap().into(),
            confirmation_id: receipt["confirmationId"].as_str().unwrap().into(),
            sequence: 1,
            text: "部分有效片段".into(),
        },
    )
    .await
    .unwrap();
    assert_eq!(f.human(1,Method::PUT,&format!("/ai/{aid}/status")).json(&json!({"requestId":Uuid::new_v4(),"expectedStatusRevision":"1","status":"disabled"})).send().await.unwrap().status(),403);
    let disabled = value(f.human(0, Method::PUT, &format!("/ai/{aid}/status")).json(
        &json!({"requestId":Uuid::new_v4(),"expectedStatusRevision":"1","status":"disabled"}),
    ))
    .await;
    assert_eq!(disabled["account"]["status"], "disabled");
    assert_eq!(disabled["bindingVersion"], "1");
    let stored_while_disabled = f.send(&cid, "停用期间仍可保存私聊原文").await;
    assert_eq!(stored_while_disabled["aiResponses"], json!([]));

    let page = value(f.human(
        0,
        Method::GET,
        &format!("/chat/conversations/{gid}/messages"),
    ))
    .await;
    assert!(
        page["messages"]
            .as_array()
            .unwrap()
            .iter()
            .any(|m| m["text"] == "部分有效片段" && m["status"] == "interrupted")
    );

    assert_eq!(
        f.node(&secret, Method::POST, &format!("/node/tasks/{tid}/result"))
            .json(&final_result(&receipt, "迟到结果"))
            .send()
            .await
            .unwrap()
            .status(),
        404
    );
    value(
        f.human(0, Method::PUT, &format!("/ai/{aid}/status")).json(
            &json!({"requestId":Uuid::new_v4(),"expectedStatusRevision":"2","status":"active"}),
        ),
    )
    .await;
    assert_eq!(
        value(f.node(&secret, Method::GET, "/node/tasks")).await,
        json!([])
    );
    assert_eq!(
        f.send(&cid, "恢复后的新触发").await["aiResponses"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    // Owner promotes a human admin; ordinary admins cannot promote others or remove owner.
    let promoted = value(
        f.human(
            0,
            Method::PUT,
            &format!("/chat/groups/{gid}/members/{}", f.humans[1].0),
        )
        .json(&json!({"requestId":Uuid::new_v4(),"expectedRevision":"1","role":"admin"})),
    )
    .await;
    assert_eq!(promoted["revision"], "2");
    assert_eq!(
        f.human(1, Method::PUT, &format!("/chat/groups/{gid}/members/{aid}"))
            .json(&json!({"requestId":Uuid::new_v4(),"expectedRevision":"2","role":"admin"}))
            .send()
            .await
            .unwrap()
            .status(),
        403
    );
    assert_eq!(
        f.human(
            1,
            Method::DELETE,
            &format!("/chat/groups/{gid}/members/{}", f.humans[0].0)
        )
        .json(&json!({"requestId":Uuid::new_v4(),"expectedRevision":"2"}))
        .send()
        .await
        .unwrap()
        .status(),
        403
    );
    assert_eq!(
        f.human(0, Method::POST, &format!("/chat/groups/{gid}/owner"))
            .json(&json!({"requestId":Uuid::new_v4(),"expectedRevision":"2","accountId":aid}))
            .send()
            .await
            .unwrap()
            .status(),
        400
    );
    let transfer=value(f.human(0,Method::POST,&format!("/chat/groups/{gid}/owner")).json(&json!({"requestId":Uuid::new_v4(),"expectedRevision":"2","accountId":f.humans[1].0}))).await;
    assert_eq!(transfer["revision"], "3");
    assert_eq!(
        f.human(1, Method::POST, &format!("/chat/groups/{gid}/leave"))
            .json(&json!({"requestId":Uuid::new_v4(),"expectedRevision":"3"}))
            .send()
            .await
            .unwrap()
            .status(),
        403
    );
    f.close().await;
}

#[tokio::test]
async fn groups_old_reply_required_membership_cancellation_and_concurrent_commands() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let ai = f.ai().await;
    let aid = ai["account"]["id"].as_str().unwrap();
    let (secret, _, _) = f.bind_ai(aid, "0").await;
    let group = value(f.human(0, Method::POST, "/chat/groups").json(
        &json!({"requestId":Uuid::new_v4(),"name":"资料与版本","memberIds":[aid,f.humans[1].0]}),
    ))
    .await;
    let cid = group["id"].as_str().unwrap();
    let old = Uuid::new_v4();
    let cid_uuid = cid.parse::<Uuid>().unwrap();
    sqlx::query("INSERT INTO cloud_messages(id,conversation,sender,sender_name,body) VALUES($1,$2,$3,'Test AI','old necessary reply')").bind(old).bind(cid_uuid).bind(aid.parse::<Uuid>().unwrap()).execute(&f.state.pool).await.unwrap();
    sqlx::query("INSERT INTO cloud_messages(id,conversation,sender,sender_name,body) SELECT md5($1::text || g::text)::uuid,$1,$2,'human0','history ' || g::text FROM generate_series(1,105) g").bind(cid_uuid).bind(f.humans[0].0).execute(&f.state.pool).await.unwrap();
    let trigger = value(
        f.human(
            0,
            Method::POST,
            &format!("/chat/conversations/{cid}/messages"),
        )
        .json(&json!({"requestId":Uuid::new_v4(),"text":"回复老AI消息","replyTo":old})),
    )
    .await;
    assert_eq!(trigger["aiResponses"].as_array().unwrap().len(), 1);
    let (task, start) = f.prepare(&secret).await;
    let tid = task["id"].as_str().unwrap();
    assert!(
        start["sources"]
            .as_array()
            .unwrap()
            .iter()
            .any(|s| s["messageId"] == old.to_string())
    );
    let receipt = value(
        f.node(&secret, Method::POST, &format!("/node/tasks/{tid}/start"))
            .json(&start),
    )
    .await;
    let command = json!({"requestId":Uuid::new_v4(),"expectedRevision":"1"});
    let path = format!("/chat/groups/{cid}/members/{}", f.humans[1].0);
    let (r1, r2) = tokio::join!(
        f.human(0, Method::DELETE, &path).json(&command).send(),
        f.human(0, Method::DELETE, &path).json(&command).send()
    );
    assert!(r1.unwrap().status().is_success());
    assert!(r2.unwrap().status().is_success());
    assert_eq!(
        value(f.node(&secret, Method::GET, &format!("/node/tasks/{tid}"))).await["status"],
        "cancelled"
    );
    assert_eq!(
        f.node(&secret, Method::POST, &format!("/node/tasks/{tid}/result"))
            .json(&final_result(&receipt, "成员变更后不可提交"))
            .send()
            .await
            .unwrap()
            .status(),
        409
    );
    let other_group =
        value(f.human(0, Method::POST, "/chat/groups").json(
            &json!({"requestId":Uuid::new_v4(),"name":"另一个群","memberIds":[f.humans[1].0]}),
        ))
        .await;
    assert_eq!(
        f.human(
            0,
            Method::DELETE,
            &format!(
                "/chat/groups/{}/members/{}",
                other_group["id"].as_str().unwrap(),
                f.humans[1].0
            )
        )
        .json(&command)
        .send()
        .await
        .unwrap()
        .status(),
        409
    );
    value(f.human(0,Method::POST,&format!("/chat/groups/{cid}/members")).json(&json!({"requestId":Uuid::new_v4(),"expectedRevision":"2","accountId":f.humans[1].0}))).await;
    assert_eq!(
        value(f.node(&secret, Method::GET, "/node/tasks")).await,
        json!([])
    );
    // Different commands race at one version: exactly one applies.
    let (a, b) = tokio::join!(
        f.human(0, Method::PUT, &format!("/chat/groups/{cid}"))
            .json(&json!({"requestId":Uuid::new_v4(),"expectedRevision":"3","name":"A"}))
            .send(),
        f.human(0, Method::PUT, &format!("/chat/groups/{cid}"))
            .json(&json!({"requestId":Uuid::new_v4(),"expectedRevision":"3","name":"B"}))
            .send()
    );
    let statuses = [a.unwrap().status().as_u16(), b.unwrap().status().as_u16()];
    assert!(statuses == [200, 409] || statuses == [409, 200]);
    f.close().await;
}

async fn connect_node(
    f: &Fixture,
    secret: &str,
    node: Uuid,
    ai: &str,
) -> connector::node::NodeConnection {
    let client = connector::node::NodeClient::new(&f.url, secret, None).unwrap();
    let mut ws = client.connect().await.unwrap();
    ws.send(&connector::node::ClientFrame::Hello {
        capabilities: vec![],
        protocol_version: 4,
        node_id: node.to_string(),
        ai_id: ai.into(),
        binding_version: "1".into(),
    })
    .await
    .unwrap();
    assert_eq!(
        ws.recv().await.unwrap(),
        Some(connector::node::ServerFrame::Ready {
            protocol_version: 4
        })
    );
    ws
}
async fn relay_request(ws: &mut connector::node::NodeConnection) -> Value {
    loop {
        let frame = tokio::time::timeout(Duration::from_secs(30), ws.recv())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        let frame = serde_json::to_value(frame).unwrap();
        if frame["type"] == "consciousnessRequest" {
            return frame;
        }
    }
}
async fn relay_response(
    ws: &mut connector::node::NodeConnection,
    request: &Value,
) -> std::result::Result<(), connector::node::NodeError> {
    let frame=serde_json::from_value(json!({"type":"consciousnessResponse","relayId":request["relayId"],"requestId":request["requestId"],"view":{"memories":[],"summaries":[],"settings":null,"models":[]},"error":null})).unwrap();
    ws.send(&frame).await
}
#[tokio::test]
async fn consciousness_relay_offline_members_admins_connection_fencing_and_response_reauth() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let ai = f.ai().await;
    let aid = ai["account"]["id"].as_str().unwrap();
    let (secret, node, _) = f.bind_ai(aid, "0").await;
    let cid = f.conversation(aid).await;
    let path = format!("/chat/conversations/{cid}/ai/{aid}/consciousness");
    let command = json!({"requestId":Uuid::new_v4(),"operation":{"type":"memories"}});
    let offline = f
        .human(0, Method::POST, &path)
        .json(&command)
        .send()
        .await
        .unwrap();
    assert_eq!(offline.status(), 409);
    assert_eq!(
        offline.json::<Value>().await.unwrap()["code"],
        "node_offline"
    );
    assert_eq!(
        f.human(1, Method::POST, &path)
            .json(&command)
            .send()
            .await
            .unwrap()
            .status(),
        403
    );
    let mut ws = connect_node(&f, &secret, node, aid).await;
    let request = f.human(0, Method::POST, &path).json(&command);
    let response = tokio::spawn(async move { request.send().await.unwrap() });
    let old = relay_request(&mut ws).await;
    assert_eq!(old["operatorId"], f.humans[0].0.to_string());
    let mut replacement = connect_node(&f, &secret, node, aid).await;
    let disconnected = response.await.unwrap();
    assert_eq!(disconnected.status(), 409);
    assert_eq!(
        disconnected.json::<Value>().await.unwrap()["code"],
        "node_disconnected"
    );
    let request = f.human(0, Method::POST, &path).json(&command);
    let response = tokio::spawn(async move { request.send().await.unwrap() });
    let fresh = relay_request(&mut replacement).await;
    assert_ne!(old["relayId"], fresh["relayId"]);
    let _ = relay_response(&mut ws, &old).await;
    assert!(!response.is_finished());
    relay_response(&mut replacement, &fresh).await.unwrap();
    assert!(response.await.unwrap().status().is_success());
    let rid = command["requestId"]
        .as_str()
        .unwrap()
        .parse::<Uuid>()
        .unwrap();
    let persisted: i64 =
        sqlx::query_scalar("SELECT count(*) FROM cloud_ai_receipts WHERE request=$1")
            .bind(rid)
            .fetch_one(&f.state.pool)
            .await
            .unwrap();
    assert_eq!(persisted, 0);
    let group = value(f.human(0, Method::POST, "/chat/groups").json(
        &json!({"requestId":Uuid::new_v4(),"name":"relay权限","memberIds":[aid,f.humans[1].0]}),
    ))
    .await;
    let gid = group["id"].as_str().unwrap();
    let path = format!("/chat/conversations/{gid}/ai/{aid}/consciousness");
    assert_eq!(
        f.human(1, Method::POST, &path)
            .json(&json!({"requestId":Uuid::new_v4(),"operation":{"type":"summaries"}}))
            .send()
            .await
            .unwrap()
            .status(),
        403
    );
    let request = f
        .human(1, Method::POST, &path)
        .json(&json!({"requestId":Uuid::new_v4(),"operation":{"type":"memories"}}));
    let response = tokio::spawn(async move { request.send().await.unwrap() });
    let pending = relay_request(&mut replacement).await;
    value(
        f.human(
            0,
            Method::DELETE,
            &format!("/chat/groups/{gid}/members/{}", f.humans[1].0),
        )
        .json(&json!({"requestId":Uuid::new_v4(),"expectedRevision":"1"})),
    )
    .await;
    relay_response(&mut replacement, &pending).await.unwrap();
    assert_eq!(response.await.unwrap().status(), 403);
    let _ = replacement.close().await;
    f.close().await;
}
#[tokio::test]
async fn references_preview_start_epoch_source_and_membership_staleness() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let ai = f.ai().await;
    let aid = ai["account"]["id"].as_str().unwrap();
    let (secret, _, _) = f.bind_ai(aid, "0").await;
    let group = value(f.human(0, Method::POST, "/chat/groups").json(
        &json!({"requestId":Uuid::new_v4(),"name":"引用来源","memberIds":[aid,f.humans[1].0]}),
    ))
    .await;
    let gid = group["id"].as_str().unwrap();
    let source = f.send(gid, "群原文").await;
    let mid = source["id"].as_str().unwrap();
    let cid = f.conversation(aid).await;
    let reference = json!({"conversationId":gid,"sources":[{"messageId":mid,"revision":"1"}]});
    let preview = format!("/chat/conversations/{cid}/reference-preview");
    let shown = value(
        f.human(0, Method::POST, &preview)
            .json(&json!({"reference":reference})),
    )
    .await;
    assert_eq!(shown["messages"][0]["text"], "群原文");
    assert!(shown["messages"][0]["position"].is_string());
    assert_eq!(
        f.human(1, Method::POST, &preview)
            .json(&json!({"reference":reference}))
            .send()
            .await
            .unwrap()
            .status(),
        404
    );
    let trigger = value(
        f.human(
            0,
            Method::POST,
            &format!("/chat/conversations/{cid}/messages"),
        )
        .json(&json!({"requestId":Uuid::new_v4(),"text":"延续群话题","reference":reference})),
    )
    .await;
    assert_eq!(trigger["contextReference"]["stale"], false);
    for format in ["json", "markdown"] {
        let exported = f
            .human(
                0,
                Method::GET,
                &format!("/chat/conversations/{cid}/export?format={format}"),
            )
            .send()
            .await
            .unwrap();
        assert!(exported.status().is_success());
        let text = exported.text().await.unwrap();
        assert!(text.contains(mid));
        assert!(text.contains(gid));
        assert!(!text.contains("群原文"));
    }

    let (task, start) = f.prepare(&secret).await;
    let tid = task["id"].as_str().unwrap();
    let materials = value(f.node(
        &secret,
        Method::GET,
        &format!("/node/tasks/{tid}/materials"),
    ))
    .await;
    assert_eq!(materials["reference"]["required"], true);
    assert_eq!(materials["reference"]["messages"][0]["id"], mid);
    let mut omitted = start.clone();
    omitted["derivedSources"] = json!([]);
    assert_eq!(
        f.node(&secret, Method::POST, &format!("/node/tasks/{tid}/start"))
            .json(&omitted)
            .send()
            .await
            .unwrap()
            .status(),
        400
    );
    let receipt = value(
        f.node(&secret, Method::POST, &format!("/node/tasks/{tid}/start"))
            .json(&start),
    )
    .await;
    value(
        f.node(&secret, Method::POST, "/node/consciousness/state")
            .json(&json!({"conversationId":cid,"revision":Uuid::new_v4()})),
    )
    .await;
    assert_eq!(
        f.node(&secret, Method::POST, &format!("/node/tasks/{tid}/result"))
            .json(&final_result(&receipt, "旧材料结果"))
            .send()
            .await
            .unwrap()
            .status(),
        409
    );
    value(
        f.human(0, Method::PATCH, &format!("/chat/messages/{mid}"))
            .json(&json!({"requestId":Uuid::new_v4(),"expectedRevision":"1","text":"修订群原文"})),
    )
    .await;
    let changes = value(f.node(&secret, Method::GET, "/node/consciousness/changes?after=0")).await;
    assert!(
        changes["changes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["messageId"] == mid)
    );
    assert!(
        changes["changes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["messageId"] == trigger["id"])
    );
    assert!(!changes.to_string().contains("修订群原文"));
    let page = value(f.human(
        0,
        Method::GET,
        &format!("/chat/conversations/{cid}/messages"),
    ))
    .await;
    assert_eq!(page["messages"][0]["contextReference"]["stale"], true);
    assert_eq!(
        f.human(0, Method::POST, &preview)
            .json(&json!({"reference":reference}))
            .send()
            .await
            .unwrap()
            .status(),
        409
    );
    let fresh = json!({"conversationId":gid,"sources":[{"messageId":mid,"revision":"2"}]});
    let next = value(
        f.human(
            0,
            Method::POST,
            &format!("/chat/conversations/{cid}/messages"),
        )
        .json(&json!({"requestId":Uuid::new_v4(),"text":"新引用","reference":fresh})),
    )
    .await;
    // An unrelated human leaving does not destroy valid reference metadata.
    value(
        f.human(1, Method::POST, &format!("/chat/groups/{gid}/leave"))
            .json(&json!({"requestId":Uuid::new_v4(),"expectedRevision":"1"})),
    )
    .await;
    let reference: Value =
        sqlx::query_scalar("SELECT context_reference FROM cloud_messages WHERE id=$1")
            .bind(next["id"].as_str().unwrap().parse::<Uuid>().unwrap())
            .fetch_one(&f.state.pool)
            .await
            .unwrap();
    assert_eq!(reference["stale"], false);
    value(
        f.human(
            0,
            Method::DELETE,
            &format!("/chat/groups/{gid}/members/{aid}"),
        )
        .json(&json!({"requestId":Uuid::new_v4(),"expectedRevision":"2"})),
    )
    .await;
    let denied=value(f.node(&secret,Method::POST,"/node/consciousness/validate").json(&json!({"sources":[{"conversationId":gid,"messageId":mid,"revision":"2"}],"conversations":[]}))).await;
    assert_eq!(denied["valid"], false);
    let changes = value(f.node(&secret, Method::GET, "/node/consciousness/changes?after=0")).await;
    assert!(
        changes["changes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["reason"] == "permission_revoked" && c["messageId"].is_null())
    );
    assert!(
        changes["changes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["reason"] == "window_changed")
    );
    value(
        f.human(0, Method::POST, &format!("/chat/groups/{gid}/members"))
            .json(&json!({"requestId":Uuid::new_v4(),"expectedRevision":"3","accountId":aid})),
    )
    .await;
    let reference: Value =
        sqlx::query_scalar("SELECT context_reference FROM cloud_messages WHERE id=$1")
            .bind(next["id"].as_str().unwrap().parse::<Uuid>().unwrap())
            .fetch_one(&f.state.pool)
            .await
            .unwrap();
    assert_eq!(reference["stale"], true);
    f.close().await;
}
#[tokio::test]
async fn summary_confirmations_metadata_only_idempotence_and_late_ready_fenced() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let ai = f.ai().await;
    let aid = ai["account"]["id"].as_str().unwrap();
    let (secret, _, _) = f.bind_ai(aid, "0").await;
    let group = value(
        f.human(0, Method::POST, "/chat/groups")
            .json(&json!({"requestId":Uuid::new_v4(),"name":"独立摘要","memberIds":[aid]})),
    )
    .await;
    let cid = group["id"].as_str().unwrap();
    let source = f.send(cid, "只有云端原文，不是摘要正文").await;
    let scope = f.scope(&secret, cid).await;
    let start = json!({"requestId":Uuid::new_v4(),"conversationId":cid,"conversationRevision":"1","sources":[{"messageId":source["id"],"revision":"1"}],"settingsRevision":"settings-1","materialScope":scope});
    let receipt = value(
        f.node(&secret, Method::POST, "/node/summaries/start")
            .json(&start),
    )
    .await;
    assert_eq!(
        receipt,
        value(
            f.node(&secret, Method::POST, "/node/summaries/start")
                .json(&start)
        )
        .await
    );
    let mut different = start.clone();
    different["settingsRevision"] = json!("settings-2");
    assert_eq!(
        f.node(&secret, Method::POST, "/node/summaries/start")
            .json(&different)
            .send()
            .await
            .unwrap()
            .status(),
        409
    );
    let finish = json!({"confirmationId":receipt["confirmationId"],"status":"ready","usage":{"inputTokens":12,"outputTokens":4,"cachedTokens":0},"errorCode":null,"text":"NODE-PRIVATE-SUMMARY-BODY"});
    let path = format!("/node/summaries/{}/finish", receipt["id"].as_str().unwrap());
    value(f.node(&secret, Method::POST, &path).json(&finish)).await;
    value(f.node(&secret, Method::POST, &path).json(&finish)).await;
    let row: Value =
        sqlx::query_scalar("SELECT to_jsonb(c) FROM cloud_summary_calls c WHERE id=$1")
            .bind(receipt["id"].as_str().unwrap().parse::<Uuid>().unwrap())
            .fetch_one(&f.state.pool)
            .await
            .unwrap();
    assert!(!row.to_string().contains("NODE-PRIVATE"));
    assert!(row.get("text").is_none());
    assert_eq!(row["state"], "ready");
    assert_eq!(
        f.node(&secret, Method::POST, "/node/summaries/start")
            .json(&start)
            .send()
            .await
            .unwrap()
            .status(),
        409
    );
    let messages: i64 =
        sqlx::query_scalar("SELECT count(*) FROM cloud_messages WHERE conversation=$1")
            .bind(cid.parse::<Uuid>().unwrap())
            .fetch_one(&f.state.pool)
            .await
            .unwrap();
    assert_eq!(messages, 1);
    let tasks: i64 = sqlx::query_scalar("SELECT count(*) FROM cloud_node_tasks")
        .fetch_one(&f.state.pool)
        .await
        .unwrap();
    assert_eq!(tasks, 0);
    let mut second = start.clone();
    second["requestId"] = json!(Uuid::new_v4());
    let other = value(
        f.node(&secret, Method::POST, "/node/summaries/start")
            .json(&second),
    )
    .await;
    value(
        f.human(
            0,
            Method::PATCH,
            &format!("/chat/messages/{}", source["id"].as_str().unwrap()),
        )
        .json(&json!({"requestId":Uuid::new_v4(),"expectedRevision":"1","text":"来源修订"})),
    )
    .await;
    assert_eq!(
        f.node(&secret, Method::POST, &path)
            .json(&finish)
            .send()
            .await
            .unwrap()
            .status(),
        409
    );
    let path = format!("/node/summaries/{}/finish", other["id"].as_str().unwrap());
    assert_eq!(f.node(&secret,Method::POST,&path).json(&json!({"confirmationId":other["confirmationId"],"status":"ready","usage":null,"errorCode":null})).send().await.unwrap().status(),409);
    f.close().await;
}
#[tokio::test]
async fn consciousness_history_full_pagination_and_stable_change_cursor() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let ai = f.ai().await;
    let aid = ai["account"]["id"].as_str().unwrap();
    let (secret, _, _) = f.bind_ai(aid, "0").await;
    let group = value(
        f.human(0, Method::POST, "/chat/groups")
            .json(&json!({"requestId":Uuid::new_v4(),"name":"完整来源分页","memberIds":[aid]})),
    )
    .await;
    let cid = group["id"].as_str().unwrap();
    let cid_uuid = cid.parse::<Uuid>().unwrap();
    sqlx::query("INSERT INTO cloud_messages(id,conversation,sender,sender_name,body) SELECT md5($1::text||g::text)::uuid,$1,$2,'human0','history '||g::text FROM generate_series(1,105) g").bind(cid_uuid).bind(f.humans[0].0).execute(&f.state.pool).await.unwrap();
    let first = value(f.node(
        &secret,
        Method::GET,
        &format!("/node/conversations/{cid}/history?after=0&limit=100"),
    ))
    .await;
    assert_eq!(first["messages"].as_array().unwrap().len(), 100);
    let last = first["messages"][99]["position"].as_str().unwrap();
    let next = value(f.node(
        &secret,
        Method::GET,
        &format!("/node/conversations/{cid}/history?after={last}&limit=100"),
    ))
    .await;
    assert_eq!(next["messages"].as_array().unwrap().len(), 5);
    assert_eq!(first["recentBoundary"], "94");
    sqlx::query("INSERT INTO cloud_source_changes(ai_id,conversation,reason) SELECT $1,$2,'window_changed' FROM generate_series(1,201)").bind(aid.parse::<Uuid>().unwrap()).bind(cid_uuid).execute(&f.state.pool).await.unwrap();
    let first = value(f.node(&secret, Method::GET, "/node/consciousness/changes?after=0")).await;
    assert_eq!(first["changes"].as_array().unwrap().len(), 200);
    assert_eq!(first["hasMore"], true);
    let cursor = first["cursor"].as_str().unwrap();
    let next = value(f.node(
        &secret,
        Method::GET,
        &format!("/node/consciousness/changes?after={cursor}"),
    ))
    .await;
    assert_eq!(next["changes"].as_array().unwrap().len(), 1);
    assert_eq!(next["hasMore"], false);
    let (new_secret, _, _) = f.bind_ai(aid, "1").await;
    let replay = value(f.node(
        &new_secret,
        Method::GET,
        "/node/consciousness/changes?after=0",
    ))
    .await;
    assert_eq!(replay["cursor"], first["cursor"]);
    f.close().await;
}

async fn participation_view(f: &Fixture, cid: &str) -> Value {
    value(f.human(
        0,
        Method::GET,
        &format!("/chat/conversations/{cid}/participation"),
    ))
    .await
}
async fn enable_contacts(f: &Fixture, cid: &str) {
    value(f.human(0,Method::PUT,&format!("/chat/conversations/{cid}/contact-settings")).json(&json!({"requestId":Uuid::new_v4(),"expectedRevision":"1","enabled":true,"quietUntil":null,"quietAction":"defer"}))).await;
}
async fn contact(f: &Fixture, cid: &str, aid: &str, seconds: i64) -> Value {
    value(f.human(0,Method::POST,&format!("/chat/conversations/{cid}/contact-intents")).json(&json!({"requestId":Uuid::new_v4(),"aiId":aid,"content":"授权的一次性联系","dueAt":chrono::Utc::now()+chrono::Duration::seconds(seconds),"expiresAt":chrono::Utc::now()+chrono::Duration::hours(2),"utcOffsetMinutes":480}))).await
}
async fn ready_now(f: &Fixture) {
    sqlx::query("UPDATE cloud_node_tasks SET not_before=now() WHERE state='queued'")
        .execute(&f.state.pool)
        .await
        .unwrap();
}
#[tokio::test]
async fn cloud_direct_merge_twenty_required_sources_and_source_invalidation() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let ai = f.ai().await;
    let aid = ai["account"]["id"].as_str().unwrap();
    let (secret, _, _) = f.bind_ai(aid, "0").await;
    let cid = f.conversation(aid).await;
    sqlx::query("UPDATE cloud_participation SET policy=jsonb_set(policy,'{directMergeMs}','2000') WHERE conversation=$1").bind(cid.parse::<Uuid>().unwrap()).execute(&f.state.pool).await.unwrap();
    let first = f.send(&cid, "first").await;
    for i in 1..21 {
        // Freeze the unstarted deadline; database tunnel latency is outside the merge behavior under test.
        sqlx::query("UPDATE cloud_node_tasks SET not_before=now()+interval '1 hour' WHERE conversation=$1 AND state='queued'").bind(cid.parse::<Uuid>().unwrap()).execute(&f.state.pool).await.unwrap();
        f.send(&cid, &format!("input{i}")).await;
    }
    let rows=sqlx::query("SELECT cardinality(input_ids) AS n FROM cloud_node_tasks WHERE conversation=$1 ORDER BY queue_position").bind(cid.parse::<Uuid>().unwrap()).fetch_all(&f.state.pool).await.unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].get::<i32, _>("n"), 20);
    assert_eq!(rows[1].get::<i32, _>("n"), 1);
    ready_now(&f).await;
    let (task, request) = f.prepare(&secret).await;
    assert_eq!(task["trigger"]["inputIds"].as_array().unwrap().len(), 20);
    assert!(request["sources"].as_array().unwrap().len() >= 20);
    let mid = first["id"].as_str().unwrap().parse().unwrap();
    let mut tx = f.state.pool.begin().await.unwrap();
    lock(&mut tx).await.unwrap();
    source_changed(&mut tx, mid).await.unwrap();
    tx.commit().await.unwrap();
    let states: Vec<String> = sqlx::query_scalar(
        "SELECT state FROM cloud_node_tasks WHERE conversation=$1 ORDER BY queue_position",
    )
    .bind(cid.parse::<Uuid>().unwrap())
    .fetch_all(&f.state.pool)
    .await
    .unwrap();
    assert_eq!(states, vec!["cancelled", "queued"]);
    f.close().await;
}
#[tokio::test]
async fn cloud_contacts_independent_trigger_idempotency_restart_and_no_fake_human() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let ai = f.ai().await;
    let aid = ai["account"]["id"].as_str().unwrap();
    let (secret, _, _) = f.bind_ai(aid, "0").await;
    let group = value(
        f.human(0, Method::POST, "/chat/groups")
            .json(&json!({"requestId":Uuid::new_v4(),"name":"约定恢复边界","memberIds":[aid]})),
    )
    .await;
    let cid = group["id"].as_str().unwrap().to_owned();
    enable_contacts(&f, &cid).await;
    let v = json!({"requestId":Uuid::new_v4(),"aiId":aid,"content":"独立授权正文","dueAt":chrono::Utc::now()+chrono::Duration::seconds(120),"expiresAt":chrono::Utc::now()+chrono::Duration::hours(2),"utcOffsetMinutes":480});
    let path = format!("/chat/conversations/{cid}/contact-intents");
    let i = value(f.human(0, Method::POST, &path).json(&v)).await;
    assert_eq!(i, value(f.human(0, Method::POST, &path).json(&v)).await);
    crate::participation::tick(
        &f.state.pool,
        chrono::Utc::now() + chrono::Duration::seconds(121),
    )
    .await
    .unwrap();
    let (task, request) = f.prepare(&secret).await;
    assert_eq!(task["trigger"]["type"], "contact");
    assert!(task["automatic"].as_bool().unwrap());
    assert!(request["sources"].as_array().unwrap().is_empty());
    let tid = task["id"].as_str().unwrap();
    f.send(&cid, "准备和开始之间的新原文").await;
    let stale = f
        .node(&secret, Method::POST, &format!("/node/tasks/{tid}/start"))
        .json(&request)
        .send()
        .await
        .unwrap();
    assert_eq!(stale.status(), StatusCode::CONFLICT);
    assert_eq!(
        stale.json::<Value>().await.unwrap()["code"],
        "task_not_ready"
    );
    assert_eq!(participation_view(&f, &cid).await["state"]["dailyCalls"], 0);
    let request = f.start_request(&secret, &task).await;
    let receipt = value(
        f.node(&secret, Method::POST, &format!("/node/tasks/{tid}/start"))
            .json(&request),
    )
    .await;
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM cloud_messages WHERE conversation=$1")
            .bind(cid.parse::<Uuid>().unwrap())
            .fetch_one(&f.state.pool)
            .await
            .unwrap();
    assert_eq!(
        count, 1,
        "only the real human message exists; contact never fabricates one"
    );
    recover(&f.state.pool).await.unwrap();
    let view = participation_view(&f, &cid).await;
    assert_eq!(view["intents"][0]["status"], "uncertain");
    assert_eq!(view["state"]["dailyCalls"], 1);
    assert!(
        value(f.node(&secret, Method::GET, "/node/tasks"))
            .await
            .as_array()
            .unwrap()
            .is_empty()
    );
    value(
        f.node(&secret, Method::POST, &format!("/node/tasks/{tid}/result"))
            .json(&final_result(&receipt, "重启前已保存的完成结果")),
    )
    .await;
    let after = participation_view(&f, &cid).await;
    assert_eq!(after["intents"][0]["status"], "delivered");
    assert_eq!(after["state"]["dailyCalls"], 1);
    let second = contact(&f, &cid, aid, 120).await;
    crate::participation::tick(
        &f.state.pool,
        chrono::Utc::now() + chrono::Duration::seconds(121),
    )
    .await
    .unwrap();
    let (task, request) = f.prepare(&secret).await;
    let tid = task["id"].as_str().unwrap();
    let saved = value(
        f.node(&secret, Method::POST, &format!("/node/tasks/{tid}/start"))
            .json(&request),
    )
    .await;
    recover(&f.state.pool).await.unwrap();
    for blocked in [true, false] {
        let c = value(f.human(0, Method::GET, &format!("/chat/contacts/{aid}"))).await;
        value(f.human(0,Method::PUT,&format!("/chat/contacts/{aid}/block")).json(&json!({"requestId":Uuid::new_v4(),"expectedRevision":c["revision"],"blocked":blocked}))).await;
    }
    let rejected = f
        .node(&secret, Method::POST, &format!("/node/tasks/{tid}/result"))
        .json(&final_result(&saved, "已撤销授权的迟到结果"))
        .send()
        .await
        .unwrap();
    assert_eq!(rejected.status(), StatusCode::CONFLICT);
    let status: String = sqlx::query_scalar("SELECT status FROM cloud_contact_intents WHERE id=$1")
        .bind(second["id"].as_str().unwrap().parse::<Uuid>().unwrap())
        .fetch_one(&f.state.pool)
        .await
        .unwrap();
    assert_eq!(
        status, "cancelled",
        "unblocking never restores the old uncertain authorization"
    );
    f.close().await;
}
#[tokio::test]
async fn cloud_end_retains_future_contacts_pause_skips_due_and_dnd_defers() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let ai = f.ai().await;
    let aid = ai["account"]["id"].as_str().unwrap();
    let (_secret, _, _) = f.bind_ai(aid, "0").await;
    let cid = f.conversation(aid).await;
    enable_contacts(&f, &cid).await;
    let future = contact(&f, &cid, aid, 3600).await;
    let due = contact(&f, &cid, aid, 120).await;
    let already_due = contact(&f, &cid, aid, 120).await;
    sqlx::query("UPDATE cloud_contact_intents SET due_at=now()-interval '1 second' WHERE id=$1")
        .bind(already_due["id"].as_str().unwrap().parse::<Uuid>().unwrap())
        .execute(&f.state.pool)
        .await
        .unwrap();
    let control = format!("/chat/conversations/{cid}/participation/control");
    value(
        f.human(0, Method::POST, &control)
            .json(&json!({"requestId":Uuid::new_v4(),"expectedRevision":"1","action":"end"})),
    )
    .await;
    value(
        f.human(0, Method::POST, &control)
            .json(&json!({"requestId":Uuid::new_v4(),"expectedRevision":"2","action":"pause"})),
    )
    .await;
    crate::participation::tick(
        &f.state.pool,
        chrono::Utc::now() + chrono::Duration::seconds(121),
    )
    .await
    .unwrap();
    let view = participation_view(&f, &cid).await;
    assert_eq!(
        view["intents"]
            .as_array()
            .unwrap()
            .iter()
            .find(|i| i["id"] == already_due["id"])
            .unwrap()["status"],
        "cancelled"
    );
    assert_eq!(
        view["intents"]
            .as_array()
            .unwrap()
            .iter()
            .find(|i| i["id"] == future["id"])
            .unwrap()["status"],
        "pending"
    );
    assert_eq!(
        view["intents"]
            .as_array()
            .unwrap()
            .iter()
            .find(|i| i["id"] == due["id"])
            .unwrap()["status"],
        "skipped"
    );
    value(
        f.human(0, Method::POST, &control)
            .json(&json!({"requestId":Uuid::new_v4(),"expectedRevision":"3","action":"resume"})),
    )
    .await;
    let dnd = contact(&f, &cid, aid, 120).await;
    value(f.human(0,Method::PUT,&format!("/chat/conversations/{cid}/contact-settings")).json(&json!({"requestId":Uuid::new_v4(),"expectedRevision":"2","enabled":true,"quietUntil":chrono::Utc::now()+chrono::Duration::hours(1),"quietAction":"defer"}))).await;
    crate::participation::tick(
        &f.state.pool,
        chrono::Utc::now() + chrono::Duration::seconds(121),
    )
    .await
    .unwrap();
    let status: String = sqlx::query_scalar("SELECT status FROM cloud_contact_intents WHERE id=$1")
        .bind(dnd["id"].as_str().unwrap().parse::<Uuid>().unwrap())
        .fetch_one(&f.state.pool)
        .await
        .unwrap();
    assert_eq!(status, "pending");
    f.send(&cid, "普通消息照常排队").await;
    let count:i64=sqlx::query_scalar("SELECT count(*) FROM cloud_node_tasks WHERE conversation=$1 AND state='queued' AND contact_intent_id IS NULL").bind(cid.parse::<Uuid>().unwrap()).fetch_one(&f.state.pool).await.unwrap();
    assert_eq!(count, 1);
    value(f.human(0,Method::PUT,&format!("/chat/conversations/{cid}/contact-settings")).json(&json!({"requestId":Uuid::new_v4(),"expectedRevision":"3","enabled":true,"quietUntil":null,"quietAction":"defer"}))).await;
    crate::participation::tick(
        &f.state.pool,
        chrono::Utc::now() + chrono::Duration::seconds(121),
    )
    .await
    .unwrap();
    f.send(&cid, "新消息保留独立已领取约定").await;
    let status: String = sqlx::query_scalar("SELECT status FROM cloud_contact_intents WHERE id=$1")
        .bind(dnd["id"].as_str().unwrap().parse::<Uuid>().unwrap())
        .fetch_one(&f.state.pool)
        .await
        .unwrap();
    assert_eq!(status, "claimed");
    value(f.human(0,Method::PUT,&format!("/chat/conversations/{cid}/contact-settings")).json(&json!({"requestId":Uuid::new_v4(),"expectedRevision":"4","enabled":true,"quietUntil":chrono::Utc::now()+chrono::Duration::hours(1),"quietAction":"defer"}))).await;
    sqlx::query("INSERT INTO cloud_participation_daily(conversation,day,calls) SELECT conversation,budget_day,48 FROM cloud_participation WHERE conversation=$1 ON CONFLICT(conversation,day) DO UPDATE SET calls=48").bind(cid.parse::<Uuid>().unwrap()).execute(&f.state.pool).await.unwrap();
    crate::participation::tick(&f.state.pool, chrono::Utc::now())
        .await
        .unwrap();
    let status: String = sqlx::query_scalar("SELECT status FROM cloud_contact_intents WHERE id=$1")
        .bind(dnd["id"].as_str().unwrap().parse::<Uuid>().unwrap())
        .fetch_one(&f.state.pool)
        .await
        .unwrap();
    assert_eq!(
        status, "claimed",
        "DND-deferred contacts evaluate quota when eligible, allowing a new UTC day"
    );
    let expiring = contact(&f, &cid, aid, 120).await;
    sqlx::query(
        "UPDATE cloud_participation SET quiet_until=now()+interval '3 hours' WHERE conversation=$1",
    )
    .bind(cid.parse::<Uuid>().unwrap())
    .execute(&f.state.pool)
    .await
    .unwrap();
    crate::participation::tick(
        &f.state.pool,
        chrono::Utc::now() + chrono::Duration::hours(2) + chrono::Duration::minutes(1),
    )
    .await
    .unwrap();
    let status: String = sqlx::query_scalar("SELECT status FROM cloud_contact_intents WHERE id=$1")
        .bind(expiring["id"].as_str().unwrap().parse::<Uuid>().unwrap())
        .fetch_one(&f.state.pool)
        .await
        .unwrap();
    assert_eq!(status, "expired", "expiry precedes quiet-hours deferral");
    f.close().await;
}
#[tokio::test]
async fn cloud_group_auto_rotation_quota_explicit_bypass_and_human_input_during_run() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let a = f.ai().await;
    let aid = a["account"]["id"].as_str().unwrap();
    let (s1, _, _) = f.bind_ai(aid, "0").await;
    let b = f.ai().await;
    let bid = b["account"]["id"].as_str().unwrap();
    let (s2, _, _) = f.bind_ai(bid, "0").await;
    let c = value(f.human(0, Method::POST, "/chat/groups").json(
        &json!({"requestId":Uuid::new_v4(),"name":"自动轮转","memberIds":[aid,bid,f.humans[1].0]}),
    ))
    .await;
    let cid = c["id"].as_str().unwrap();
    let p = json!({"mode":"limitedAuto","directMergeMs":800,"maxConsecutiveAi":3,"maxBoutCalls":12,"maxDailyCalls":3,"idleSeconds":300});
    value(
        f.human(
            0,
            Method::PUT,
            &format!("/chat/conversations/{cid}/participation"),
        )
        .json(&json!({"requestId":Uuid::new_v4(),"expectedRevision":"1","policy":p})),
    )
    .await;
    let member_view = value(f.human(
        1,
        Method::GET,
        &format!("/chat/conversations/{cid}/participation"),
    ))
    .await;
    assert_eq!(member_view["canManage"], false);
    let denied = f
        .human(
            1,
            Method::POST,
            &format!("/chat/conversations/{cid}/participation/control"),
        )
        .json(&json!({"requestId":Uuid::new_v4(),"expectedRevision":"2","action":"pause"}))
        .send()
        .await
        .unwrap();
    assert_eq!(denied.status(), StatusCode::FORBIDDEN);
    f.send(cid, "开始讨论").await;
    let mut actors = vec![];
    for n in 0..3 {
        ready_now(&f).await;
        let t1 = value(f.node(&s1, Method::GET, "/node/tasks")).await;
        let (secret, task) = if t1.as_array().unwrap().is_empty() {
            (
                &s2,
                value(f.node(&s2, Method::GET, "/node/tasks")).await[0].clone(),
            )
        } else {
            (&s1, t1[0].clone())
        };
        actors.push(task["aiId"].clone());
        let req = f.start_request(secret, &task).await;
        let tid = task["id"].as_str().unwrap();
        let start = value(
            f.node(secret, Method::POST, &format!("/node/tasks/{tid}/start"))
                .json(&req),
        )
        .await;
        if n == 0 {
            let human = f.send(cid, "生成期间的新输入").await;
            let input: Uuid = sqlx::query_scalar(
                "SELECT input_id FROM cloud_node_tasks WHERE conversation=$1 AND state='queued'",
            )
            .bind(cid.parse::<Uuid>().unwrap())
            .fetch_one(&f.state.pool)
            .await
            .unwrap();
            assert_eq!(input.to_string(), human["id"].as_str().unwrap());
        }
        value(
            f.node(secret, Method::POST, &format!("/node/tasks/{tid}/result"))
                .json(&final_result(&start, "完整回答")),
        )
        .await;
    }
    assert_ne!(actors[1], actors[2]);
    let v = participation_view(&f, cid).await;
    assert_eq!(v["state"]["dailyCalls"], 3);
    let m = value(
        f.human(
            0,
            Method::POST,
            &format!("/chat/conversations/{cid}/messages"),
        )
        .json(&json!({"requestId":Uuid::new_v4(),"text":"明确点名","mentions":[aid]})),
    )
    .await;
    let (task, request) = f.prepare(&s1).await;
    assert_eq!(task["automatic"], false);
    assert_eq!(task["trigger"]["messageId"], m["id"]);
    let saved_final = value(
        f.node(
            &s1,
            Method::POST,
            &format!("/node/tasks/{}/start", task["id"].as_str().unwrap()),
        )
        .json(&request),
    )
    .await;
    assert_eq!(participation_view(&f, cid).await["state"]["dailyCalls"], 3);
    recover(&f.state.pool).await.unwrap();
    sqlx::query("UPDATE cloud_participation SET bout_id=NULL,reason='idle',policy=jsonb_set(policy,'{maxDailyCalls}','200') WHERE conversation=$1").bind(cid.parse::<Uuid>().unwrap()).execute(&f.state.pool).await.unwrap();
    value(
        f.node(
            &s1,
            Method::POST,
            &format!("/node/tasks/{}/result", task["id"].as_str().unwrap()),
        )
        .json(&final_result(&saved_final, "空闲结束后已保存结果的确认")),
    )
    .await;
    let queued: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM cloud_node_tasks WHERE conversation=$1 AND state='queued'",
    )
    .bind(cid.parse::<Uuid>().unwrap())
    .fetch_one(&f.state.pool)
    .await
    .unwrap();
    assert_eq!(queued, 0, "late known final does not reopen an idle bout");
    let tomorrow = chrono::Utc::now() + chrono::Duration::days(1);
    crate::participation::tick(&f.state.pool, tomorrow)
        .await
        .unwrap();
    assert_eq!(participation_view(&f, cid).await["state"]["dailyCalls"], 0);
    crate::participation::tick(&f.state.pool, chrono::Utc::now())
        .await
        .unwrap();
    assert_eq!(participation_view(&f, cid).await["state"]["dailyCalls"], 0);
    let day: chrono::NaiveDate =
        sqlx::query_scalar("SELECT budget_day FROM cloud_participation WHERE conversation=$1")
            .bind(cid.parse::<Uuid>().unwrap())
            .fetch_one(&f.state.pool)
            .await
            .unwrap();
    assert_eq!(day, tomorrow.date_naive());
    f.close().await;
}

#[tokio::test]
async fn managed_profile_and_administrators_use_current_permissions_and_replay() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let ai = f.ai().await;
    let aid = ai["account"]["id"].as_str().unwrap();
    let admin_path = format!("/ai/{aid}/administrators/{}", f.humans[1].0);
    let grant = json!({"requestId":Uuid::new_v4(),"expectedRevision":"1","enabled":true});
    let admins = value(f.human(0, Method::PUT, &admin_path).json(&grant)).await;
    assert_eq!(admins["revision"], "2");
    assert_eq!(admins["administrators"].as_array().unwrap().len(), 2);
    let path = format!("/ai/{aid}/profile");
    let edit = json!({"requestId":Uuid::new_v4(),"expectedProfileRevision":"1","name":"New name","bio":"New bio"});
    assert_eq!(
        value(f.human(1, Method::PUT, &path).json(&edit)).await["profileRevision"],
        "2"
    );
    value(f.human(0, Method::PUT, &path).json(
        &json!({"requestId":Uuid::new_v4(),"expectedProfileRevision":"2","name":"Latest","bio":""}),
    ))
    .await;
    assert_eq!(
        value(f.human(1, Method::PUT, &path).json(&edit)).await["account"]["name"],
        "Latest"
    );
    assert_eq!(
        f.human(1, Method::PUT, &admin_path)
            .json(&json!({"requestId":Uuid::new_v4(),"expectedRevision":"2","enabled":false}))
            .send()
            .await
            .unwrap()
            .status(),
        403
    );
    let initial_path = format!("/ai/{aid}/administrators/{}", f.humans[0].0);
    assert_eq!(
        f.human(0, Method::PUT, &initial_path)
            .json(&json!({"requestId":Uuid::new_v4(),"expectedRevision":"2","enabled":false}))
            .send()
            .await
            .unwrap()
            .status(),
        400
    );
    value(
        f.human(0, Method::PUT, &admin_path)
            .json(&json!({"requestId":Uuid::new_v4(),"expectedRevision":"2","enabled":false})),
    )
    .await;
    assert_eq!(
        f.human(1, Method::PUT, &path)
            .json(&edit)
            .send()
            .await
            .unwrap()
            .status(),
        403
    );
    let replay = value(f.human(0, Method::PUT, &admin_path).json(&grant)).await;
    assert_eq!(replay["revision"], "3");
    assert_eq!(replay["administrators"].as_array().unwrap().len(), 1);
    f.close().await;
}
#[tokio::test]
async fn managed_account_view_is_read_only_and_checks_membership_and_attachments() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let ai = f.ai().await;
    let aid = ai["account"]["id"].as_str().unwrap();
    let cid = f.conversation(aid).await;
    let sent=value(f.human(0,Method::POST,&format!("/chat/conversations/{cid}/messages")).json(&json!({"requestId":Uuid::new_v4(),"text":"private file","files":[{"name":"private.txt","mime":"text/plain","data":"aGVsbG8="}]}))).await;
    let before: Vec<(Uuid, i64)> = sqlx::query(
        "SELECT owner,read_position FROM cloud_list WHERE conversation=$1 ORDER BY owner",
    )
    .bind(cid.parse::<Uuid>().unwrap())
    .fetch_all(&f.state.pool)
    .await
    .unwrap()
    .iter()
    .map(|r| (r.get(0), r.get(1)))
    .collect();
    let view = value(f.human(0, Method::GET, &format!("/ai/{aid}/view"))).await;
    assert_eq!(view["account"]["id"], aid);
    assert_eq!(view["conversations"].as_array().unwrap().len(), 1);
    let path = format!("/ai/{aid}/view/conversations/{cid}/messages");
    assert_eq!(
        value(f.human(0, Method::GET, &path)).await["messages"][0]["text"],
        "private file"
    );
    assert_eq!(
        f.human(0, Method::POST, &path)
            .json(&json!({"text":"impersonation"}))
            .send()
            .await
            .unwrap()
            .status(),
        405
    );
    let download = format!(
        "{path}/{}/attachments/0?revision={}",
        sent["id"].as_str().unwrap(),
        sent["revision"].as_str().unwrap()
    );
    assert_eq!(
        f.human(0, Method::GET, &download)
            .send()
            .await
            .unwrap()
            .bytes()
            .await
            .unwrap()
            .as_ref(),
        b"hello"
    );
    assert_eq!(
        f.human(1, Method::GET, &download)
            .send()
            .await
            .unwrap()
            .status(),
        403
    );
    let after: Vec<(Uuid, i64)> = sqlx::query(
        "SELECT owner,read_position FROM cloud_list WHERE conversation=$1 ORDER BY owner",
    )
    .bind(cid.parse::<Uuid>().unwrap())
    .fetch_all(&f.state.pool)
    .await
    .unwrap()
    .iter()
    .map(|r| (r.get(0), r.get(1)))
    .collect();
    assert_eq!(before, after);
    sqlx::query("DELETE FROM cloud_members WHERE conversation=$1 AND account_id=$2")
        .bind(cid.parse::<Uuid>().unwrap())
        .bind(aid.parse::<Uuid>().unwrap())
        .execute(&f.state.pool)
        .await
        .unwrap();
    assert_eq!(
        f.human(0, Method::GET, &path)
            .send()
            .await
            .unwrap()
            .status(),
        404
    );
    assert_eq!(
        f.human(0, Method::GET, &download)
            .send()
            .await
            .unwrap()
            .status(),
        404
    );
    f.close().await;
}
#[tokio::test]
async fn managed_profile_relay_requires_capability_and_rechecks_administration() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let ai = f.ai().await;
    let aid = ai["account"]["id"].as_str().unwrap();
    let (secret, node, _) = f.bind_ai(aid, "0").await;
    let path = format!("/ai/{aid}/management");
    let command = json!({"requestId":Uuid::new_v4(),"operation":{"type":"view"}});
    let response = f
        .human(0, Method::POST, &path)
        .json(&command)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 409);
    assert_eq!(
        response.json::<Value>().await.unwrap()["code"],
        "node_offline"
    );
    let mut ws = connect_node(&f, &secret, node, aid).await;
    let response = f
        .human(0, Method::POST, &path)
        .json(&command)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 409);
    assert_eq!(
        response.json::<Value>().await.unwrap()["code"],
        "unsupported_capability"
    );
    ws.close().await.unwrap();
    let client = connector::node::NodeClient::new(&f.url, &secret, None).unwrap();
    let mut ws = client.connect().await.unwrap();
    ws.send(&connector::node::ClientFrame::Hello {
        protocol_version: 4,
        node_id: node.to_string(),
        ai_id: aid.into(),
        binding_version: "1".into(),
        capabilities: vec!["profileManagement".into()],
    })
    .await
    .unwrap();
    assert!(matches!(
        ws.recv().await.unwrap(),
        Some(connector::node::ServerFrame::Ready { .. })
    ));
    let pending = tokio::spawn(f.human(0, Method::POST, &path).json(&command).send());
    let request = loop {
        let v = serde_json::to_value(ws.recv().await.unwrap().unwrap()).unwrap();
        if v["type"] == "managementRequest" {
            break v;
        }
    };
    assert_eq!(request["operatorId"], json!(f.humans[0].0));
    sqlx::query("DELETE FROM cloud_ai_admins WHERE ai_id=$1 AND administrator=$2")
        .bind(aid.parse::<Uuid>().unwrap())
        .bind(f.humans[0].0)
        .execute(&f.state.pool)
        .await
        .unwrap();
    let response=serde_json::from_value(json!({"type":"managementResponse","relayId":request["relayId"],"requestId":request["requestId"],"view":{"profile":{"revision":"1","persona":"private","providerId":"test","modelId":"test","reasoningEffort":null,"temperature":null,"maxTokens":null,"autoDailyTokens":null},"models":[]},"error":null})).unwrap();
    ws.send(&response).await.unwrap();
    assert_eq!(pending.await.unwrap().unwrap().status(), 403);
    f.close().await;
}
