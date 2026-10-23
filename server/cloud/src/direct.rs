//! Acceptance of already executed same-device conversations; never creates cloud execution tasks.
use crate::{AppError, AppState, Result, auth, chat, consciousness, db as sqlx, nodes};
use axum::{
    Json, Router,
    extract::{Path, State},
    http::HeaderMap,
    routing::post,
};
use connector::direct::*;
use rand::RngCore;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::Row;
use std::sync::Arc;
use uuid::Uuid;
type Tx<'a> = sqlx::Transaction<'a, sqlx::Postgres>;
fn uid(s: &str) -> Result<Uuid> {
    let id: Uuid = s.parse().map_err(|_| AppError::bad("UUID 无效"))?;
    if id.is_nil() {
        return Err(AppError::bad("UUID 无效"));
    }
    Ok(id)
}
fn num(s: &str) -> Result<i64> {
    s.parse().map_err(|_| AppError::bad("版本无效"))
}
fn hash(s: &str) -> String {
    format!("{:x}", Sha256::digest(s.as_bytes()))
}
fn conflict() -> AppError {
    let mut e = AppError::conflict("直连授权、来源或窗口状态已变化");
    e.code = "direct_conflict";
    e
}
pub(crate) fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/chat/conversations/{id}/direct", post(grant))
        .route("/node/direct/prepare", post(prepare))
        .route("/node/direct/sync", post(sync))
}
struct Access {
    human: Uuid,
    ai: Uuid,
    cid: Uuid,
    version: i64,
}
async fn permitted(tx: &mut Tx<'_>, a: &Access, idle: bool) -> Result<()> {
    let members = chat::member(tx, a.cid, a.human).await?;
    chat::writable(tx, a.cid).await?;
    nodes::admin(tx, a.ai, a.human).await?;
    let valid:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM cloud_conversations c JOIN accounts h ON h.id=$2 JOIN accounts a ON a.id=$3 WHERE c.id=$1 AND c.kind='direct' AND h.kind='human' AND h.status='active' AND a.kind='ai' AND a.status='active')").bind(a.cid).bind(a.human).bind(a.ai).fetch_one(&mut **tx).await?;
    if !valid
        || members.len() != 2
        || !members.contains(&a.ai)
        || crate::groups::direct_blocked(tx, a.cid).await?
    {
        return Err(conflict());
    }
    if idle {
        crate::participation::ensure(tx, a.cid).await?;
        let blocked:bool=sqlx::query_scalar("SELECT paused OR $2=ANY(paused_ai) OR EXISTS(SELECT 1 FROM cloud_node_tasks WHERE conversation=$1 AND state IN ('queued','running','uncertain')) FROM cloud_participation WHERE conversation=$1").bind(a.cid).bind(a.ai).fetch_one(&mut **tx).await?;
        if blocked {
            return Err(conflict());
        }
    }
    Ok(())
}
async fn access(tx: &mut Tx<'_>, h: &HeaderMap, token: &str) -> Result<Access> {
    let n = nodes::authenticate(tx, h).await?;
    let r = sqlx::query("SELECT * FROM cloud_direct_grants WHERE token_hash=$1")
        .bind(hash(token))
        .fetch_optional(&mut **tx)
        .await?
        .ok_or_else(AppError::forbidden)?;
    let a = Access {
        human: r.get("human_id"),
        ai: r.get("ai_id"),
        cid: r.get("conversation"),
        version: r.get("binding_version"),
    };
    if n.ai != a.ai || n.node != r.get::<Uuid, _>("node_id") || n.version != a.version {
        return Err(conflict());
    }
    Ok(a)
}
async fn grant(
    State(s): State<Arc<AppState>>,
    h: HeaderMap,
    Path(cid): Path<Uuid>,
    Json(v): Json<DirectGrantRequest>,
) -> Result<Json<DirectGrant>> {
    let p = auth::authenticate(&s, &h).await?;
    let mut tx = s.pool.begin().await?;
    nodes::lock(&mut tx).await?;
    let members = chat::member(&mut tx, cid, p.id).await?;
    let ai = *members
        .iter()
        .find(|&&id| id != p.id)
        .ok_or_else(AppError::forbidden)?;
    let version = num(&v.binding_version)?;
    let a = Access {
        human: p.id,
        ai,
        cid,
        version,
    };
    permitted(&mut tx, &a, true).await?;
    let valid:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM cloud_ai_bindings WHERE ai_id=$1 AND node_id=$2 AND version=$3 AND token_hash IS NOT NULL)").bind(ai).bind(uid(&v.node_id)?).bind(version).fetch_one(&mut *tx).await?;
    if !valid {
        return Err(conflict());
    }
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    let token = bytes.iter().map(|x| format!("{x:02x}")).collect::<String>();
    sqlx::query("INSERT INTO cloud_direct_grants VALUES($1,$2,$3,$4,$5,$6)")
        .bind(hash(&token))
        .bind(p.id)
        .bind(ai)
        .bind(cid)
        .bind(uid(&v.node_id)?)
        .bind(version)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(Json(DirectGrant { token }))
}
async fn head(tx: &mut Tx<'_>, cid: Uuid) -> Result<Option<DirectHead>> {
    Ok(sqlx::query("SELECT id,revision,position FROM cloud_messages WHERE conversation=$1 ORDER BY position DESC LIMIT 1").bind(cid).fetch_optional(&mut **tx).await?.map(|r|DirectHead{message_id:r.get::<Uuid,_>("id").to_string(),revision:r.get::<i64,_>("revision").to_string(),position:r.get::<i64,_>("position").to_string()}))
}
async fn prepare(
    State(s): State<Arc<AppState>>,
    h: HeaderMap,
    Json(v): Json<DirectPrepare>,
) -> Result<Json<DirectBootstrap>> {
    let mut tx = s.pool.begin().await?;
    nodes::lock(&mut tx).await?;
    let a = access(&mut tx, &h, &v.grant).await?;
    permitted(&mut tx, &a, true).await?;
    let mut rows=sqlx::query("SELECT * FROM cloud_messages WHERE conversation=$1 AND status IN ('committed','failed','interrupted') AND NOT withdrawn ORDER BY position DESC LIMIT 65").bind(a.cid).fetch_all(&mut *tx).await?;
    let has_more = rows.len() > 64;
    rows.truncate(64);
    rows.reverse();
    if rows.iter().any(|r| {
        !r.get::<Value, _>("attachments")
            .as_array()
            .unwrap()
            .is_empty()
            || r.get::<Option<Value>, _>("context_reference").is_some()
    }) {
        return Err(AppError::bad(
            "首批直连仅支持无附件、无跨窗口引用的文字历史",
        ));
    }
    let out = DirectBootstrap {
        ai: serde_json::from_value(chat::account(&mut tx, a.ai).await?).unwrap(),
        human: serde_json::from_value(chat::account(&mut tx, a.human).await?).unwrap(),
        conversation: serde_json::from_value(
            consciousness::node_conversation(&mut tx, a.cid).await?,
        )
        .unwrap(),
        binding_version: a.version.to_string(),
        base: head(&mut tx, a.cid).await?,
        messages: rows
            .iter()
            .map(|r| serde_json::from_value(consciousness::node_message(r)).unwrap())
            .collect(),
        message_statuses: rows
            .iter()
            .map(|r| {
                (
                    r.get::<Uuid, _>("id").to_string(),
                    r.get::<String, _>("status"),
                )
            })
            .collect(),
        has_more,
    };
    tx.commit().await?;
    Ok(Json(out))
}
async fn receipt(tx: &mut Tx<'_>, cid: Uuid, ids: Vec<Uuid>) -> Result<DirectSyncReceipt> {
    let mut messages = vec![];
    for id in ids {
        let r = sqlx::query("SELECT * FROM cloud_messages WHERE id=$1 AND conversation=$2")
            .bind(id)
            .bind(cid)
            .fetch_one(&mut **tx)
            .await?;
        messages.push(DirectSyncedMessage {
            message: serde_json::from_value(consciousness::node_message(&r)).unwrap(),
            status: if r.get::<bool, _>("withdrawn") {
                "withdrawn".into()
            } else {
                r.get("status")
            },
        });
    }
    // A replay acknowledges this batch, not later cloud messages the node has not read.
    let head = messages.last().map(|item| DirectHead {
        message_id: item.message.id.clone(),
        revision: item.message.revision.clone(),
        position: item.message.position.clone(),
    });
    Ok(DirectSyncReceipt { messages, head })
}
async fn sync(
    State(s): State<Arc<AppState>>,
    h: HeaderMap,
    Json(v): Json<DirectSyncRequest>,
) -> Result<Json<DirectSyncReceipt>> {
    let rid = uid(&v.request_id)?;
    let fp = hash(&serde_json::to_string(&v).unwrap());
    let mut tx = s.pool.begin().await?;
    nodes::lock(&mut tx).await?;
    let a = access(&mut tx, &h, &v.grant).await?;
    if let Some(r) =
        sqlx::query("SELECT * FROM cloud_direct_receipts WHERE ai_id=$1 AND request_id=$2")
            .bind(a.ai)
            .bind(rid)
            .fetch_optional(&mut *tx)
            .await?
    {
        permitted(&mut tx, &a, false).await?;
        if r.get::<String, _>("fingerprint") != fp {
            return Err(conflict());
        }
        return Ok(Json(receipt(&mut tx, a.cid, r.get("message_ids")).await?));
    }
    permitted(&mut tx, &a, true).await?;
    let revision: i64 = sqlx::query_scalar("SELECT revision FROM cloud_conversations WHERE id=$1")
        .bind(a.cid)
        .fetch_one(&mut *tx)
        .await?;
    if revision != num(&v.conversation_revision)? || head(&mut tx, a.cid).await? != v.base {
        return Err(conflict());
    }
    if v.messages.is_empty()
        || v.messages.len() > 64
        || v.sources.len() > 64
        || v.configuration_revision.is_empty()
        || v.configuration_revision.len() > 256
    {
        return Err(AppError::bad("直连批次或配置标识无效"));
    }
    for source in &v.sources {
        let valid:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM cloud_messages WHERE id=$1 AND conversation=$2 AND revision=$3 AND NOT withdrawn AND status='committed')").bind(uid(&source.message_id)?).bind(a.cid).bind(num(&source.revision)?).fetch_one(&mut *tx).await?;
        if !valid {
            return Err(conflict());
        }
    }
    if v.derived_sources
        .iter()
        .any(|x| x.conversation_id != a.cid.to_string())
        || v.material_scopes
            .iter()
            .any(|x| x.conversation_id != a.cid.to_string())
        || !consciousness::scoped_valid(&mut tx, a.ai, &v.derived_sources).await?
        || !consciousness::scopes_valid(&mut tx, a.ai, &v.material_scopes).await?
    {
        return Err(conflict());
    }
    let mut ids = vec![];
    let mut previous = None;
    let mut awaiting = false;
    let mut seen = std::collections::HashSet::new();
    for m in &v.messages {
        let mid = uid(&m.id)?;
        if !seen.insert(mid) {
            return Err(AppError::bad("消息 ID 重复"));
        }
        let sender = uid(&m.sender_id)?;
        let human = sender == a.human;
        if (sender != a.ai && !human)
            || human == awaiting
            || m.text.len() > 256 * 1024
            || (human || m.status == "committed") && m.text.trim().is_empty()
            || !matches!(m.status.as_str(), "committed" | "failed" | "interrupted")
            || human
                && (m.status != "committed"
                    || m.run_id.is_some()
                    || m.usage.is_some()
                    || m.error.is_some())
            || !human && m.run_id.is_none()
            || m.error.as_ref().is_some_and(|x| x.len() > 256)
        {
            return Err(AppError::bad("直连消息顺序、执行标识或正文无效"));
        }
        let created = m
            .created_at
            .parse::<chrono::DateTime<chrono::Utc>>()
            .map_err(|_| AppError::bad("消息时间无效"))?;
        let reply = m.reply_to.as_deref().map(uid).transpose()?;
        if reply.is_some() && reply != previous {
            return Err(AppError::bad("直连回复只能引用前一条消息"));
        }
        if sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT 1 FROM cloud_messages WHERE id=$1)")
            .bind(mid)
            .fetch_one(&mut *tx)
            .await?
        {
            return Err(conflict());
        }
        sqlx::query("INSERT INTO cloud_messages(id,conversation,sender,body,status,created_at,updated_at,sender_name,reply_to) VALUES($1,$2,$3,$4,$5,$6,$6,(SELECT name FROM accounts WHERE id=$3),$7)").bind(mid).bind(a.cid).bind(sender).bind(&m.text).bind(&m.status).bind(created).bind(reply).execute(&mut *tx).await?;
        if let Some(run) = &m.run_id {
            let run = uid(run)?;
            if sqlx::query_scalar::<_,bool>("SELECT EXISTS(SELECT 1 FROM cloud_direct_runs WHERE run_id=$1 UNION ALL SELECT 1 FROM cloud_node_tasks WHERE run_id=$1)").bind(run).fetch_one(&mut *tx).await? {return Err(conflict());}
            sqlx::query("INSERT INTO cloud_direct_runs VALUES($1,$2,$3,$4,$5,$6,$7)")
                .bind(run)
                .bind(mid)
                .bind(a.ai)
                .bind(a.version)
                .bind(&v.configuration_revision)
                .bind(json!(m.usage))
                .bind(&m.error)
                .execute(&mut *tx)
                .await?;
        }
        ids.push(mid);
        previous = Some(mid);
        awaiting = human;
    }
    sqlx::query("INSERT INTO cloud_direct_receipts VALUES($1,$2,$3,$4)")
        .bind(a.ai)
        .bind(rid)
        .bind(fp)
        .bind(&ids)
        .execute(&mut *tx)
        .await?;
    chat::notify(&mut tx, a.cid, vec![a.ai, a.human]).await?;
    let out = receipt(&mut tx, a.cid, ids).await?;
    tx.commit().await?;
    Ok(Json(out))
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat::tests::Fixture;
    async fn setup(f: &Fixture) -> (Uuid, Uuid, HeaderMap, DirectGrant) {
        let ai = Uuid::new_v4();
        let node = Uuid::new_v4();
        let cid = Uuid::new_v4();
        let human = f.users[0].0;
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
        sqlx::query(
            "INSERT INTO cloud_ai_bindings(ai_id,version,node_id,token_hash) VALUES($1,1,$2,$3)",
        )
        .bind(ai)
        .bind(node)
        .bind(hash(&"a".repeat(64)))
        .execute(&f.state.pool)
        .await
        .unwrap();
        let (first, second) = if human < ai { (human, ai) } else { (ai, human) };
        sqlx::query("INSERT INTO cloud_conversations(id,first_id,second_id) VALUES($1,$2,$3)")
            .bind(cid)
            .bind(first)
            .bind(second)
            .execute(&f.state.pool)
            .await
            .unwrap();
        for id in [human, ai] {
            sqlx::query("INSERT INTO cloud_members(conversation,account_id) VALUES($1,$2)")
                .bind(cid)
                .bind(id)
                .execute(&f.state.pool)
                .await
                .unwrap();
        }
        let req = DirectGrantRequest {
            node_id: node.to_string(),
            binding_version: "1".into(),
        };
        assert!(
            grant(
                State(f.state.clone()),
                f.users[1].1.clone(),
                Path(cid),
                Json(req.clone())
            )
            .await
            .is_err()
        );
        let g = grant(
            State(f.state.clone()),
            f.users[0].1.clone(),
            Path(cid),
            Json(req),
        )
        .await
        .unwrap()
        .0;
        let mut h = HeaderMap::new();
        h.insert(
            "authorization",
            format!("Bearer {}", "a".repeat(64)).parse().unwrap(),
        );
        (cid, ai, h, g)
    }
    fn batch(f: &Fixture, ai: Uuid, g: &DirectGrant) -> DirectSyncRequest {
        DirectSyncRequest {
            request_id: Uuid::new_v4().to_string(),
            grant: g.token.clone(),
            conversation_revision: "1".into(),
            base: None,
            sources: vec![],
            derived_sources: vec![],
            material_scopes: vec![],
            configuration_revision: "local-config-1".into(),
            messages: vec![
                DirectMessage {
                    id: Uuid::new_v4().to_string(),
                    sender_id: f.users[0].0.to_string(),
                    text: "hello".into(),
                    status: "committed".into(),
                    created_at: chrono::Utc::now().to_rfc3339(),
                    reply_to: None,
                    run_id: None,
                    usage: None,
                    error: None,
                },
                DirectMessage {
                    id: Uuid::new_v4().to_string(),
                    sender_id: ai.to_string(),
                    text: "reply".into(),
                    status: "committed".into(),
                    created_at: chrono::Utc::now().to_rfc3339(),
                    reply_to: None,
                    run_id: Some(Uuid::new_v4().to_string()),
                    usage: None,
                    error: None,
                },
            ],
        }
    }
    #[tokio::test]
    async fn direct_authorization_idempotency_withdraw_and_no_tasks() {
        let Some(f) = Fixture::new().await else {
            return;
        };
        let (cid, ai, h, g) = setup(&f).await;
        let b = prepare(
            State(f.state.clone()),
            h.clone(),
            Json(DirectPrepare {
                grant: g.token.clone(),
            }),
        )
        .await
        .unwrap()
        .0;
        assert_eq!(b.conversation.id, cid.to_string());
        let v = batch(&f, ai, &g);
        let first = sync(State(f.state.clone()), h.clone(), Json(v.clone()))
            .await
            .unwrap()
            .0;
        assert_eq!(first.messages.len(), 2);
        let replay = sync(State(f.state.clone()), h.clone(), Json(v.clone()))
            .await
            .unwrap()
            .0;
        assert_eq!(replay.messages[0].message.id, first.messages[0].message.id);
        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM cloud_node_tasks")
            .fetch_one(&f.state.pool)
            .await
            .unwrap();
        assert_eq!(count, 0);
        let mid = uid(&v.messages[0].id).unwrap();
        sqlx::query(
            "UPDATE cloud_messages SET body='',withdrawn=true,revision=revision+1 WHERE id=$1",
        )
        .bind(mid)
        .execute(&f.state.pool)
        .await
        .unwrap();
        let replay = sync(State(f.state.clone()), h.clone(), Json(v.clone()))
            .await
            .unwrap()
            .0;
        assert_eq!(replay.messages[0].status, "withdrawn");
        assert!(replay.messages[0].message.text.is_empty());
        let mut other = v.clone();
        other.messages[0].text = "changed".into();
        assert!(
            sync(State(f.state.clone()), h.clone(), Json(other))
                .await
                .is_err()
        );
        sqlx::query("UPDATE cloud_ai_bindings SET version=2 WHERE ai_id=$1")
            .bind(ai)
            .execute(&f.state.pool)
            .await
            .unwrap();
        assert!(sync(State(f.state.clone()), h, Json(v)).await.is_err());
        f.close().await;
    }
    #[tokio::test]
    async fn replay_head_does_not_acknowledge_unread_remote_messages() {
        let Some(f) = Fixture::new().await else {
            return;
        };
        let (cid, ai, h, g) = setup(&f).await;
        let v = batch(&f, ai, &g);
        let first = sync(State(f.state.clone()), h.clone(), Json(v.clone()))
            .await
            .unwrap()
            .0;
        let remote = Uuid::new_v4();
        sqlx::query("INSERT INTO cloud_messages(id,conversation,sender,body) VALUES($1,$2,$3,'remote input')").bind(remote).bind(cid).bind(f.users[0].0).execute(&f.state.pool).await.unwrap();
        let replay = sync(State(f.state.clone()), h.clone(), Json(v))
            .await
            .unwrap()
            .0;
        assert_eq!(replay.head, first.head);
        assert_ne!(replay.head.as_ref().unwrap().message_id, remote.to_string());
        let mut next = batch(&f, ai, &g);
        next.base = replay.head;
        let error = sync(State(f.state.clone()), h, Json(next))
            .await
            .err()
            .unwrap();
        assert_eq!(error.status, axum::http::StatusCode::CONFLICT);
        assert_eq!(error.code, "direct_conflict");
        f.close().await;
    }
    #[tokio::test]
    async fn direct_prepare_preserves_failed_interrupted_and_excludes_withdrawn() {
        let Some(f) = Fixture::new().await else {
            return;
        };
        let (cid, ai, h, g) = setup(&f).await;
        let failed = Uuid::new_v4();
        let interrupted = Uuid::new_v4();
        let withdrawn = Uuid::new_v4();
        for (id, status, removed) in [
            (failed, "failed", false),
            (interrupted, "interrupted", false),
            (withdrawn, "committed", true),
        ] {
            sqlx::query("INSERT INTO cloud_messages(id,conversation,sender,body,status,withdrawn) VALUES($1,$2,$3,'partial',$4,$5)").bind(id).bind(cid).bind(ai).bind(status).bind(removed).execute(&f.state.pool).await.unwrap();
        }
        let out = prepare(
            State(f.state.clone()),
            h,
            Json(DirectPrepare { grant: g.token }),
        )
        .await
        .unwrap()
        .0;
        assert_eq!(out.messages.len(), 2);
        assert_eq!(out.message_statuses[&failed.to_string()], "failed");
        assert_eq!(
            out.message_statuses[&interrupted.to_string()],
            "interrupted"
        );
        assert!(!out.message_statuses.contains_key(&withdrawn.to_string()));
        assert_eq!(out.base.unwrap().message_id, withdrawn.to_string());
        f.close().await;
    }
    #[tokio::test]
    async fn direct_preserves_empty_failed_ai_without_cloud_retry() {
        let Some(f) = Fixture::new().await else {
            return;
        };
        let (_, ai, h, g) = setup(&f).await;
        let mut v = batch(&f, ai, &g);
        v.messages[1].text.clear();
        v.messages[1].status = "failed".into();
        v.messages[1].error = Some("provider_failed".into());
        let out = sync(State(f.state.clone()), h.clone(), Json(v.clone()))
            .await
            .unwrap()
            .0;
        assert_eq!(out.messages[1].status, "failed");
        assert!(out.messages[1].message.text.is_empty());
        assert_eq!(
            sync(State(f.state.clone()), h, Json(v))
                .await
                .unwrap()
                .0
                .messages
                .len(),
            2
        );
        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM cloud_node_tasks")
            .fetch_one(&f.state.pool)
            .await
            .unwrap();
        assert_eq!(count, 0);
        f.close().await;
    }
    #[tokio::test]
    async fn direct_rejects_changed_tail_paused_sources_and_busy() {
        let Some(f) = Fixture::new().await else {
            return;
        };
        let (cid, ai, h, g) = setup(&f).await;
        let _ = prepare(
            State(f.state.clone()),
            h.clone(),
            Json(DirectPrepare {
                grant: g.token.clone(),
            }),
        )
        .await
        .unwrap();
        let mut v = batch(&f, ai, &g);
        sqlx::query("UPDATE cloud_participation SET paused=true WHERE conversation=$1")
            .bind(cid)
            .execute(&f.state.pool)
            .await
            .unwrap();
        assert!(
            sync(State(f.state.clone()), h.clone(), Json(v.clone()))
                .await
                .is_err()
        );
        sqlx::query("UPDATE cloud_participation SET paused=false WHERE conversation=$1")
            .bind(cid)
            .execute(&f.state.pool)
            .await
            .unwrap();
        let source = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO cloud_messages(id,conversation,sender,body) VALUES($1,$2,$3,'online')",
        )
        .bind(source)
        .bind(cid)
        .bind(f.users[0].0)
        .execute(&f.state.pool)
        .await
        .unwrap();
        assert!(
            sync(State(f.state.clone()), h.clone(), Json(v.clone()))
                .await
                .is_err()
        );
        let mut tx = f.state.pool.begin().await.unwrap();
        v.base = head(&mut tx, cid).await.unwrap();
        tx.commit().await.unwrap();
        v.sources = vec![connector::node::SourceRevision {
            message_id: source.to_string(),
            revision: "1".into(),
        }];
        sqlx::query("UPDATE cloud_messages SET revision=2 WHERE id=$1")
            .bind(source)
            .execute(&f.state.pool)
            .await
            .unwrap();
        let mut tx = f.state.pool.begin().await.unwrap();
        v.base = head(&mut tx, cid).await.unwrap();
        tx.commit().await.unwrap();
        assert!(
            sync(State(f.state.clone()), h.clone(), Json(v.clone()))
                .await
                .is_err()
        );
        v.sources.clear();
        v.material_scopes = vec![connector::node::MaterialScope {
            conversation_id: cid.to_string(),
            revision: Uuid::new_v4().to_string(),
        }];
        assert!(
            sync(State(f.state.clone()), h.clone(), Json(v.clone()))
                .await
                .is_err()
        );
        v.material_scopes.clear();
        sqlx::query("INSERT INTO cloud_node_tasks(id,ai_id,conversation,input_id,input_revision,binding_version,expires_at,state) VALUES($1,$2,$3,$4,2,1,now()+interval '1 hour','uncertain')").bind(Uuid::new_v4()).bind(ai).bind(cid).bind(source).execute(&f.state.pool).await.unwrap();
        assert!(sync(State(f.state.clone()), h, Json(v)).await.is_err());
        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM cloud_direct_receipts")
            .fetch_one(&f.state.pool)
            .await
            .unwrap();
        assert_eq!(count, 0);
        f.close().await;
    }
}
