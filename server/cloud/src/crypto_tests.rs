use super::*;
use crate::chat::tests::Fixture;
use reqwest::{Client, Method};
struct Api {
    url: String,
    task: tokio::task::JoinHandle<()>,
}
impl Api {
    async fn new(f: &Fixture) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let app = crate::router(f.state.clone(), std::env::temp_dir());
        let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        Self { url, task }
    }
    async fn call(&self, h: &HeaderMap, method: Method, path: &str, v: Value) -> (u16, Value) {
        let r = Client::new()
            .request(method, format!("{}/api{path}", self.url))
            .headers(h.clone())
            .header("x-llmn-client", "1")
            .json(&v)
            .send()
            .await
            .unwrap();
        let code = r.status().as_u16();
        let text = r.text().await.unwrap();
        (
            code,
            serde_json::from_str(&text).unwrap_or_else(|_| json!({"invalid":text})),
        )
    }
    async fn ok(&self, h: &HeaderMap, method: Method, path: &str, mut v: Value) -> Value {
        if method == Method::PUT
            && path.starts_with("/crypto/resources/")
            && v.get("expectedAudienceRevision").is_none()
        {
            let mut query = format!(
                "/crypto/resource-participants?aiId={}&kind={}",
                v["aiId"].as_str().unwrap(),
                v["kind"].as_str().unwrap()
            );
            if let Some(cid) = v["conversationId"].as_str() {
                query.push_str(&format!("&conversationId={cid}"));
            }
            let (status, audience) = self.call(h, Method::GET, &query, Value::Null).await;
            assert_eq!(status, 200, "{audience}");
            v["expectedAudienceRevision"] = audience["revision"].clone();
        }
        let (code, body) = self.call(h, method, path, v).await;
        assert_eq!(code, 200, "{body}");
        body
    }
}
impl Drop for Api {
    fn drop(&mut self) {
        self.task.abort();
    }
}
async fn conversation(f: &Fixture, a: Uuid, b: Uuid) -> Uuid {
    let cid = Uuid::new_v4();
    let (first, second) = if a < b { (a, b) } else { (b, a) };
    sqlx::query("INSERT INTO cloud_conversations(id,first_id,second_id) VALUES($1,$2,$3)")
        .bind(cid)
        .bind(first)
        .bind(second)
        .execute(&f.state.pool)
        .await
        .unwrap();
    for owner in [a, b] {
        sqlx::query("INSERT INTO cloud_members(conversation,account_id) VALUES($1,$2)")
            .bind(cid)
            .bind(owner)
            .execute(&f.state.pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO cloud_list(owner,conversation) VALUES($1,$2)")
            .bind(owner)
            .bind(cid)
            .execute(&f.state.pool)
            .await
            .unwrap();
    }
    cid
}
async fn registered(api: &Api, h: &HeaderMap, aid: Uuid) -> String {
    let d = Uuid::new_v4().to_string();
    api.ok(
        h,
        Method::POST,
        "/crypto/devices",
        json!({"deviceId":d,"name":"test device"}),
    )
    .await;
    api.ok(h,Method::POST,"/crypto/requests",json!({"deviceId":d,"requestId":Uuid::new_v4(),"kind":"keysUpload","body":{"device_keys":{"user_id":matrix_user(aid),"device_id":d,"algorithms":["m.olm.v1.curve25519-aes-sha2","m.megolm.v1.aes-sha2"],"keys":{"curve25519:test":"opaque-public","ed25519:test":"opaque-public"},"signatures":{matrix_user(aid):{"ed25519:test":"opaque-signature"}}}}})).await;
    d
}
fn sealed(a: Uuid, d: &str, cid: Uuid, mid: Uuid, rev: u32) -> Value {
    json!({"version":1,"roomId":format!("!{cid}:llmn"),"sender":matrix_user(a),"deviceId":d,"eventId":format!("${mid}.{rev}.final"),"deviceKeys":{"ed25519":"opaque-public","curve25519":"opaque-public"},"signatures":{matrix_user(a):{format!("ed25519:{d}"):"opaque-signature"}},"content":{"algorithm":"m.megolm.v1.aes-sha2","ciphertext":"server-persistence-test-only","session_id":"opaque-session","sender_key":"opaque-public","device_id":d}})
}
#[tokio::test]
async fn matrix_transport_is_scoped_deduplicated_and_revocable() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let api = Api::new(&f).await;
    let (a, ha) = &f.users[0];
    let (b, hb) = &f.users[1];
    conversation(&f, *a, *b).await;
    let da = registered(&api, ha, *a).await;
    let db = registered(&api, hb, *b).await;
    api.ok(ha,Method::POST,"/crypto/requests",json!({"deviceId":da,"requestId":"cross-sign-device","kind":"signatures","body":{matrix_user(*a):{da.clone():{"keys":{"curve25519:test":"opaque-public","ed25519:test":"opaque-public"},"signatures":{matrix_user(*a):{"ed25519:self-signing":"cross-signature"}}}}}})).await;
    let signed=api.ok(ha,Method::POST,"/crypto/requests",json!({"deviceId":da,"requestId":"query-signed","kind":"keysQuery","body":{"device_keys":{matrix_user(*a):[da.clone()]}}})).await;
    assert_eq!(
        signed["device_keys"][matrix_user(*a)][&da]["signatures"][matrix_user(*a)]["ed25519:test"],
        "opaque-signature"
    );
    assert_eq!(
        signed["device_keys"][matrix_user(*a)][&da]["signatures"][matrix_user(*a)]["ed25519:self-signing"],
        "cross-signature"
    );
    api.ok(hb,Method::POST,"/crypto/requests",json!({"deviceId":db,"requestId":"otk","kind":"keysUpload","body":{"one_time_keys":{"signed_curve25519:1":{"key":"one-time-public","signatures":{}}}}})).await;
    let claim = json!({"deviceId":da,"requestId":"claim","kind":"keysClaim","body":{"one_time_keys":{matrix_user(*b):{db.clone():"signed_curve25519"}}}});
    let claimed = api
        .ok(ha, Method::POST, "/crypto/requests", claim.clone())
        .await;
    let repeated = api.ok(ha, Method::POST, "/crypto/requests", claim).await;
    assert_eq!(claimed, repeated);
    assert!(
        claimed["one_time_keys"][matrix_user(*b)][&db]
            .get("signed_curve25519:1")
            .is_some()
    );
    let send = json!({"deviceId":da,"requestId":"delivery","kind":"toDevice","body":{"event_type":"m.room.encrypted","txn_id":"delivery","messages":{matrix_user(*b):{db.clone():{"ciphertext":"opaque"}}}}});
    api.ok(ha, Method::POST, "/crypto/requests", send.clone())
        .await;
    api.ok(ha, Method::POST, "/crypto/requests", send).await;
    let inbox = api
        .ok(
            hb,
            Method::GET,
            &format!("/crypto/sync?deviceId={db}&after=0"),
            Value::Null,
        )
        .await;
    assert_eq!(inbox["to_device"]["events"].as_array().unwrap().len(), 1);
    let next = inbox["next_batch"].as_str().unwrap();
    let empty = api
        .ok(
            hb,
            Method::GET,
            &format!("/crypto/sync?deviceId={db}&after={next}"),
            Value::Null,
        )
        .await;
    assert_eq!(empty["to_device"]["events"], json!([]));
    let denied=api.call(ha,Method::POST,"/crypto/requests",json!({"deviceId":da,"requestId":"unrelated","kind":"toDevice","body":{"event_type":"m.room.encrypted","txn_id":"x","messages":{matrix_user(f.users[2].0):{"*":{"ciphertext":"opaque"}}}}})).await;
    assert_eq!(denied.0, 403);
    api.ok(
        ha,
        Method::POST,
        &format!("/crypto/devices/{da}/revoke"),
        Value::Null,
    )
    .await;
    assert_eq!(
        api.call(
            ha,
            Method::GET,
            &format!("/crypto/sync?deviceId={da}"),
            Value::Null
        )
        .await
        .0,
        403
    );
    drop(api);
    f.close().await;
}
#[tokio::test]
async fn sealed_private_messages_never_write_plaintext_or_fallback() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let api = Api::new(&f).await;
    let (a, ha) = &f.users[0];
    let (b, hb) = &f.users[1];
    let cid = conversation(&f, *a, *b).await;
    let da = registered(&api, ha, *a).await;
    registered(&api, hb, *b).await;
    let activated = api
        .ok(
            ha,
            Method::POST,
            &format!("/chat/conversations/{cid}/encryption"),
            json!({"deviceId":da,"requestId":Uuid::new_v4()}),
        )
        .await;
    assert_eq!(activated["encryption"]["version"], 1);
    assert_eq!(
        api.call(
            ha,
            Method::POST,
            &format!("/chat/conversations/{cid}/messages"),
            json!({"requestId":Uuid::new_v4(),"text":"must not reach storage"})
        )
        .await
        .0,
        409
    );
    let mid = Uuid::new_v4();
    let request = json!({"requestId":Uuid::new_v4(),"messageId":mid,"text":"","sealed":sealed(*a,&da,cid,mid,1)});
    let sent = api
        .ok(
            ha,
            Method::POST,
            &format!("/chat/conversations/{cid}/messages"),
            request.clone(),
        )
        .await;
    assert_eq!(sent["sealed"], request["sealed"]);
    assert_eq!(sent["text"], "");
    assert_eq!(
        api.ok(
            ha,
            Method::POST,
            &format!("/chat/conversations/{cid}/messages"),
            request
        )
        .await["id"],
        mid.to_string()
    );
    let text: String = sqlx::query_scalar("SELECT body FROM cloud_messages WHERE id=$1")
        .bind(mid)
        .fetch_one(&f.state.pool)
        .await
        .unwrap();
    assert!(text.is_empty());
    let changed=api.ok(ha,Method::PATCH,&format!("/chat/messages/{mid}"),json!({"requestId":Uuid::new_v4(),"text":"","expectedRevision":"1","sealed":sealed(*a,&da,cid,mid,2)})).await;
    assert_eq!(changed["revision"], "2");
    let gone = api
        .ok(
            ha,
            Method::POST,
            &format!("/chat/messages/{mid}/withdraw"),
            json!({"requestId":Uuid::new_v4(),"expectedRevision":"2"}),
        )
        .await;
    assert!(gone["sealed"].is_null());
    assert_eq!(gone["status"], "withdrawn");
    assert_eq!(
        api.call(
            ha,
            Method::POST,
            &format!("/chat/conversations/{cid}/contact-intents"),
            json!({"requestId":Uuid::new_v4(),"content":"never store this"})
        )
        .await
        .0,
        409
    );
    drop(api);
    f.close().await;
}
#[tokio::test]
async fn recovery_and_backup_store_only_opaque_payloads() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let api = Api::new(&f).await;
    let (a, h) = &f.users[0];
    let d = registered(&api, h, *a).await;
    let version = Uuid::new_v4();
    api.ok(h,Method::PUT,"/crypto/backup",json!({"deviceId":d,"expectedVersion":null,"version":version,"algorithm":"m.megolm_backup.v1.curve25519-aes-sha2","auth_data":{"public_key":"backup-public","signatures":{}}})).await;
    api.ok(h,Method::POST,"/crypto/requests",json!({"deviceId":d,"requestId":"backup-session","kind":"keysBackup","body":{"version":version,"rooms":{"!room:llmn":{"sessions":{"opaque-session":{"first_message_index":0,"forwarded_count":0,"is_verified":true,"session_data":{"ciphertext":"encrypted-history-key","ephemeral":"public-ephemeral","mac":"opaque-mac"}}}}}}})).await;
    let keys = api
        .ok(h, Method::GET, "/crypto/backup/keys", Value::Null)
        .await;
    assert_eq!(
        keys["rooms"]["!room:llmn"]["sessions"]["opaque-session"]["session_data"]["ciphertext"],
        "encrypted-history-key"
    );
    api.ok(
        h,
        Method::PUT,
        "/crypto/recovery",
        json!({"deviceId":d,"expectedVersion":null,"version":version,"ciphertext":"b3BhcXVlLXNlY3JldHMtYnVuZGxl"}),
    )
    .await;
    assert_eq!(
        api.ok(h, Method::GET, "/crypto/recovery", Value::Null)
            .await["version"],
        version.to_string()
    );
    assert_eq!(
        api.call(&f.users[1].1, Method::GET, "/crypto/recovery", Value::Null)
            .await
            .0,
        404
    );
    let new_version = Uuid::new_v4();
    let next = json!({"deviceId":d,"expectedVersion":version,"version":new_version,"algorithm":"m.megolm_backup.v1.curve25519-aes-sha2","auth_data":{"public_key":"next-backup-public","signatures":{}}});
    api.ok(h, Method::PUT, "/crypto/backup", next.clone()).await;
    api.ok(h, Method::PUT, "/crypto/backup", next).await;
    assert_eq!(api.call(h,Method::PUT,"/crypto/backup",json!({"deviceId":d,"expectedVersion":new_version,"version":version,"algorithm":"m.megolm_backup.v1.curve25519-aes-sha2","auth_data":{"public_key":"backup-public","signatures":{}}})).await.0,409);
    let next = json!({"deviceId":d,"expectedVersion":version,"version":new_version,"ciphertext":"bmV3LW9wYXF1ZS1zZWNyZXRz"});
    api.ok(h, Method::PUT, "/crypto/recovery", next.clone())
        .await;
    api.ok(h, Method::PUT, "/crypto/recovery", next).await;
    assert_eq!(api.call(h,Method::PUT,"/crypto/recovery",json!({"deviceId":d,"expectedVersion":new_version,"version":version,"ciphertext":"b3BhcXVlLXNlY3JldHMtYnVuZGxl"})).await.0,409);
    assert_eq!(api.call(h,Method::PUT,"/crypto/recovery",json!({"deviceId":d,"expectedVersion":new_version,"version":new_version,"ciphertext":"b3ZlcndyaXRl"})).await.0,409);
    drop(api);
    f.close().await;
}
#[tokio::test]
async fn resources_enforce_scope_cas_applied_binding_and_source_tombstones() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let api = Api::new(&f).await;
    let human = f.users[0].0;
    let h = &f.users[0].1;
    let ai = Uuid::new_v4();
    let node = Uuid::new_v4();
    let token = "a".repeat(64);
    sqlx::query("INSERT INTO accounts(id,name,kind) VALUES($1,'AI','ai')")
        .bind(ai)
        .execute(&f.state.pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO cloud_ai_admins VALUES($1,$2,true)")
        .bind(ai)
        .bind(human)
        .execute(&f.state.pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO cloud_ai_bindings(ai_id,version,node_id,token_hash,capabilities) VALUES($1,1,$2,$3,'[\"sealedV1\"]'::jsonb)").bind(ai).bind(node).bind(format!("{:x}",Sha256::digest(token.as_bytes()))).execute(&f.state.pool).await.unwrap();
    let cid = conversation(&f, human, ai).await;
    let dh = registered(&api, h, human).await;
    let mut hn = HeaderMap::new();
    hn.insert("authorization", format!("Bearer {token}").parse().unwrap());
    let dn = registered(&api, &hn, ai).await;
    // Revocation between roster lookup and delivery must not fall back to the
    // still-valid private-chat relationship, including receipt replays.
    let history = json!({"deviceId":dn,"requestId":"admin-history","kind":"toDevice","body":{"llmn_purpose":"aiAdminHistory","event_type":"m.room.encrypted","txn_id":"history","messages":{matrix_user(human):{dh.clone():{"ciphertext":"opaque-history"}}}}});
    api.ok(&hn, Method::POST, "/crypto/requests", history.clone())
        .await;
    sqlx::query("DELETE FROM cloud_ai_admins WHERE ai_id=$1 AND administrator=$2")
        .bind(ai)
        .bind(human)
        .execute(&f.state.pool)
        .await
        .unwrap();
    assert_eq!(
        api.call(&hn, Method::POST, "/crypto/requests", history.clone())
            .await
            .0,
        403
    );
    let mut late = history.clone();
    late["requestId"] = json!("late-admin-history");
    assert_eq!(
        api.call(&hn, Method::POST, "/crypto/requests", late.clone())
            .await
            .0,
        403
    );
    late["requestId"] = json!("ordinary-room-key");
    late["body"].as_object_mut().unwrap().remove("llmn_purpose");
    api.ok(&hn, Method::POST, "/crypto/requests", late).await;
    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM cloud_crypto_inbox WHERE account_id=$1 AND device_id=$2",
    )
    .bind(human)
    .bind(&dh)
    .fetch_one(&f.state.pool)
    .await
    .unwrap();
    assert_eq!(count, 2);
    sqlx::query("INSERT INTO cloud_ai_admins VALUES($1,$2,true)")
        .bind(ai)
        .bind(human)
        .execute(&f.state.pool)
        .await
        .unwrap();
    let item = Uuid::new_v4();
    let mut blob = sealed(human, &dh, cid, item, 1);
    blob["roomId"] = json!(connector::crypto::resource_room(
        &ai.to_string(),
        "profile",
        None
    ));
    let payload = json!({"requestId":Uuid::new_v4(),"deviceId":dh,"aiId":ai,"conversationId":null,"kind":"profile","expectedRevision":"0","sealed":blob,"sources":[],"tombstone":null});
    let participants_path = format!("/crypto/resource-participants?aiId={ai}&kind=profile");
    let old = api
        .ok(h, Method::GET, &participants_path, Value::Null)
        .await;
    let admin = api
        .ok(
            h,
            Method::GET,
            &format!("/ai/{ai}/administrators"),
            Value::Null,
        )
        .await;
    let added = api
        .ok(
            h,
            Method::PUT,
            &format!("/ai/{ai}/administrators/{}", f.users[2].0),
            json!({"requestId":Uuid::new_v4(),"expectedRevision":admin["revision"],"enabled":true}),
        )
        .await;
    api.ok(
        h,
        Method::PUT,
        &format!("/ai/{ai}/administrators/{}", f.users[2].0),
        json!({"requestId":Uuid::new_v4(),"expectedRevision":added["revision"],"enabled":false}),
    )
    .await;
    let current = api
        .ok(h, Method::GET, &participants_path, Value::Null)
        .await;
    assert_eq!(old["accounts"], current["accounts"]);
    assert_ne!(old["revision"], current["revision"]);
    let mut stale = payload.clone();
    stale["expectedAudienceRevision"] = old["revision"].clone();
    let denied = api
        .call(h, Method::PUT, &format!("/crypto/resources/{item}"), stale)
        .await;
    assert_eq!(denied.0, 409);
    assert_eq!(denied.1["code"], "audience_changed");
    registered(&api, h, human).await;
    let newer = api
        .ok(h, Method::GET, &participants_path, Value::Null)
        .await;
    assert_ne!(current["revision"], newer["revision"]);
    let saved = api
        .ok(
            h,
            Method::PUT,
            &format!("/crypto/resources/{item}"),
            payload.clone(),
        )
        .await;
    assert_eq!(saved["revision"], "1");
    assert_eq!(
        api.ok(
            h,
            Method::PUT,
            &format!("/crypto/resources/{item}"),
            payload
        )
        .await,
        saved
    );
    let members = api
        .ok(
            h,
            Method::GET,
            &format!("/crypto/resource-participants?aiId={ai}&kind=profile"),
            Value::Null,
        )
        .await;
    assert_eq!(members["accounts"].as_array().unwrap().len(), 2);
    assert_eq!(
        api.call(
            &f.users[1].1,
            Method::GET,
            &format!("/crypto/resources?aiId={ai}&kind=profile"),
            Value::Null
        )
        .await
        .0,
        403
    );
    let mut tx = f.state.pool.begin().await.unwrap();
    assert!(
        crate::crypto_resources::ready(&mut tx, ai, cid)
            .await
            .is_err()
    );
    tx.rollback().await.unwrap();
    api.ok(
        &hn,
        Method::POST,
        &format!("/crypto/resources/{item}/applied"),
        json!({"deviceId":dn,"revision":"1","status":"applied"}),
    )
    .await;
    let mut tx = f.state.pool.begin().await.unwrap();
    crate::crypto_resources::ready(&mut tx, ai, cid)
        .await
        .unwrap();
    tx.rollback().await.unwrap();
    let source = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO cloud_messages(id,conversation,sender,body) VALUES($1,$2,$3,'legacy source')",
    )
    .bind(source)
    .bind(cid)
    .bind(human)
    .execute(&f.state.pool)
    .await
    .unwrap();
    let memory = Uuid::new_v4();
    let mut blob = sealed(human, &dh, cid, memory, 1);
    blob["roomId"] = json!(connector::crypto::resource_room(
        &ai.to_string(),
        "memory",
        Some(&cid.to_string())
    ));
    api.ok(h,Method::PUT,&format!("/crypto/resources/{memory}"),json!({"requestId":Uuid::new_v4(),"deviceId":dh,"aiId":ai,"conversationId":cid,"kind":"memory","expectedRevision":"0","sealed":blob,"sources":[{"messageId":source,"revision":"1"}],"tombstone":null})).await;
    let summary = Uuid::new_v4();
    let mut blob = sealed(ai, &dn, cid, summary, 1);
    blob["roomId"] = json!(connector::crypto::resource_room(
        &ai.to_string(),
        "summary",
        Some(&cid.to_string())
    ));
    let summary_save = json!({"requestId":Uuid::new_v4(),"deviceId":dn,"aiId":ai,"conversationId":cid,"kind":"summary","expectedRevision":"0","sealed":blob,"sources":[{"messageId":source,"revision":"1"}],"tombstone":null});
    // Freeze the actual request (including audience CAS) for a replay after withdrawal.
    let audience = api
        .ok(
            &hn,
            Method::GET,
            &format!("/crypto/resource-participants?aiId={ai}&conversationId={cid}&kind=summary"),
            Value::Null,
        )
        .await;
    let mut summary_save = summary_save;
    summary_save["expectedAudienceRevision"] = audience["revision"].clone();
    api.ok(
        &hn,
        Method::PUT,
        &format!("/crypto/resources/{summary}"),
        summary_save.clone(),
    )
    .await;
    api.ok(
        h,
        Method::POST,
        &format!("/chat/messages/{source}/withdraw"),
        json!({"requestId":Uuid::new_v4(),"expectedRevision":"1"}),
    )
    .await;
    let replay = api
        .ok(
            &hn,
            Method::PUT,
            &format!("/crypto/resources/{summary}"),
            summary_save,
        )
        .await;
    assert_eq!(replay["tombstone"], "stale");
    assert!(replay["sealed"].is_null());
    assert_eq!(replay["revision"], "2");
    assert_eq!(replay["sources"][0]["messageId"], source.to_string());
    let list = api
        .ok(
            h,
            Method::GET,
            &format!("/crypto/resources?aiId={ai}&conversationId={cid}&kind=memory"),
            Value::Null,
        )
        .await;
    assert_eq!(list["items"][0]["tombstone"], "stale");
    assert!(list["items"][0]["sealed"].is_null());
    assert_eq!(list["items"][0]["revision"], "2");
    let forgotten=api.ok(h,Method::PUT,&format!("/crypto/resources/{memory}"),json!({"requestId":Uuid::new_v4(),"deviceId":dh,"aiId":ai,"conversationId":cid,"kind":"memory","expectedRevision":"2","sealed":null,"sources":[],"tombstone":"forgotten"})).await;
    assert_eq!(forgotten["tombstone"], "forgotten");
    assert_eq!(forgotten["sources"][0]["messageId"], source.to_string());
    assert_eq!(forgotten["revision"], "3");
    sqlx::query("UPDATE cloud_ai_bindings SET version=2 WHERE ai_id=$1")
        .bind(ai)
        .execute(&f.state.pool)
        .await
        .unwrap();
    assert_eq!(
        api.call(
            &hn,
            Method::POST,
            &format!("/crypto/resources/{item}/applied"),
            json!({"deviceId":dn,"revision":"1","status":"applied"})
        )
        .await
        .0,
        403
    );
    // A recovered binding can discover encrypted scopes before materials can
    // pass the applied-resource gate; no message history is returned here.
    sqlx::query(
        "UPDATE cloud_conversations SET crypto_enabled=true,crypto_from_position=0 WHERE id=$1",
    )
    .bind(cid)
    .execute(&f.state.pool)
    .await
    .unwrap();
    conversation(&f, ai, f.users[1].0).await;
    let new_token = "b".repeat(64);
    sqlx::query("UPDATE cloud_ai_bindings SET node_id=$2,token_hash=$3 WHERE ai_id=$1")
        .bind(ai)
        .bind(Uuid::new_v4())
        .bind(format!("{:x}", Sha256::digest(new_token.as_bytes())))
        .execute(&f.state.pool)
        .await
        .unwrap();
    assert_eq!(
        api.call(&hn, Method::GET, "/node/crypto/conversations", Value::Null)
            .await
            .0,
        401
    );
    let mut recovered = HeaderMap::new();
    recovered.insert(
        "authorization",
        format!("Bearer {new_token}").parse().unwrap(),
    );
    let discovery = api
        .ok(
            &recovered,
            Method::GET,
            "/node/crypto/conversations",
            Value::Null,
        )
        .await;
    let windows = discovery["conversations"].as_array().unwrap();
    assert_eq!(windows.len(), 1);
    assert_eq!(windows[0]["id"], cid.to_string());
    assert_eq!(windows[0]["encryption"]["version"], 1);
    assert_eq!(windows[0]["encryption"]["activationPosition"], "0");
    assert_eq!(windows[0]["members"].as_array().unwrap().len(), 2);
    assert!(windows[0].get("messages").is_none());
    assert_eq!(
        api.call(h, Method::GET, "/node/crypto/conversations", Value::Null)
            .await
            .0,
        403
    );
    drop(api);
    f.close().await;
}
#[tokio::test]
async fn encrypted_node_handover_drains_checks_applied_and_requires_ready_or_force() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let api = Api::new(&f).await;
    let human = f.users[0].0;
    let h = &f.users[0].1;
    let ai = Uuid::new_v4();
    let node = Uuid::new_v4();
    let secret = "a".repeat(64);
    sqlx::query("INSERT INTO accounts(id,name,kind) VALUES($1,'AI','ai')")
        .bind(ai)
        .execute(&f.state.pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO cloud_ai_admins VALUES($1,$2,true)")
        .bind(ai)
        .bind(human)
        .execute(&f.state.pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO cloud_ai_bindings(ai_id,version,node_id,token_hash,capabilities) VALUES($1,1,$2,$3,'[\"sealedV1\"]'::jsonb)").bind(ai).bind(node).bind(format!("{:x}",Sha256::digest(secret.as_bytes()))).execute(&f.state.pool).await.unwrap();
    let cid = conversation(&f, human, ai).await;
    let dh = registered(&api, h, human).await;
    let mut hn = HeaderMap::new();
    hn.insert("authorization", format!("Bearer {secret}").parse().unwrap());
    let dn = registered(&api, &hn, ai).await;
    let profile = Uuid::new_v4();
    let mut blob = sealed(human, &dh, cid, profile, 1);
    blob["roomId"] = json!(connector::crypto::resource_room(
        &ai.to_string(),
        "profile",
        None
    ));
    api.ok(h,Method::PUT,&format!("/crypto/resources/{profile}"),json!({"requestId":Uuid::new_v4(),"deviceId":dh,"aiId":ai,"conversationId":null,"kind":"profile","expectedRevision":"0","sealed":blob,"sources":[],"tombstone":null})).await;
    assert_eq!(
        api.call(
            h,
            Method::POST,
            &format!("/ai/{ai}/node"),
            json!({"requestId":Uuid::new_v4(),"expectedBindingVersion":"1","pairingCode":"MISSING"})
        )
        .await
        .0,
        409
    );
    let input = Uuid::new_v4();
    let queued = Uuid::new_v4();
    sqlx::query("INSERT INTO cloud_messages(id,conversation,sender,body) VALUES($1,$2,$3,'prior cloud input')").bind(input).bind(cid).bind(human).execute(&f.state.pool).await.unwrap();
    sqlx::query("INSERT INTO cloud_node_tasks(id,ai_id,conversation,input_id,input_revision,binding_version,expires_at,input_ids) VALUES($1,$2,$3,$4,1,1,now()+interval '1 hour',ARRAY[$4])").bind(queued).bind(ai).bind(cid).bind(input).execute(&f.state.pool).await.unwrap();
    let started = api
        .ok(
            h,
            Method::POST,
            &format!("/ai/{ai}/node/handover"),
            json!({"requestId":Uuid::new_v4(),"expectedBindingVersion":"1"}),
        )
        .await;
    assert_eq!(started["node"]["handoverState"], "draining");
    let state: String = sqlx::query_scalar("SELECT state FROM cloud_node_tasks WHERE id=$1")
        .bind(queued)
        .fetch_one(&f.state.pool)
        .await
        .unwrap();
    assert_eq!(state, "cancelled");
    let sync = api
        .ok(
            &hn,
            Method::GET,
            &format!("/crypto/sync?deviceId={dn}"),
            Value::Null,
        )
        .await;
    assert_eq!(sync["llmn_handover_state"], "draining");
    let version = Uuid::new_v4();
    let ready = json!({"deviceId":dn,"expectedBindingVersion":"1","backupVersion":version});
    assert_eq!(
        api.call(&hn, Method::POST, "/node/handover-ready", ready.clone())
            .await
            .0,
        409
    );
    api.ok(
        &hn,
        Method::POST,
        &format!("/crypto/resources/{profile}/applied"),
        json!({"deviceId":dn,"revision":"1","status":"applied"}),
    )
    .await;
    api.ok(&hn,Method::PUT,"/crypto/backup",json!({"deviceId":dn,"expectedVersion":null,"version":version,"algorithm":"m.megolm_backup.v1.curve25519-aes-sha2","auth_data":{"public_key":"backup-public"}})).await;
    api.ok(&hn, Method::POST, "/node/handover-ready", ready.clone())
        .await;
    let mut blob = sealed(human, &dh, cid, profile, 2);
    blob["roomId"] = json!(connector::crypto::resource_room(
        &ai.to_string(),
        "profile",
        None
    ));
    api.ok(h,Method::PUT,&format!("/crypto/resources/{profile}"),json!({"requestId":Uuid::new_v4(),"deviceId":dh,"aiId":ai,"conversationId":null,"kind":"profile","expectedRevision":"1","sealed":blob,"sources":[],"tombstone":null})).await;
    assert_eq!(
        api.ok(
            &hn,
            Method::GET,
            &format!("/crypto/sync?deviceId={dn}"),
            Value::Null
        )
        .await["llmn_handover_state"],
        "draining"
    );
    assert_eq!(
        api.call(&hn, Method::POST, "/node/handover-ready", ready.clone())
            .await
            .0,
        409
    );
    api.ok(
        &hn,
        Method::POST,
        &format!("/crypto/resources/{profile}/applied"),
        json!({"deviceId":dn,"revision":"2","status":"applied"}),
    )
    .await;
    api.ok(&hn, Method::POST, "/node/handover-ready", ready)
        .await;
    for (index, code, force, expected) in [(2, "NEXTNODE", false, "1"), (3, "FORCENODE", true, "2")]
    {
        let next = Uuid::new_v4();
        let nextsecret = (if index == 2 { "b" } else { "c" }).repeat(64);
        sqlx::query("INSERT INTO cloud_node_pairings(id,node_id,node_name,token_hash,user_code,expires_at) VALUES($1,$2,'next',$3,$4,now()+interval '1 hour')").bind(Uuid::new_v4()).bind(next).bind(format!("{:x}",Sha256::digest(nextsecret.as_bytes()))).bind(code).execute(&f.state.pool).await.unwrap();
        let out=api.ok(h,Method::POST,&format!("/ai/{ai}/node"),json!({"requestId":Uuid::new_v4(),"expectedBindingVersion":expected,"pairingCode":code,"force":force})).await;
        assert_eq!(out["bindingVersion"], index.to_string());
        assert_eq!(out["node"]["handoverState"], "active");
    }
    assert_eq!(
        api.call(
            &hn,
            Method::GET,
            &format!("/crypto/sync?deviceId={dn}"),
            Value::Null
        )
        .await
        .0,
        401
    );
    drop(api);
    f.close().await;
}
