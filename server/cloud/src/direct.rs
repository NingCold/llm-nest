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
        .route("/node/direct/{aiId}/reference", post(reference))
        .route("/node/direct/{aiId}/attachment", post(attachment))
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
    crate::history_migration::frozen(tx, a.cid).await?;
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
    if s.require_encryption && !crate::crypto::enabled(&mut tx, a.cid).await? {
        return Err(crate::crypto::required());
    }
    crate::handover::active(&mut tx, a.ai).await?;
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
    if s.require_encryption && !crate::crypto::enabled(&mut tx, a.cid).await? {
        return Err(crate::crypto::required());
    }
    crate::handover::active(&mut tx, a.ai).await?;
    crate::history_migration::node_capability(&mut tx, a.ai, a.version, a.cid).await?;
    let mut rows=sqlx::query("SELECT * FROM cloud_messages WHERE conversation=$1 AND status IN ('committed','failed','interrupted') AND NOT withdrawn ORDER BY position DESC LIMIT 65").bind(a.cid).fetch_all(&mut *tx).await?;
    let has_more = rows.len() > 64;
    rows.truncate(64);
    rows.reverse();
    let mut references = Vec::new();
    for r in rows.iter().rev().take(12) {
        if let Some(reference) = r.get::<Option<Value>, _>("context_reference")
            && reference["stale"] == false
        {
            let selection = serde_json::from_value(reference).map_err(|_| AppError::internal())?;
            let mut value =
                consciousness::direct_reference(&mut tx, a.cid, a.human, r.get("id"), &selection)
                    .await?;
            crate::history_migration::node_capability(
                &mut tx,
                a.ai,
                a.version,
                uid(value["conversation"]["id"].as_str().unwrap())?,
            )
            .await?;
            value["required"] = json!(false);
            references.push(serde_json::from_value(value).map_err(|_| AppError::internal())?);
        }
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
        references,
    };
    tx.commit().await?;
    Ok(Json(out))
}
async fn reference(
    State(s): State<Arc<AppState>>,
    h: HeaderMap,
    Path(ai): Path<Uuid>,
    Json(v): Json<DirectReferenceRequest>,
) -> Result<Json<connector::node::NodeReference>> {
    let mut tx = s.pool.begin().await?;
    nodes::lock(&mut tx).await?;
    let a = access(&mut tx, &h, &v.grant).await?;
    if ai != a.ai {
        return Err(AppError::forbidden());
    }
    permitted(&mut tx, &a, false).await?;
    let value =
        consciousness::direct_reference(&mut tx, a.cid, a.human, Uuid::nil(), &v.selection).await?;
    crate::history_migration::node_capability(
        &mut tx,
        a.ai,
        a.version,
        uid(&v.selection.conversation_id)?,
    )
    .await?;
    tx.commit().await?;
    Ok(Json(
        serde_json::from_value(value).map_err(|_| AppError::internal())?,
    ))
}
async fn common_scope(tx: &mut Tx<'_>, a: &Access, cid: Uuid) -> Result<()> {
    if cid != a.cid {
        let kind: String = sqlx::query_scalar("SELECT kind FROM cloud_conversations WHERE id=$1")
            .bind(cid)
            .fetch_one(&mut **tx)
            .await?;
        if kind != "group" {
            return Err(AppError::forbidden());
        }
        let members = chat::member(tx, cid, a.human).await?;
        if !members.contains(&a.ai) {
            return Err(AppError::forbidden());
        }
    }
    Ok(())
}
async fn attachment(
    State(s): State<Arc<AppState>>,
    h: HeaderMap,
    Path(ai): Path<Uuid>,
    Json(v): Json<Value>,
) -> Result<axum::response::Response> {
    let mut tx = s.pool.begin().await?;
    nodes::lock(&mut tx).await?;
    let a = access(
        &mut tx,
        &h,
        v["grant"].as_str().ok_or_else(AppError::forbidden)?,
    )
    .await?;
    if ai != a.ai {
        return Err(AppError::forbidden());
    }
    permitted(&mut tx, &a, false).await?;
    let cid = uid(v["conversationId"]
        .as_str()
        .ok_or_else(|| AppError::bad("窗口无效"))?)?;
    common_scope(&mut tx, &a, cid).await?;
    crate::history_migration::node_capability(&mut tx, a.ai, a.version, cid).await?;
    let mid = uid(v["messageId"]
        .as_str()
        .ok_or_else(|| AppError::bad("消息无效"))?)?;
    let index = v["index"]
        .as_u64()
        .ok_or_else(|| AppError::bad("附件索引无效"))? as usize;
    let row=sqlx::query("SELECT * FROM cloud_messages WHERE id=$1 AND conversation=$2 AND NOT withdrawn AND status='committed'").bind(mid).bind(cid).fetch_optional(&mut *tx).await?.ok_or_else(AppError::not_found)?;
    if v["revision"].as_str() != Some(row.get::<i64, _>("revision").to_string().as_str()) {
        return Err(conflict());
    }
    let parts: Value = row.get("attachments");
    if parts.get(index).is_none() {
        return Err(AppError::not_found());
    }
    let bytes = tokio::fs::read(
        crate::history_migration::attachment_path(&mut tx, &s.attachments, mid, index).await?,
    )
    .await?;
    tx.commit().await?;
    Ok(chat::binary(
        bytes,
        "application/octet-stream",
        "attachment",
        false,
    ))
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
    if s.require_encryption && v.messages.iter().any(|m| m.sealed.is_none()) {
        return Err(crate::crypto::required());
    }
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
    let ready: bool =
        sqlx::query_scalar("SELECT handover_state='ready' FROM cloud_ai_bindings WHERE ai_id=$1")
            .bind(a.ai)
            .fetch_one(&mut *tx)
            .await?;
    if ready {
        return Err(AppError::conflict("节点已声明移交就绪，不能新增补传"));
    }
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
    for cid in v
        .derived_sources
        .iter()
        .map(|x| &x.conversation_id)
        .chain(v.material_scopes.iter().map(|x| &x.conversation_id))
    {
        common_scope(&mut tx, &a, uid(cid)?).await?;
    }
    if !consciousness::scoped_valid(&mut tx, a.ai, &v.derived_sources).await?
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
            || (human || m.status == "committed") && m.text.trim().is_empty() && m.sealed.is_none()
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
        let actor = crate::crypto::Actor {
            id: sender,
            node: if human {
                None
            } else {
                Some(crate::nodes::authenticate(&mut tx, &h).await?)
            },
        };
        crate::crypto::content(
            &mut tx,
            a.cid,
            &actor,
            mid,
            1,
            None,
            &m.text,
            m.sealed.as_ref(),
        )
        .await?;
        if m.sealed.is_some()
            && m.audience_revision
                .as_deref()
                .is_some_and(|revision| revision != v.conversation_revision)
        {
            return Err(conflict());
        }
        let mentions = m
            .mentions
            .iter()
            .map(|x| uid(x))
            .collect::<Result<Vec<_>>>()?;
        let pair = chat::member(&mut tx, a.cid, a.human).await?;
        crate::groups::validate_send(&mut tx, a.cid, &pair, None, &mentions).await?;
        if !m.attachments.is_empty() && m.encrypted_attachments.is_empty() {
            return Err(AppError::bad("补传附件缺少密文字节"));
        }
        if !m.encrypted_attachments.is_empty() && m.sealed.is_none() {
            return Err(AppError::bad("附件必须加密"));
        }
        chat::unoccupied_message(&mut tx, mid).await?;
        let attachments =
            chat::save_encrypted_uploads(&s.attachments, mid, &m.encrypted_attachments).await?;
        let created = m
            .created_at
            .parse::<chrono::DateTime<chrono::Utc>>()
            .map_err(|_| AppError::bad("消息时间无效"))?;
        let reply = m.reply_to.as_deref().map(uid).transpose()?;
        if !human && reply.is_some() && reply != previous {
            return Err(AppError::bad("AI回复只能引用紧前真人消息"));
        }
        if human {
            crate::groups::validate_send(&mut tx, a.cid, &pair, reply, &[]).await?;
        }
        sqlx::query("INSERT INTO cloud_messages(id,conversation,sender,body,status,created_at,updated_at,sender_name,reply_to,sealed,attachments,mentions,crypto_audience_revision) VALUES($1,$2,$3,$4,$5,$6,$6,(SELECT name FROM accounts WHERE id=$3),$7,$8,$9,$10,$11)").bind(mid).bind(a.cid).bind(sender).bind(&m.text).bind(&m.status).bind(created).bind(reply).bind(m.sealed.as_ref().map(|e|json!(e))).bind(attachments).bind(mentions).bind(m.audience_revision.as_deref().map(num).transpose()?).execute(&mut *tx).await?;
        if let Some(reference) = &m.context_reference {
            if reference.stale {
                return Err(conflict());
            }
            let selection = connector::node::ReferenceSelection {
                conversation_id: reference.conversation_id.clone(),
                sources: reference.sources.clone(),
            };
            consciousness::save_reference(&mut tx, a.cid, mid, a.human, &selection).await?;
        }
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
                    sealed: None,
                    audience_revision: None,
                    context_reference: None,
                    mentions: vec![],
                    attachments: vec![],
                    encrypted_attachments: vec![],
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
                    sealed: None,
                    audience_revision: None,
                    context_reference: None,
                    mentions: vec![],
                    attachments: vec![],
                    encrypted_attachments: vec![],
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
    #[tokio::test]
    async fn direct_reference_and_attachment_require_current_shared_group() {
        let Some(f) = Fixture::new().await else {
            return;
        };
        let (_, ai, h, g) = setup(&f).await;
        let group = Uuid::new_v4();
        let mid = Uuid::new_v4();
        sqlx::query("INSERT INTO cloud_conversations(id,kind,name) VALUES($1,'group','shared')")
            .bind(group)
            .execute(&f.state.pool)
            .await
            .unwrap();
        for account in [f.users[0].0, ai] {
            sqlx::query("INSERT INTO cloud_members(conversation,account_id) VALUES($1,$2)")
                .bind(group)
                .bind(account)
                .execute(&f.state.pool)
                .await
                .unwrap();
        }
        let upload = connector::crypto::EncryptedAttachment {
            index: 0,
            data: "Y2lwaGVydGV4dA==".into(),
        };
        let parts = chat::save_encrypted_uploads(&f.state.attachments, mid, &[upload])
            .await
            .unwrap();
        sqlx::query("INSERT INTO cloud_messages(id,conversation,sender,body,attachments) VALUES($1,$2,$3,'',$4)").bind(mid).bind(group).bind(f.users[0].0).bind(parts).execute(&f.state.pool).await.unwrap();
        let selection = connector::node::ReferenceSelection {
            conversation_id: group.to_string(),
            sources: vec![connector::node::SourceRevision {
                message_id: mid.to_string(),
                revision: "1".into(),
            }],
        };
        let request = DirectReferenceRequest {
            grant: g.token.clone(),
            selection,
        };
        let preview = reference(
            State(f.state.clone()),
            h.clone(),
            Path(ai),
            Json(request.clone()),
        )
        .await
        .unwrap()
        .0;
        assert_eq!(preview.messages[0].attachments[0].kind, "encrypted");
        let download = json!({"grant":g.token,"conversationId":group,"messageId":mid,"index":0,"revision":"1"});
        assert!(
            attachment(
                State(f.state.clone()),
                h.clone(),
                Path(ai),
                Json(download.clone())
            )
            .await
            .is_ok()
        );
        let mut stale = download.clone();
        stale["revision"] = json!("2");
        assert!(
            attachment(State(f.state.clone()), h.clone(), Path(ai), Json(stale))
                .await
                .is_err()
        );
        sqlx::query("DELETE FROM cloud_members WHERE conversation=$1 AND account_id=$2")
            .bind(group)
            .bind(f.users[0].0)
            .execute(&f.state.pool)
            .await
            .unwrap();
        assert!(
            reference(State(f.state.clone()), h.clone(), Path(ai), Json(request))
                .await
                .is_err()
        );
        assert!(
            attachment(State(f.state.clone()), h, Path(ai), Json(download))
                .await
                .is_err()
        );
        f.close().await;
    }
    #[tokio::test]
    async fn legacy_private_sealed_outbox_keeps_missing_audience_revision() {
        let Some(f) = Fixture::new().await else {
            return;
        };
        let (cid, ai, h, g) = setup(&f).await;
        sqlx::query("UPDATE cloud_conversations SET crypto_enabled=true WHERE id=$1")
            .bind(cid)
            .execute(&f.state.pool)
            .await
            .unwrap();
        let node: Uuid = sqlx::query_scalar("SELECT node_id FROM cloud_ai_bindings WHERE ai_id=$1")
            .bind(ai)
            .fetch_one(&f.state.pool)
            .await
            .unwrap();
        let mut request = batch(&f, ai, &g);
        for (index, m) in request.messages.iter_mut().enumerate() {
            let device = format!("legacy-{index}");
            let sender = uid(&m.sender_id).unwrap();
            sqlx::query("INSERT INTO cloud_crypto_devices(account_id,device_id,node_id,binding_version,device_keys) VALUES($1,$2,$3,$4,'{}')").bind(sender).bind(&device).bind((sender==ai).then_some(node)).bind((sender==ai).then_some(1i64)).execute(&f.state.pool).await.unwrap();
            m.text.clear();
            m.sealed=Some(serde_json::from_value(json!({"version":1,"roomId":format!("!{cid}:llmn"),"sender":format!("@{sender}:llmn"),"deviceId":device,"eventId":format!("${}.1.final",m.id),"deviceKeys":{"ed25519":"public","curve25519":"public"},"signatures":{"account":{"device":"signature"}},"content":{"algorithm":"m.megolm.v1.aes-sha2","ciphertext":"legacy-frozen-ciphertext"}})).unwrap());
        }
        let mut wrong = request.clone();
        wrong.messages[0].audience_revision = Some("0".into());
        assert!(
            sync(State(f.state.clone()), h.clone(), Json(wrong))
                .await
                .is_err()
        );
        let result = sync(State(f.state.clone()), h, Json(request.clone()))
            .await
            .unwrap()
            .0;
        assert_eq!(result.messages.len(), 2);
        for (message, original) in result.messages.iter().zip(&request.messages) {
            assert!(message.message.audience_revision.is_none());
            assert_eq!(message.message.sealed, original.sealed);
        }
        f.close().await;
    }
    #[tokio::test]
    async fn human_direct_replies_accept_older_history_and_earlier_batch_messages() {
        let Some(f) = Fixture::new().await else {
            return;
        };
        let (cid, ai, h, g) = setup(&f).await;
        let first = batch(&f, ai, &g);
        let old = first.messages[0].id.clone();
        let receipt = sync(State(f.state.clone()), h.clone(), Json(first))
            .await
            .unwrap()
            .0;
        let mut second = batch(&f, ai, &g);
        second.base = receipt.head;
        second.messages[0].reply_to = Some(old.clone());
        second.messages[1].reply_to = Some(old.clone());
        assert!(
            sync(State(f.state.clone()), h.clone(), Json(second.clone()))
                .await
                .is_err()
        );
        second.messages[1].reply_to = Some(second.messages[0].id.clone());
        let mut third = batch(&f, ai, &g);
        third.messages[0].reply_to = Some(second.messages[0].id.clone());
        third.messages[1].reply_to = Some(third.messages[0].id.clone());
        second.messages.extend(third.messages);
        let receipt = sync(State(f.state.clone()), h.clone(), Json(second))
            .await
            .unwrap()
            .0;
        assert_eq!(receipt.messages.len(), 4);
        sqlx::query("UPDATE cloud_messages SET withdrawn=true WHERE id=$1")
            .bind(uid(&old).unwrap())
            .execute(&f.state.pool)
            .await
            .unwrap();
        let mut rejected = batch(&f, ai, &g);
        rejected.base = receipt.head;
        rejected.messages[0].reply_to = Some(old);
        assert!(
            sync(State(f.state.clone()), h, Json(rejected))
                .await
                .is_err()
        );
        let count: i64 =
            sqlx::query_scalar("SELECT count(*) FROM cloud_messages WHERE conversation=$1")
                .bind(cid)
                .fetch_one(&f.state.pool)
                .await
                .unwrap();
        assert_eq!(count, 6);
        f.close().await;
    }
    #[tokio::test]
    async fn strict_direct_requires_encryption_before_grant_or_prepare_but_keeps_old_read_access() {
        let Some(mut f) = Fixture::new().await else {
            return;
        };
        let (cid, ai, h, g) = setup(&f).await;
        Arc::get_mut(&mut f.state).unwrap().require_encryption = true;
        let node: Uuid = sqlx::query_scalar("SELECT node_id FROM cloud_ai_bindings WHERE ai_id=$1")
            .bind(ai)
            .fetch_one(&f.state.pool)
            .await
            .unwrap();
        let request = DirectGrantRequest {
            node_id: node.to_string(),
            binding_version: "1".into(),
        };
        let error = grant(
            State(f.state.clone()),
            f.users[0].1.clone(),
            Path(cid),
            Json(request.clone()),
        )
        .await
        .err()
        .unwrap();
        assert_eq!(error.status.as_u16(), 409);
        assert_eq!(error.code, "encryption_required");
        let error = prepare(
            State(f.state.clone()),
            h.clone(),
            Json(DirectPrepare {
                grant: g.token.clone(),
            }),
        )
        .await
        .err()
        .unwrap();
        assert_eq!(error.status.as_u16(), 409);
        assert_eq!(error.code, "encryption_required");
        let grants: i64 =
            sqlx::query_scalar("SELECT count(*) FROM cloud_direct_grants WHERE conversation=$1")
                .bind(cid)
                .fetch_one(&f.state.pool)
                .await
                .unwrap();
        assert_eq!(grants, 1);
        let mid = Uuid::new_v4();
        let dir = f.state.attachments.join(mid.to_string());
        tokio::fs::create_dir_all(&dir).await.unwrap();
        tokio::fs::write(dir.join("0"), b"authorized old bytes")
            .await
            .unwrap();
        sqlx::query("INSERT INTO cloud_messages(id,conversation,sender,body,attachments) VALUES($1,$2,$3,'old plaintext',$4)").bind(mid).bind(cid).bind(f.users[0].0).bind(json!([{"index":0,"kind":"file","name":"old.txt","mime":"text/plain","byteLength":20}])).execute(&f.state.pool).await.unwrap();
        assert!(attachment(State(f.state.clone()),h.clone(),Path(ai),Json(json!({"grant":g.token,"conversationId":cid,"messageId":mid,"index":0,"revision":"1"}))).await.is_ok());
        sqlx::query("UPDATE cloud_conversations SET crypto_enabled=true WHERE id=$1")
            .bind(cid)
            .execute(&f.state.pool)
            .await
            .unwrap();
        assert!(
            grant(
                State(f.state.clone()),
                f.users[0].1.clone(),
                Path(cid),
                Json(request)
            )
            .await
            .is_ok()
        );
        assert!(
            prepare(
                State(f.state.clone()),
                h,
                Json(DirectPrepare { grant: g.token })
            )
            .await
            .is_ok()
        );
        f.close().await;
    }
}
