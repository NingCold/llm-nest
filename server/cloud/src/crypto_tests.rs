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
        if v.get("expectedConversationRevision").is_none()
            && (path.ends_with("/encryption") || v.get("sealed").is_some())
        {
            let cid = path
                .strip_prefix("/chat/conversations/")
                .and_then(|v| v.split('/').next());
            if let Some(cid) = cid {
                let (_, windows) = self.call(h, Method::GET, "/chat/init", Value::Null).await;
                if let Some(c) = windows["conversations"]
                    .as_array()
                    .and_then(|rows| rows.iter().find(|c| c["id"] == cid))
                {
                    v["expectedConversationRevision"] = c["revision"].clone();
                }
            } else if path.starts_with("/chat/messages/") && v.get("sealed").is_some() {
                v["expectedConversationRevision"] = json!("2");
            }
        }
        let (code, body) = self.call(h, method, path, v).await;
        assert_eq!(code, 200, "{path}: {body}");
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

#[tokio::test]
async fn encrypted_group_history_audience_and_attachment_boundaries() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let api = Api::new(&f).await;
    let (a, ha) = &f.users[0];
    let (b, hb) = &f.users[1];
    let (c, hc) = &f.users[2];
    let da = registered(&api, ha, *a).await;
    let db = registered(&api, hb, *b).await;
    registered(&api, hc, *c).await;
    let group = api
        .ok(
            ha,
            Method::POST,
            "/chat/groups",
            json!({"requestId":Uuid::new_v4(),"name":"encrypted group","memberIds":[b,c]}),
        )
        .await;
    let cid: Uuid = serde_json::from_value(group["id"].clone()).unwrap();
    let path = format!("/chat/conversations/{cid}/encryption");
    assert_eq!(
        api.call(
            hb,
            Method::POST,
            &path,
            json!({"deviceId":db,"requestId":Uuid::new_v4(),"expectedConversationRevision":"1"})
        )
        .await
        .0,
        403
    );
    let enabled = api
        .ok(
            ha,
            Method::POST,
            &path,
            json!({"deviceId":da,"requestId":Uuid::new_v4(),"expectedConversationRevision":"1"}),
        )
        .await;
    let revision = enabled["revision"].clone();
    let mid = Uuid::new_v4();
    let request = json!({"requestId":Uuid::new_v4(),"messageId":mid,"text":"","sealed":sealed(*a,&da,cid,mid,1),"expectedConversationRevision":revision,"mentions":[b],"encryptedAttachments":[{"index":0,"data":"Y2lwaGVydGV4dA=="}]});
    let message = api
        .ok(
            ha,
            Method::POST,
            &format!("/chat/conversations/{cid}/messages"),
            request.clone(),
        )
        .await;
    assert_eq!(message["attachments"][0]["kind"], "encrypted");
    assert_eq!(
        message["attachments"][0]["mime"],
        "application/octet-stream"
    );
    assert!(message["attachments"][0]["name"].is_null());
    assert_eq!(message["audienceRevision"], revision);
    let mut occupied = request.clone();
    occupied["requestId"] = json!(Uuid::new_v4());
    occupied["encryptedAttachments"][0]["data"] = json!("cmVwbGFjZWQ=");
    assert_eq!(
        api.call(
            ha,
            Method::POST,
            &format!("/chat/conversations/{cid}/messages"),
            occupied
        )
        .await
        .0,
        409
    );
    assert_eq!(
        tokio::fs::read(f.state.attachments.join(mid.to_string()).join("0"))
            .await
            .unwrap(),
        b"ciphertext"
    );

    let mut stale = request.clone();
    stale["requestId"] = json!(Uuid::new_v4());
    stale["messageId"] = json!(Uuid::new_v4());
    stale["expectedConversationRevision"] = json!("1");
    assert_eq!(
        api.call(
            ha,
            Method::POST,
            &format!("/chat/conversations/{cid}/messages"),
            stale
        )
        .await
        .0,
        409
    );
    let history = json!({"deviceId":da,"requestId":Uuid::new_v4(),"kind":"toDevice","body":{"event_type":"m.room.encrypted","txn_id":"history","llmn_purpose":"conversationHistory","llmn_conversation_id":cid,"llmn_expected_revision":revision,"messages":{matrix_user(*b):{db.clone():{"ciphertext":"olm-secret"}}}}});
    api.ok(ha, Method::POST, "/crypto/requests", history.clone())
        .await;
    sqlx::query(
        "UPDATE cloud_crypto_devices SET revoked=true WHERE account_id=$1 AND device_id=$2",
    )
    .bind(b)
    .bind(&db)
    .execute(&f.state.pool)
    .await
    .unwrap();
    assert_eq!(
        api.call(ha, Method::POST, "/crypto/requests", history.clone())
            .await
            .0,
        403
    );
    sqlx::query(
        "UPDATE cloud_crypto_devices SET revoked=false WHERE account_id=$1 AND device_id=$2",
    )
    .bind(b)
    .bind(&db)
    .execute(&f.state.pool)
    .await
    .unwrap();
    sqlx::query("DELETE FROM cloud_members WHERE conversation=$1 AND account_id=$2")
        .bind(cid)
        .bind(b)
        .execute(&f.state.pool)
        .await
        .unwrap();
    assert_eq!(
        api.call(ha, Method::POST, "/crypto/requests", history)
            .await
            .0,
        403
    );
    let stored: String = sqlx::query_scalar("SELECT body FROM cloud_messages WHERE id=$1")
        .bind(mid)
        .fetch_one(&f.state.pool)
        .await
        .unwrap();
    assert!(stored.is_empty());
    drop(api);
    f.close().await;
}

#[tokio::test]
async fn opaque_attachments_reject_bad_indexes_before_writing() {
    let dir = tempfile::tempdir().unwrap();
    let id = Uuid::new_v4();
    let upload = connector::crypto::EncryptedAttachment {
        index: 1,
        data: "YQ==".into(),
    };
    assert!(
        crate::chat::save_encrypted_uploads(dir.path(), id, &[upload])
            .await
            .is_err()
    );
    assert!(!dir.path().join(id.to_string()).exists());
}

#[tokio::test]
async fn encrypted_contact_keeps_schedule_metadata_and_ciphertext_only() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let api = Api::new(&f).await;
    let (human, h) = &f.users[0];
    let ai = Uuid::new_v4();
    sqlx::query("INSERT INTO accounts(id,name,kind) VALUES($1,'contact AI','ai')")
        .bind(ai)
        .execute(&f.state.pool)
        .await
        .unwrap();
    let cid = conversation(&f, *human, ai).await;
    let device = registered(&api, h, *human).await;
    sqlx::query("UPDATE cloud_conversations SET crypto_enabled=true WHERE id=$1")
        .bind(cid)
        .execute(&f.state.pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO cloud_participation(conversation,enabled) VALUES($1,true)")
        .bind(cid)
        .execute(&f.state.pool)
        .await
        .unwrap();
    let iid = Uuid::new_v4();
    let due = chrono::Utc::now() + chrono::Duration::hours(1);
    let mut blob = sealed(*human, &device, cid, iid, 1);
    use base64::Engine;
    blob["content"]["ciphertext"] =
        json!(base64::engine::general_purpose::STANDARD.encode(format!(
            "{}{}",
            "字".repeat(4000),
            "signed-schedule-metadata".repeat(100)
        )));
    let request = json!({"requestId":Uuid::new_v4(),"intentId":iid,"deviceId":device,"aiId":ai,"content":"","sealed":blob,"expectedConversationRevision":"1","dueAt":due,"expiresAt":due+chrono::Duration::hours(2),"utcOffsetMinutes":480});
    assert!(serde_json::to_vec(&request).unwrap().len() > 16 * 1024);
    let path = format!("/chat/conversations/{cid}/contact-intents");
    let saved = api.ok(h, Method::POST, &path, request.clone()).await;
    assert_eq!(saved["id"], iid.to_string());
    assert_eq!(saved["content"], "");
    assert_eq!(saved["audienceRevision"], "1");
    assert_eq!(
        api.ok(h, Method::POST, &path, request.clone()).await["id"],
        saved["id"]
    );
    let stored: String =
        sqlx::query_scalar("SELECT content FROM cloud_contact_intents WHERE id=$1")
            .bind(iid)
            .fetch_one(&f.state.pool)
            .await
            .unwrap();
    assert!(stored.is_empty());
    let node = Uuid::new_v4();
    let token = "c".repeat(64);
    sqlx::query("INSERT INTO cloud_ai_bindings(ai_id,version,node_id,token_hash,capabilities) VALUES($1,1,$2,$3,'[\"sealedV1\"]')").bind(ai).bind(node).bind(format!("{:x}",Sha256::digest(token.as_bytes()))).execute(&f.state.pool).await.unwrap();
    let tid = Uuid::new_v4();
    let trigger = json!({"type":"contact","intentId":iid,"creatorId":human,"revision":"2","content":"","sealed":saved["sealed"],"dueAt":saved["dueAt"],"expiresAt":saved["expiresAt"],"utcOffsetMinutes":480,"audienceRevision":"1"});
    sqlx::query("INSERT INTO cloud_node_tasks(id,ai_id,conversation,contact_intent_id,binding_version,conversation_revision,expires_at,trigger) VALUES($1,$2,$3,$4,1,1,now()+interval '3 hours',$5)").bind(tid).bind(ai).bind(cid).bind(iid).bind(trigger).execute(&f.state.pool).await.unwrap();
    sqlx::query("UPDATE cloud_contact_intents SET status='claimed',task_id=$2 WHERE id=$1")
        .bind(iid)
        .bind(tid)
        .execute(&f.state.pool)
        .await
        .unwrap();
    let mut headers = HeaderMap::new();
    headers.insert("authorization", format!("Bearer {token}").parse().unwrap());
    let materials = api
        .ok(
            &headers,
            Method::GET,
            &format!("/node/tasks/{tid}/materials"),
            Value::Null,
        )
        .await;
    assert_eq!(materials["task"]["trigger"]["sealed"], saved["sealed"]);
    assert_eq!(materials["conversation"]["encryption"]["version"], 1);
    let mid = Uuid::new_v4();
    let uploads = [connector::crypto::EncryptedAttachment {
        index: 0,
        data: "Y2lwaGVydGV4dA==".into(),
    }];
    let parts = crate::chat::save_encrypted_uploads(&f.state.attachments, mid, &uploads)
        .await
        .unwrap();
    sqlx::query("INSERT INTO cloud_messages(id,conversation,sender,body,sealed,attachments,crypto_audience_revision) VALUES($1,$2,$3,'',$4,$5,1)").bind(mid).bind(cid).bind(human).bind(sealed(*human,&device,cid,mid,1)).bind(parts).execute(&f.state.pool).await.unwrap();
    let client = connector::node::NodeClient::new(&api.url, &token, None).unwrap();
    assert_eq!(
        client
            .attachment(&tid.to_string(), &mid.to_string(), 0, "1")
            .await
            .unwrap(),
        b"ciphertext"
    );
    assert!(
        client
            .attachment(&tid.to_string(), &mid.to_string(), 0, "2")
            .await
            .is_err()
    );
    sqlx::query("UPDATE cloud_messages SET withdrawn=true WHERE id=$1")
        .bind(mid)
        .execute(&f.state.pool)
        .await
        .unwrap();
    assert!(
        client
            .attachment(&tid.to_string(), &mid.to_string(), 0, "1")
            .await
            .is_err()
    );

    let mut plain = request.clone();
    plain["requestId"] = json!(Uuid::new_v4());
    plain["content"] = json!("must not store");
    assert_eq!(api.call(h, Method::POST, &path, plain).await.0, 409);
    let mut stale = request;
    stale["requestId"] = json!(Uuid::new_v4());
    stale["expectedConversationRevision"] = json!("0");
    assert_eq!(api.call(h, Method::POST, &path, stale).await.0, 409);
    drop(api);
    f.close().await;
}

#[tokio::test]
async fn encryption_enable_replays_frozen_intent_and_rechecks_authority() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let api = Api::new(&f).await;
    let (a, ha) = &f.users[0];
    let (b, hb) = &f.users[1];
    let device = registered(&api, ha, *a).await;
    registered(&api, hb, *b).await;
    let group = api
        .ok(
            ha,
            Method::POST,
            "/chat/groups",
            json!({"requestId":Uuid::new_v4(),"name":"enable replay","memberIds":[b]}),
        )
        .await;
    let cid: Uuid = serde_json::from_value(group["id"].clone()).unwrap();
    let path = format!("/chat/conversations/{cid}/encryption");
    let request =
        json!({"requestId":Uuid::new_v4(),"deviceId":device,"expectedConversationRevision":"1"});
    let enabled = api.ok(ha, Method::POST, &path, request.clone()).await;
    let replay = api.ok(ha, Method::POST, &path, request.clone()).await;
    assert_eq!(replay["revision"], enabled["revision"]);
    let mut changed = request.clone();
    changed["expectedConversationRevision"] = enabled["revision"].clone();
    assert_eq!(api.call(ha, Method::POST, &path, changed).await.0, 409);
    sqlx::query(
        "UPDATE cloud_crypto_devices SET revoked=true WHERE account_id=$1 AND device_id=$2",
    )
    .bind(a)
    .bind(&device)
    .execute(&f.state.pool)
    .await
    .unwrap();
    assert_eq!(
        api.call(ha, Method::POST, &path, request.clone()).await.0,
        403
    );
    sqlx::query(
        "UPDATE cloud_crypto_devices SET revoked=false WHERE account_id=$1 AND device_id=$2",
    )
    .bind(a)
    .bind(&device)
    .execute(&f.state.pool)
    .await
    .unwrap();
    sqlx::query("UPDATE cloud_members SET role='member' WHERE conversation=$1 AND account_id=$2")
        .bind(cid)
        .bind(a)
        .execute(&f.state.pool)
        .await
        .unwrap();
    assert_eq!(
        api.call(ha, Method::POST, &path, request.clone()).await.0,
        403
    );
    sqlx::query("DELETE FROM cloud_members WHERE conversation=$1 AND account_id=$2")
        .bind(cid)
        .bind(a)
        .execute(&f.state.pool)
        .await
        .unwrap();
    assert_eq!(api.call(ha, Method::POST, &path, request).await.0, 404);
    drop(api);
    f.close().await;
}

#[tokio::test]
async fn encrypted_group_resources_keep_each_ai_scopes_and_permissions() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let api = Api::new(&f).await;
    let (human, h) = &f.users[0];
    let (other, ho) = &f.users[1];
    let device = registered(&api, h, *human).await;
    let mut agents = Vec::new();
    for n in 0..2 {
        let ai = Uuid::new_v4();
        let node = Uuid::new_v4();
        let token = format!("{n}").repeat(64);
        sqlx::query("INSERT INTO accounts(id,name,kind) VALUES($1,'group AI','ai')")
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
        sqlx::query("INSERT INTO cloud_ai_bindings(ai_id,version,node_id,token_hash,capabilities) VALUES($1,1,$2,$3,'[\"sealedV1\"]')").bind(ai).bind(node).bind(format!("{:x}",Sha256::digest(token.as_bytes()))).execute(&f.state.pool).await.unwrap();
        let mut headers = HeaderMap::new();
        headers.insert("authorization", format!("Bearer {token}").parse().unwrap());
        let d = registered(&api, &headers, ai).await;
        agents.push((ai, headers, d));
    }
    let group=api.ok(h,Method::POST,"/chat/groups",json!({"requestId":Uuid::new_v4(),"name":"scoped encryption","memberIds":[other,agents[0].0,agents[1].0]})).await;
    let cid: Uuid = serde_json::from_value(group["id"].clone()).unwrap();
    sqlx::query("UPDATE cloud_conversations SET crypto_enabled=true WHERE id=$1")
        .bind(cid)
        .execute(&f.state.pool)
        .await
        .unwrap();
    let mut summary_ids = Vec::new();
    for (ai, headers, node_device) in &agents {
        for kind in ["profile", "memory", "summarySettings", "summary"] {
            let id = Uuid::new_v4();
            let conversation = (kind != "profile").then_some(cid);
            let mut blob = sealed(*human, &device, cid, id, 1);
            blob["roomId"] = json!(connector::crypto::resource_room(
                &ai.to_string(),
                kind,
                conversation.map(|c| c.to_string()).as_deref()
            ));
            let payload = json!({"requestId":Uuid::new_v4(),"deviceId":device,"aiId":ai,"conversationId":conversation,"kind":kind,"expectedRevision":"0","sealed":blob,"sources":[],"tombstone":null});
            let saved = api
                .ok(h, Method::PUT, &format!("/crypto/resources/{id}"), payload)
                .await;
            assert_eq!(saved["aiId"], ai.to_string());
            api.ok(
                headers,
                Method::POST,
                &format!("/crypto/resources/{id}/applied"),
                json!({"deviceId":node_device,"revision":"1","status":"applied"}),
            )
            .await;
            if kind == "summary" {
                summary_ids.push(id);
            }
        }
        let memory = api
            .ok(
                ho,
                Method::GET,
                &format!("/crypto/resources?aiId={ai}&conversationId={cid}&kind=memory"),
                Value::Null,
            )
            .await;
        assert_eq!(memory["items"].as_array().unwrap().len(), 1);
        assert_eq!(
            api.call(
                ho,
                Method::GET,
                &format!("/crypto/resources?aiId={ai}&conversationId={cid}&kind=summary"),
                Value::Null
            )
            .await
            .0,
            403
        );
    }
    for (index, (ai, headers, _)) in agents.iter().enumerate() {
        let summaries = api
            .ok(
                headers,
                Method::GET,
                &format!("/crypto/resources?aiId={ai}&conversationId={cid}&kind=summary"),
                Value::Null,
            )
            .await;
        assert_eq!(summaries["items"].as_array().unwrap().len(), 1);
        assert_eq!(summaries["items"][0]["id"], summary_ids[index].to_string());
        assert_eq!(summaries["items"][0]["applied"]["status"], "applied");
        let other_ai = agents[1 - index].0;
        assert_eq!(
            api.call(
                headers,
                Method::GET,
                &format!("/crypto/resources?aiId={other_ai}&conversationId={cid}&kind=summary"),
                Value::Null
            )
            .await
            .0,
            403
        );
    }
    let removed = agents[0].0;
    api.ok(
        h,
        Method::DELETE,
        &format!("/chat/groups/{cid}/members/{removed}"),
        json!({"requestId":Uuid::new_v4(),"expectedRevision":"1"}),
    )
    .await;
    api.ok(
        h,
        Method::POST,
        &format!("/chat/groups/{cid}/members"),
        json!({"requestId":Uuid::new_v4(),"expectedRevision":"2","accountId":removed}),
    )
    .await;
    let mut settings_id = None;
    for kind in ["memory", "summary", "summarySettings"] {
        let items = api
            .ok(
                &agents[0].1,
                Method::GET,
                &format!("/crypto/resources?aiId={removed}&conversationId={cid}&kind={kind}"),
                Value::Null,
            )
            .await;
        let item = &items["items"][0];
        assert_eq!(item["tombstone"], "stale");
        assert!(item["sealed"].is_null());
        assert_eq!(item["revision"], "2");
        api.ok(
            &agents[0].1,
            Method::POST,
            &format!("/crypto/resources/{}/applied", item["id"].as_str().unwrap()),
            json!({"deviceId":agents[0].2,"revision":"2","status":"applied"}),
        )
        .await;
        if kind == "summarySettings" {
            settings_id = item["id"].as_str().map(str::to_owned);
        }
    }
    let other_ai = agents[1].0;
    let unaffected = api
        .ok(
            &agents[1].1,
            Method::GET,
            &format!("/crypto/resources?aiId={other_ai}&conversationId={cid}&kind=summary"),
            Value::Null,
        )
        .await;
    assert_eq!(unaffected["items"][0]["revision"], "1");
    assert!(unaffected["items"][0]["tombstone"].is_null());
    let id = settings_id.unwrap();
    let item: Uuid = id.parse().unwrap();
    let mut blob = sealed(*human, &device, cid, item, 3);
    blob["roomId"] = json!(connector::crypto::resource_room(
        &removed.to_string(),
        "summarySettings",
        Some(&cid.to_string())
    ));
    let participants=api.ok(h,Method::GET,&format!("/crypto/resource-participants?aiId={removed}&conversationId={cid}&kind=summarySettings"),Value::Null).await;
    let mut save = json!({"requestId":Uuid::new_v4(),"deviceId":device,"aiId":removed,"conversationId":cid,"kind":"summarySettings","expectedRevision":"1","expectedAudienceRevision":participants["revision"],"sealed":blob,"sources":[],"tombstone":null});
    assert_eq!(
        api.call(
            h,
            Method::PUT,
            &format!("/crypto/resources/{id}"),
            save.clone()
        )
        .await
        .0,
        409
    );
    save["expectedRevision"] = json!("2");
    let enabled = api
        .ok(h, Method::PUT, &format!("/crypto/resources/{id}"), save)
        .await;
    assert_eq!(enabled["revision"], "3");
    assert!(enabled["tombstone"].is_null());
    drop(api);
    f.close().await;
}

#[tokio::test]
async fn encrypted_uploads_recover_partial_files_and_fixed_id_retries() {
    use base64::Engine;
    let root = tempfile::tempdir().unwrap();
    let id = Uuid::new_v4();
    let dir = root.path().join(id.to_string());
    tokio::fs::create_dir_all(&dir).await.unwrap();
    tokio::fs::write(dir.join("0"), b"partial previous final")
        .await
        .unwrap();
    tokio::fs::write(dir.join("0.pending"), b"partial interrupted write")
        .await
        .unwrap();
    let upload = connector::crypto::EncryptedAttachment {
        index: 0,
        data: base64::engine::general_purpose::STANDARD.encode(b"frozen ciphertext"),
    };
    let parts = crate::chat::save_encrypted_uploads(root.path(), id, std::slice::from_ref(&upload))
        .await
        .unwrap();
    assert_eq!(
        tokio::fs::read(dir.join("0")).await.unwrap(),
        b"frozen ciphertext"
    );
    assert!(!dir.join("0.pending").exists());
    assert_eq!(
        crate::chat::save_encrypted_uploads(root.path(), id, &[upload])
            .await
            .unwrap(),
        parts
    );
}

#[tokio::test]
async fn history_conversion_preserves_identity_sources_readstate_and_private_backups() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let api = Api::new(&f).await;
    let (a, ha) = &f.users[0];
    let (b, hb) = &f.users[1];
    let cid = conversation(&f, *a, *b).await;
    let original=api.ok(hb,Method::POST,&format!("/chat/conversations/{cid}/messages"),json!({"requestId":Uuid::new_v4(),"text":"old author's words","files":[{"name":"old.txt","mime":"text/plain","data":"b2xkLWJ5dGVz"}]})).await;
    let mid: Uuid = serde_json::from_value(original["id"].clone()).unwrap();
    sqlx::query("UPDATE cloud_messages SET revision=4,crypto_audience_revision=9 WHERE id=$1")
        .bind(mid)
        .execute(&f.state.pool)
        .await
        .unwrap();
    let reply = api
        .ok(
            ha,
            Method::POST,
            &format!("/chat/conversations/{cid}/messages"),
            json!({"requestId":Uuid::new_v4(),"text":"reply history","replyTo":mid}),
        )
        .await;
    let replyid: Uuid = serde_json::from_value(reply["id"].clone()).unwrap();
    sqlx::query("INSERT INTO cloud_reference_sources(message_id,conversation,source_id,source_revision) VALUES($1,$2,$3,4)").bind(replyid).bind(cid).bind(mid).execute(&f.state.pool).await.unwrap();
    sqlx::query("UPDATE cloud_list SET read_position=$3 WHERE owner=$1 AND conversation=$2")
        .bind(a)
        .bind(cid)
        .bind(
            original["position"]
                .as_str()
                .unwrap()
                .parse::<i64>()
                .unwrap(),
        )
        .execute(&f.state.pool)
        .await
        .unwrap();
    let ai = Uuid::new_v4();
    sqlx::query("INSERT INTO accounts(id,name,kind) VALUES($1,'former AI','ai')")
        .bind(ai)
        .execute(&f.state.pool)
        .await
        .unwrap();
    let intent = Uuid::new_v4();
    sqlx::query("INSERT INTO cloud_contact_intents(id,conversation,creator_id,ai_id,content,due_at,expires_at,utc_offset_minutes,status,revision) VALUES($1,$2,$3,$4,'old intent',now()-interval '2 day',now()-interval '1 day',480,'cancelled',7)").bind(intent).bind(cid).bind(b).bind(ai).execute(&f.state.pool).await.unwrap();
    sqlx::query("UPDATE cloud_conversations SET read_only=true WHERE id=$1")
        .bind(cid)
        .execute(&f.state.pool)
        .await
        .unwrap();
    let device = registered(&api, ha, *a).await;
    let preview = api
        .ok(
            ha,
            Method::POST,
            "/crypto/history-migrations/preview",
            json!({"conversationId":cid}),
        )
        .await;
    let begin = json!({"requestId":Uuid::new_v4(),"deviceId":device,"conversationId":cid,"expectedConversationRevision":preview["conversationRevision"],"sourceDigest":preview["sourceDigest"],"backupDigest":"a".repeat(64),"recoveryDigest":"b".repeat(64),"localPendingResolved":true});
    let started = api
        .ok(
            ha,
            Method::POST,
            "/crypto/history-migrations",
            begin.clone(),
        )
        .await;
    assert_eq!(
        api.ok(
            ha,
            Method::POST,
            "/crypto/history-migrations",
            begin.clone()
        )
        .await["id"],
        started["id"]
    );
    let mut competing = begin.clone();
    competing["requestId"] = json!(Uuid::new_v4());
    assert_eq!(
        api.call(ha, Method::POST, "/crypto/history-migrations", competing)
            .await
            .0,
        409
    );
    let id: Uuid = serde_json::from_value(started["id"].clone()).unwrap();
    assert_eq!(
        api.ok(
            ha,
            Method::GET,
            &format!("/crypto/history-migrations?conversationId={cid}"),
            Value::Null
        )
        .await["migration"]["id"],
        started["id"]
    );
    let finish =
        json!({"requestId":Uuid::new_v4(),"deviceId":device,"verificationDigest":"c".repeat(64)});
    assert_eq!(
        api.call(
            ha,
            Method::POST,
            &format!("/crypto/history-migrations/{id}/finish"),
            finish.clone()
        )
        .await
        .0,
        409
    );
    let list = api
        .ok(
            ha,
            Method::GET,
            &format!("/crypto/history-migrations/{id}/items?includeConverted=true"),
            Value::Null,
        )
        .await;
    assert_eq!(list["items"].as_array().unwrap().len(), 3);
    let old_message = list["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["id"] == mid.to_string())
        .unwrap();
    assert_eq!(old_message["originalAudienceRevision"], "9");
    for entry in list["items"].as_array().unwrap() {
        let item: Uuid = serde_json::from_value(entry["id"].clone()).unwrap();
        let kind = entry["kind"].as_str().unwrap();
        let rev = entry["revision"].as_str().unwrap().parse::<u32>().unwrap();
        let mut blob = sealed(*a, &device, cid, item, rev);
        blob["eventId"] = json!(format!(
            "${item}.{}.migration.{id}",
            if kind == "contactIntent" { 1 } else { rev }
        ));
        let mut command = json!({"requestId":Uuid::new_v4(),"deviceId":device,"expectedRevision":entry["revision"],"sourceDigest":entry["sourceDigest"],"sealed":blob});
        if item == mid {
            command["encryptedAttachments"] = json!([{"index":0,"data":"bmV3LWNpcGhlcg=="}]);
        }
        let path = format!("/crypto/history-migrations/{id}/items/{kind}/{item}");
        if item == mid {
            tokio::fs::write(
                f.state.attachments.join(mid.to_string()).join("0"),
                b"unexpected changed bytes",
            )
            .await
            .unwrap();
            assert_eq!(
                api.call(ha, Method::PUT, &path, command.clone()).await.0,
                409
            );
            tokio::fs::write(
                f.state.attachments.join(mid.to_string()).join("0"),
                b"old-bytes",
            )
            .await
            .unwrap();
        }
        let converted = api.ok(ha, Method::PUT, &path, command.clone()).await;
        assert_eq!(
            api.ok(ha, Method::PUT, &path, command.clone()).await,
            converted
        );
        assert_eq!(
            converted["record"]["historyMigration"]["actorId"],
            a.to_string()
        );
        assert_eq!(converted["record"]["revision"], entry["revision"]);
        if item == mid {
            assert_eq!(converted["record"]["senderId"], b.to_string());
            assert_eq!(converted["record"]["position"], original["position"]);
        }
        command["sourceDigest"] = json!("0".repeat(64));
        assert_eq!(api.call(ha, Method::PUT, &path, command).await.0, 409);
    }
    assert_eq!(
        tokio::fs::read(f.state.attachments.join(mid.to_string()).join("0"))
            .await
            .unwrap(),
        b"old-bytes"
    );
    let response = Client::new()
        .get(format!(
            "{}/api/chat/messages/{mid}/attachments/0?revision=4",
            api.url
        ))
        .headers(ha.clone())
        .send()
        .await
        .unwrap();
    assert_eq!(response.bytes().await.unwrap().as_ref(), b"new-cipher");
    let done = api
        .ok(
            ha,
            Method::POST,
            &format!("/crypto/history-migrations/{id}/finish"),
            finish.clone(),
        )
        .await;
    assert_eq!(done["state"], "complete");
    assert_eq!(
        api.ok(
            ha,
            Method::GET,
            &format!("/crypto/history-migrations?conversationId={cid}"),
            Value::Null
        )
        .await["migration"]["state"],
        "complete"
    );

    assert_eq!(
        api.ok(
            ha,
            Method::POST,
            &format!("/crypto/history-migrations/{id}/finish"),
            finish
        )
        .await["state"],
        "complete"
    );
    let init = api.ok(ha, Method::GET, "/chat/init", Value::Null).await;
    let conv = init["conversations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["id"] == cid.to_string())
        .unwrap();
    assert_eq!(conv["encryption"]["historyComplete"], true);
    assert_eq!(conv["encryption"]["activationPosition"], "0");
    assert_eq!(conv["readOnly"], true);
    assert_eq!(conv["readPosition"], original["position"]);
    let sources: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM cloud_reference_sources WHERE source_id=$1 AND source_revision=4",
    )
    .bind(mid)
    .fetch_one(&f.state.pool)
    .await
    .unwrap();
    assert_eq!(sources, 1);
    let tasks: i64 =
        sqlx::query_scalar("SELECT count(*) FROM cloud_node_tasks WHERE conversation=$1")
            .bind(cid)
            .fetch_one(&f.state.pool)
            .await
            .unwrap();
    assert_eq!(tasks, 0);
    drop(api);
    f.close().await;
}

#[tokio::test]
async fn forced_encryption_rejects_plain_paths_and_reports_requirement() {
    let Some(mut f) = Fixture::new().await else {
        return;
    };
    Arc::get_mut(&mut f.state).unwrap().require_encryption = true;
    let api = Api::new(&f).await;
    let (a, h) = &f.users[0];
    let cid = conversation(&f, *a, f.users[1].0).await;
    assert_eq!(
        api.ok(h, Method::GET, "/chat/init", Value::Null).await["requireEncryption"],
        true
    );
    assert_eq!(
        api.call(
            h,
            Method::POST,
            &format!("/chat/conversations/{cid}/messages"),
            json!({"requestId":Uuid::new_v4(),"text":"plain"})
        )
        .await
        .0,
        409
    );
    assert_eq!(
        api.call(
            h,
            Method::POST,
            &format!("/chat/conversations/{cid}/contact-intents"),
            json!({"requestId":Uuid::new_v4(),"content":"plain"})
        )
        .await
        .0,
        409
    );
    assert_eq!(
        api.call(h, Method::POST, "/migration/import", json!({}))
            .await
            .0,
        409
    );
    drop(api);
    f.close().await;
}

#[tokio::test]
async fn history_migration_freezes_content_but_allows_revocation_and_repreview() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let api = Api::new(&f).await;
    let (a, ha) = &f.users[0];
    let (_, hb) = &f.users[1];
    let device = registered(&api, ha, *a).await;
    let group=api.ok(ha,Method::POST,"/chat/groups",json!({"requestId":Uuid::new_v4(),"name":"migration conflict","memberIds":[f.users[1].0]})).await;
    let cid: Uuid = serde_json::from_value(group["id"].clone()).unwrap();
    let message = api
        .ok(
            ha,
            Method::POST,
            &format!("/chat/conversations/{cid}/messages"),
            json!({"requestId":Uuid::new_v4(),"text":"original"}),
        )
        .await;
    let mid: Uuid = serde_json::from_value(message["id"].clone()).unwrap();
    assert_eq!(
        api.call(
            hb,
            Method::POST,
            "/crypto/history-migrations/preview",
            json!({"conversationId":cid})
        )
        .await
        .0,
        403
    );
    let preview = api
        .ok(
            ha,
            Method::POST,
            "/crypto/history-migrations/preview",
            json!({"conversationId":cid}),
        )
        .await;
    let mut request = json!({"requestId":Uuid::new_v4(),"deviceId":device,"conversationId":cid,"expectedConversationRevision":preview["conversationRevision"],"sourceDigest":preview["sourceDigest"],"backupDigest":"a".repeat(64),"recoveryDigest":"b".repeat(64),"localPendingResolved":true});
    let first = api
        .ok(
            ha,
            Method::POST,
            "/crypto/history-migrations",
            request.clone(),
        )
        .await;
    let old_id = first["id"].as_str().unwrap();
    assert_eq!(
        api.call(
            ha,
            Method::POST,
            &format!("/chat/conversations/{cid}/messages"),
            json!({"requestId":Uuid::new_v4(),"text":"must not send"})
        )
        .await
        .0,
        409
    );
    api.ok(
        ha,
        Method::POST,
        &format!("/chat/groups/{cid}/members"),
        json!({"requestId":Uuid::new_v4(),"expectedRevision":"1","accountId":f.users[2].0}),
    )
    .await;
    assert_eq!(
        api.ok(
            ha,
            Method::GET,
            &format!("/crypto/history-migrations/{old_id}"),
            Value::Null
        )
        .await["state"],
        "conflicted"
    );
    let preview = api
        .ok(
            ha,
            Method::POST,
            "/crypto/history-migrations/preview",
            json!({"conversationId":cid}),
        )
        .await;
    request["requestId"] = json!(Uuid::new_v4());
    request["expectedConversationRevision"] = preview["conversationRevision"].clone();
    request["sourceDigest"] = preview["sourceDigest"].clone();
    let next = api
        .ok(ha, Method::POST, "/crypto/history-migrations", request)
        .await;
    let id: Uuid = serde_json::from_value(next["id"].clone()).unwrap();
    assert_ne!(next["id"], first["id"]);
    let list = api
        .ok(
            ha,
            Method::GET,
            &format!("/crypto/history-migrations/{id}/items"),
            Value::Null,
        )
        .await;
    let entry = &list["items"][0];
    let mut blob = sealed(*a, &device, cid, mid, 1);
    blob["eventId"] = json!(format!("${mid}.1.migration.{id}"));
    api.ok(ha,Method::PUT,&format!("/crypto/history-migrations/{id}/items/message/{mid}"),json!({"requestId":Uuid::new_v4(),"deviceId":device,"expectedRevision":"1","sourceDigest":entry["sourceDigest"],"sealed":blob})).await;
    api.ok(
        ha,
        Method::POST,
        &format!("/crypto/history-migrations/{id}/finish"),
        json!({"requestId":Uuid::new_v4(),"deviceId":device,"verificationDigest":"c".repeat(64)}),
    )
    .await;
    let edited=api.ok(ha,Method::PATCH,&format!("/chat/messages/{mid}"),json!({"requestId":Uuid::new_v4(),"text":"","expectedRevision":"1","expectedConversationRevision":"2","sealed":sealed(*a,&device,cid,mid,2)})).await;
    assert!(edited["historyMigration"].is_null());
    assert_eq!(edited["revision"], "2");
    sqlx::query("UPDATE cloud_conversations SET read_only=true WHERE id=$1")
        .bind(cid)
        .execute(&f.state.pool)
        .await
        .unwrap();
    assert_eq!(
        api.call(
            hb,
            Method::POST,
            "/crypto/history-migrations/preview",
            json!({"conversationId":cid})
        )
        .await
        .0,
        403
    );
    drop(api);
    f.close().await;
}

#[tokio::test]
async fn migrated_materials_require_node_history_capability() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let api = Api::new(&f).await;
    let human = f.users[0].0;
    let ai = Uuid::new_v4();
    let node = Uuid::new_v4();
    let token = "d".repeat(64);
    sqlx::query("INSERT INTO accounts(id,name,kind) VALUES($1,'old node AI','ai')")
        .bind(ai)
        .execute(&f.state.pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO cloud_ai_bindings(ai_id,version,node_id,token_hash,capabilities) VALUES($1,1,$2,$3,'[\"sealedV1\"]')").bind(ai).bind(node).bind(format!("{:x}",Sha256::digest(token.as_bytes()))).execute(&f.state.pool).await.unwrap();
    let cid = conversation(&f, human, ai).await;
    let mid = Uuid::new_v4();
    let migration = Uuid::new_v4();
    let device = Uuid::new_v4().to_string();
    sqlx::query("UPDATE cloud_conversations SET crypto_enabled=true,crypto_history_complete=true WHERE id=$1").bind(cid).execute(&f.state.pool).await.unwrap();
    sqlx::query("INSERT INTO cloud_messages(id,conversation,sender,body,sealed,history_migration) VALUES($1,$2,$3,'',$4,$5)").bind(mid).bind(cid).bind(human).bind(sealed(human,&device,cid,mid,1)).bind(json!({"id":migration,"actorId":human,"sourceDigest":"a".repeat(64)})).execute(&f.state.pool).await.unwrap();
    let tid = Uuid::new_v4();
    sqlx::query("INSERT INTO cloud_node_tasks(id,ai_id,conversation,input_id,input_revision,binding_version,expires_at,trigger) VALUES($1,$2,$3,$4,1,1,now()+interval '1 hour',$5)").bind(tid).bind(ai).bind(cid).bind(mid).bind(json!({"type":"message","messageId":mid,"inputIds":[mid]})).execute(&f.state.pool).await.unwrap();
    let mut headers = HeaderMap::new();
    headers.insert("authorization", format!("Bearer {token}").parse().unwrap());
    let path = format!("/node/tasks/{tid}/materials");
    let rejected = api.call(&headers, Method::GET, &path, Value::Null).await;
    assert_eq!(rejected.0, 409);
    assert_eq!(rejected.1["code"], "history_migration_unsupported");
    sqlx::query("UPDATE cloud_ai_bindings SET capabilities='[\"sealedV1\",\"historyMigrationV1\"]' WHERE ai_id=$1").bind(ai).execute(&f.state.pool).await.unwrap();
    let materials = api.ok(&headers, Method::GET, &path, Value::Null).await;
    assert_eq!(
        materials["messages"][0]["historyMigration"]["id"],
        migration.to_string()
    );
    drop(api);
    f.close().await;
}

#[tokio::test]
async fn history_commit_receipts_survive_device_recovery_and_recheck_current_authority() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let api = Api::new(&f).await;
    let (a, ha) = &f.users[0];
    let (b, hb) = &f.users[1];
    let device = registered(&api, ha, *a).await;
    let group = api
        .ok(
            ha,
            Method::POST,
            "/chat/groups",
            json!({"requestId":Uuid::new_v4(),"name":"receipt recovery","memberIds":[b]}),
        )
        .await;
    let cid: Uuid = serde_json::from_value(group["id"].clone()).unwrap();
    let original = api
        .ok(
            ha,
            Method::POST,
            &format!("/chat/conversations/{cid}/messages"),
            json!({"requestId":Uuid::new_v4(),"text":"old receipt source"}),
        )
        .await;
    let mid: Uuid = serde_json::from_value(original["id"].clone()).unwrap();
    let preview = api
        .ok(
            ha,
            Method::POST,
            "/crypto/history-migrations/preview",
            json!({"conversationId":cid}),
        )
        .await;
    let begin = json!({"requestId":Uuid::new_v4(),"deviceId":device,"conversationId":cid,"expectedConversationRevision":preview["conversationRevision"],"sourceDigest":preview["sourceDigest"],"backupDigest":"a".repeat(64),"recoveryDigest":"b".repeat(64),"localPendingResolved":true});
    let started = api
        .ok(
            ha,
            Method::POST,
            "/crypto/history-migrations",
            begin.clone(),
        )
        .await;
    let id: Uuid = serde_json::from_value(started["id"].clone()).unwrap();
    let entries = api
        .ok(
            ha,
            Method::GET,
            &format!("/crypto/history-migrations/{id}/items"),
            Value::Null,
        )
        .await;
    let mut blob = sealed(*a, &device, cid, mid, 1);
    blob["eventId"] = json!(format!("${mid}.1.migration.{id}"));
    let conversion = json!({"requestId":Uuid::new_v4(),"deviceId":device,"expectedRevision":"1","sourceDigest":entries["items"][0]["sourceDigest"],"sealed":blob});
    let item_path = format!("/crypto/history-migrations/{id}/items/message/{mid}");
    api.ok(ha, Method::PUT, &item_path, conversion.clone())
        .await;
    api.ok(
        ha,
        Method::POST,
        &format!("/crypto/devices/{device}/revoke"),
        json!({}),
    )
    .await;
    assert_eq!(
        api.call(ha, Method::PUT, &item_path, conversion.clone())
            .await
            .0,
        403
    );
    for (operation, command) in [
        ("begin".to_owned(), begin.clone()),
        (format!("{id}:message:{mid}"), conversion),
    ] {
        let path = format!(
            "/crypto/history-migration-receipts/{}",
            command["requestId"].as_str().unwrap()
        );
        let response = api.ok(ha, Method::GET, &path, Value::Null).await;
        let fingerprint = format!(
            "{:x}",
            Sha256::digest(json!([operation, command]).to_string().as_bytes())
        );
        assert_eq!(response["receipt"]["fingerprint"], fingerprint);
        assert_eq!(response["receipt"]["migration"]["id"], id.to_string());
        assert!(api.ok(hb, Method::GET, &path, Value::Null).await["receipt"].is_null());
    }
    assert!(
        api.ok(
            ha,
            Method::GET,
            &format!("/crypto/history-migration-receipts/{}", Uuid::new_v4()),
            Value::Null
        )
        .await["receipt"]
            .is_null()
    );
    let recovered = registered(&api, ha, *a).await;
    let finish = json!({"requestId":Uuid::new_v4(),"deviceId":recovered,"verificationDigest":"c".repeat(64)});
    api.ok(
        ha,
        Method::POST,
        &format!("/crypto/history-migrations/{id}/finish"),
        finish.clone(),
    )
    .await;
    let response = api
        .ok(
            ha,
            Method::GET,
            &format!(
                "/crypto/history-migration-receipts/{}",
                finish["requestId"].as_str().unwrap()
            ),
            Value::Null,
        )
        .await;
    assert_eq!(
        response["receipt"]["fingerprint"],
        format!(
            "{:x}",
            Sha256::digest(
                json!([format!("finish:{id}"), finish])
                    .to_string()
                    .as_bytes()
            )
        )
    );
    assert_eq!(response["receipt"]["migration"]["state"], "complete");
    let preview = api
        .ok(
            ha,
            Method::POST,
            "/crypto/history-migrations/preview",
            json!({"conversationId":cid}),
        )
        .await;
    let next = json!({"requestId":Uuid::new_v4(),"deviceId":recovered,"conversationId":cid,"expectedConversationRevision":preview["conversationRevision"],"sourceDigest":preview["sourceDigest"],"backupDigest":"a".repeat(64),"recoveryDigest":"b".repeat(64),"localPendingResolved":true});
    api.ok(ha, Method::POST, "/crypto/history-migrations", next.clone())
        .await;
    api.ok(ha,Method::PUT,&format!("/chat/groups/{cid}"),json!({"requestId":Uuid::new_v4(),"expectedRevision":"1","name":"membership revision changed"})).await;
    let path = format!(
        "/crypto/history-migration-receipts/{}",
        next["requestId"].as_str().unwrap()
    );
    assert_eq!(
        api.ok(ha, Method::GET, &path, Value::Null).await["receipt"]["migration"]["state"],
        "conflicted"
    );
    api.ok(
        ha,
        Method::POST,
        &format!("/chat/groups/{cid}/owner"),
        json!({"requestId":Uuid::new_v4(),"expectedRevision":"2","accountId":b}),
    )
    .await;
    assert_eq!(api.call(ha, Method::GET, &path, Value::Null).await.0, 403);
    assert!(api.ok(hb, Method::GET, &path, Value::Null).await["receipt"].is_null());
    api.ok(
        hb,
        Method::DELETE,
        &format!("/chat/groups/{cid}/members/{a}"),
        json!({"requestId":Uuid::new_v4(),"expectedRevision":"3"}),
    )
    .await;
    assert_eq!(api.call(ha, Method::GET, &path, Value::Null).await.0, 403);
    drop(api);
    f.close().await;
}
