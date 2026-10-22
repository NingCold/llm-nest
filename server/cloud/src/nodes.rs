//! AI-scoped node authorization and durable remote execution.
use crate::{AppError, AppState, Result, auth, db as sqlx};
use axum::{
    Json, Router,
    extract::{
        Path, Query, State, WebSocketUpgrade,
        ws::{Message, WebSocket},
    },
    http::{HeaderMap, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::{Postgres, Row, Transaction};
use std::{sync::Arc, time::Duration};
use uuid::Uuid;
type Tx<'a> = Transaction<'a, Postgres>;

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/ai", get(managed).post(create))
        .route("/ai/{id}/node", post(bind))
        .route("/ai/{id}/node/revoke", post(revoke))
        .route("/node/pairings", post(pair))
        .route("/node/pairings/{id}", get(pair_status))
        .route("/node/tasks", get(tasks))
        .route("/node/tasks/{id}", get(task))
        .route("/node/tasks/{id}/materials", get(materials))
        .route(
            "/node/tasks/{id}/messages/{mid}/attachments/{index}",
            get(attachment),
        )
        .route("/node/tasks/{id}/start", post(start))
        .route("/node/tasks/{id}/result", post(result))
        .route("/node/tasks/{id}/reject", post(reject))
        .route("/chat/ai-tasks/{id}/cancel", post(cancel))
        .route("/node/ws", get(ws))
}
fn hash(s: &str) -> String {
    format!("{:x}", Sha256::digest(s.as_bytes()))
}
fn field<'a>(v: &'a Value, name: &str) -> Result<&'a str> {
    v.get(name)
        .and_then(Value::as_str)
        .ok_or_else(|| AppError::bad("请求字段缺失或类型错误"))
}
fn id(v: &Value, name: &str) -> Result<Uuid> {
    field(v, name)?
        .parse()
        .map_err(|_| AppError::bad("标识无效"))
}
fn revision(v: &Value, name: &str) -> Result<i64> {
    field(v, name)?
        .parse()
        .map_err(|_| AppError::bad("版本无效"))
}
fn bearer(h: &HeaderMap) -> Result<&str> {
    h.get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .filter(|v| {
            v.len() == 64
                && v.bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        })
        .ok_or_else(AppError::unauthorized)
}
pub(crate) async fn lock(tx: &mut Tx<'_>) -> Result<()> {
    sqlx::query("SELECT pg_advisory_xact_lock(731921)")
        .execute(&mut **tx)
        .await?;
    Ok(())
}
#[derive(Clone)]
struct Node {
    ai: Uuid,
    node: Uuid,
    version: i64,
}
async fn authenticate(tx: &mut Tx<'_>, h: &HeaderMap) -> Result<Node> {
    let row =
        sqlx::query("SELECT ai_id,node_id,version FROM cloud_ai_bindings WHERE token_hash=$1")
            .bind(hash(bearer(h)?))
            .fetch_optional(&mut **tx)
            .await?
            .ok_or_else(AppError::unauthorized)?;
    Ok(Node {
        ai: row.get(0),
        node: row.get(1),
        version: row.get(2),
    })
}
async fn admin(tx: &mut Tx<'_>, ai: Uuid, owner: Uuid) -> Result<()> {
    if !sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM cloud_ai_admins WHERE ai_id=$1 AND administrator=$2)",
    )
    .bind(ai)
    .bind(owner)
    .fetch_one(&mut **tx)
    .await?
    {
        return Err(AppError::forbidden());
    }
    Ok(())
}
async fn account(tx: &mut Tx<'_>, ai: Uuid) -> Result<Value> {
    let r = sqlx::query("SELECT id,name,bio,kind FROM accounts WHERE id=$1")
        .bind(ai)
        .fetch_optional(&mut **tx)
        .await?
        .ok_or_else(AppError::not_found)?;
    Ok(
        json!({"id":r.get::<Uuid,_>(0),"name":r.get::<String,_>(1),"bio":r.get::<String,_>(2),"kind":r.get::<String,_>(3)}),
    )
}
async fn view(tx: &mut Tx<'_>, ai: Uuid, owner: Uuid) -> Result<Value> {
    let r=sqlx::query("SELECT a.initial,b.version,b.node_id,b.node_name,b.last_seen_at,b.connection_id IS NOT NULL AND b.last_seen_at>now()-interval '45 seconds' AS online FROM cloud_ai_admins a JOIN cloud_ai_bindings b ON b.ai_id=a.ai_id WHERE a.ai_id=$1 AND a.administrator=$2").bind(ai).bind(owner).fetch_optional(&mut **tx).await?.ok_or_else(AppError::forbidden)?;
    let nid: Option<Uuid> = r.get("node_id");
    Ok(
        json!({"account":account(tx,ai).await?,"role":if r.get::<bool,_>("initial"){"initial"}else{"admin"},"bindingVersion":r.get::<i64,_>("version").to_string(),"node":nid.map(|nid|json!({"id":nid,"name":r.get::<Option<String>,_>("node_name"),"online":r.get::<bool,_>("online"),"lastSeenAt":r.get::<Option<chrono::DateTime<chrono::Utc>>,_>("last_seen_at")}))}),
    )
}
async fn admin_receipt(
    tx: &mut Tx<'_>,
    owner: Uuid,
    v: &Value,
    operation: &str,
    target: Uuid,
) -> Result<Option<Value>> {
    let rid = id(v, "requestId")?;
    let fp = hash(&json!([operation, target, v]).to_string());
    if let Some(r) = sqlx::query(
        "SELECT fingerprint,response FROM cloud_ai_receipts WHERE owner=$1 AND request=$2",
    )
    .bind(owner)
    .bind(rid)
    .fetch_optional(&mut **tx)
    .await?
    {
        if r.get::<String, _>(0) != fp {
            return Err(AppError::conflict("命令标识已用于其他内容"));
        }
        return Ok(Some(r.get(1)));
    }
    Ok(None)
}
async fn save_admin_receipt(
    tx: &mut Tx<'_>,
    owner: Uuid,
    v: &Value,
    operation: &str,
    target: Uuid,
    response: &Value,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO cloud_ai_receipts(owner,request,fingerprint,response) VALUES($1,$2,$3,$4)",
    )
    .bind(owner)
    .bind(id(v, "requestId")?)
    .bind(hash(&json!([operation, target, v]).to_string()))
    .bind(response)
    .execute(&mut **tx)
    .await?;
    Ok(())
}
async fn managed(State(s): State<Arc<AppState>>, h: HeaderMap) -> Result<Json<Value>> {
    let p = auth::authenticate(&s, &h).await?;
    let mut tx = s.pool.begin().await?;
    let ids: Vec<Uuid> = sqlx::query_scalar(
        "SELECT ai_id FROM cloud_ai_admins WHERE administrator=$1 ORDER BY ai_id",
    )
    .bind(p.id)
    .fetch_all(&mut *tx)
    .await?;
    let mut out = vec![];
    for ai in ids {
        out.push(view(&mut tx, ai, p.id).await?);
    }
    Ok(Json(json!(out)))
}
async fn create(
    State(s): State<Arc<AppState>>,
    h: HeaderMap,
    Json(v): Json<Value>,
) -> Result<Json<Value>> {
    let p = auth::authenticate(&s, &h).await?;
    let name = field(&v, "name")?.trim();
    let bio = field(&v, "bio")?;
    if name.is_empty() || name.chars().count() > 80 || bio.len() > 4096 {
        return Err(AppError::bad("账号资料超出限制"));
    }
    let mut tx = s.pool.begin().await?;
    lock(&mut tx).await?;
    if let Some(r) = admin_receipt(&mut tx, p.id, &v, "create", p.id).await? {
        return Ok(Json(r));
    }
    let ai = Uuid::new_v4();
    sqlx::query("INSERT INTO accounts(id,name,bio,kind) VALUES($1,$2,$3,'ai')")
        .bind(ai)
        .bind(name)
        .bind(bio)
        .execute(&mut *tx)
        .await?;
    sqlx::query("INSERT INTO cloud_ai_admins(ai_id,administrator,initial) VALUES($1,$2,true)")
        .bind(ai)
        .bind(p.id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("INSERT INTO cloud_ai_bindings(ai_id) VALUES($1)")
        .bind(ai)
        .execute(&mut *tx)
        .await?;
    let r = view(&mut tx, ai, p.id).await?;
    save_admin_receipt(&mut tx, p.id, &v, "create", p.id, &r).await?;
    tx.commit().await?;
    Ok(Json(r))
}
async fn pair(
    State(s): State<Arc<AppState>>,
    h: HeaderMap,
    Json(v): Json<connector::node::PairingRequest>,
) -> Result<Json<Value>> {
    let v = serde_json::to_value(v).unwrap();
    let token_hash = hash(bearer(&h)?);
    let nid = id(&v, "nodeId")?;
    let name = field(&v, "nodeName")?.trim();
    if name.is_empty() || name.chars().count() > 80 {
        return Err(AppError::bad("节点名称须为1–80字"));
    }
    let mut tx = s.pool.begin().await?;
    lock(&mut tx).await?;
    if sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM cloud_node_pairings WHERE expires_at>now()",
    )
    .fetch_one(&mut *tx)
    .await?
        > 1000
    {
        return Err(AppError::bad("配对请求过多，请稍后重试"));
    }
    if let Some(r)=sqlx::query("SELECT id,user_code,expires_at,node_id,node_name FROM cloud_node_pairings WHERE token_hash=$1 AND expires_at>now()").bind(&token_hash).fetch_optional(&mut *tx).await? {
        if r.get::<Uuid,_>("node_id")!=nid || r.get::<String,_>("node_name")!=name { return Err(AppError::conflict("凭据已用于其他配对")); }
        return Ok(Json(json!({"pairingId":r.get::<Uuid,_>("id"),"userCode":r.get::<String,_>("user_code"),"expiresAt":r.get::<chrono::DateTime<chrono::Utc>,_>("expires_at")})));
    }
    let pid = Uuid::new_v4();
    let code = Uuid::new_v4().simple().to_string()[..12].to_uppercase();
    let r=sqlx::query("INSERT INTO cloud_node_pairings(id,node_id,node_name,token_hash,user_code,expires_at) VALUES($1,$2,$3,$4,$5,now()+interval '10 minutes') RETURNING expires_at").bind(pid).bind(nid).bind(name).bind(token_hash).bind(&code).fetch_one(&mut *tx).await?;
    tx.commit().await?;
    Ok(Json(
        json!({"pairingId":pid,"userCode":code,"expiresAt":r.get::<chrono::DateTime<chrono::Utc>,_>(0)}),
    ))
}
async fn pair_status(
    State(s): State<Arc<AppState>>,
    h: HeaderMap,
    Path(pid): Path<Uuid>,
) -> Result<Json<Value>> {
    let r=sqlx::query("SELECT ai_id,binding_version FROM cloud_node_pairings WHERE id=$1 AND token_hash=$2 AND (expires_at>now() OR ai_id IS NOT NULL)").bind(pid).bind(hash(bearer(&h)?)).fetch_optional(&s.pool).await?.ok_or_else(AppError::not_found)?;
    let ai: Option<Uuid> = r.get(0);
    if let Some(ai) = ai {
        let mut tx = s.pool.begin().await?;
        let n = authenticate(&mut tx, &h).await?;
        if n.ai != ai || n.version != r.get::<i64, _>(1) {
            return Err(AppError::unauthorized());
        }
        Ok(Json(
            json!({"state":"authorized","aiId":ai,"bindingVersion":n.version.to_string()}),
        ))
    } else {
        Ok(Json(json!({"state":"pending"})))
    }
}
async fn bind(
    State(s): State<Arc<AppState>>,
    h: HeaderMap,
    Path(ai): Path<Uuid>,
    Json(v): Json<Value>,
) -> Result<Json<Value>> {
    change_binding(s, h, ai, v, false).await
}
async fn revoke(
    State(s): State<Arc<AppState>>,
    h: HeaderMap,
    Path(ai): Path<Uuid>,
    Json(v): Json<Value>,
) -> Result<Json<Value>> {
    change_binding(s, h, ai, v, true).await
}
async fn change_binding(
    s: Arc<AppState>,
    h: HeaderMap,
    ai: Uuid,
    v: Value,
    revoke: bool,
) -> Result<Json<Value>> {
    let p = auth::authenticate(&s, &h).await?;
    let mut tx = s.pool.begin().await?;
    lock(&mut tx).await?;
    admin(&mut tx, ai, p.id).await?;
    let op = if revoke { "revoke" } else { "bind" };
    if admin_receipt(&mut tx, p.id, &v, op, ai).await?.is_some() {
        return Ok(Json(view(&mut tx, ai, p.id).await?));
    }
    let old: i64 = sqlx::query_scalar("SELECT version FROM cloud_ai_bindings WHERE ai_id=$1")
        .bind(ai)
        .fetch_one(&mut *tx)
        .await?;
    if old != revision(&v, "expectedBindingVersion")? {
        return Err(AppError::conflict("节点绑定已变化"));
    }
    invalidate_ai(&mut tx, ai, "node_changed").await?;
    if revoke {
        sqlx::query("UPDATE cloud_ai_bindings SET version=version+1,node_id=NULL,node_name=NULL,token_hash=NULL,last_seen_at=NULL,connection_id=NULL WHERE ai_id=$1").bind(ai).execute(&mut *tx).await?;
    } else {
        let r=sqlx::query("SELECT * FROM cloud_node_pairings WHERE user_code=$1 AND expires_at>now() AND ai_id IS NULL").bind(field(&v,"pairingCode")?.trim().to_uppercase()).fetch_optional(&mut *tx).await?.ok_or_else(||AppError::bad("配对码无效、已使用或已过期"))?;
        sqlx::query("UPDATE cloud_ai_bindings SET version=version+1,node_id=$2,node_name=$3,token_hash=$4,last_seen_at=NULL,connection_id=NULL WHERE ai_id=$1").bind(ai).bind(r.get::<Uuid,_>("node_id")).bind(r.get::<String,_>("node_name")).bind(r.get::<String,_>("token_hash")).execute(&mut *tx).await?;
        sqlx::query("UPDATE cloud_node_pairings SET ai_id=$2,binding_version=$3 WHERE id=$1")
            .bind(r.get::<Uuid, _>("id"))
            .bind(ai)
            .bind(old + 1)
            .execute(&mut *tx)
            .await?;
    }
    let r = view(&mut tx, ai, p.id).await?;
    save_admin_receipt(&mut tx, p.id, &v, op, ai, &r).await?;
    tx.commit().await?;
    Ok(Json(r))
}
fn task_value(r: &sqlx::postgres::PgRow) -> Value {
    json!({"id":r.get::<Uuid,_>("id"),"aiId":r.get::<Uuid,_>("ai_id"),"conversationId":r.get::<Uuid,_>("conversation"),"bindingVersion":r.get::<i64,_>("binding_version").to_string(),"revision":r.get::<i64,_>("revision").to_string(),"triggerMessageId":r.get::<Uuid,_>("input_id"),"status":r.get::<String,_>("state"),"expiresAt":r.get::<chrono::DateTime<chrono::Utc>,_>("expires_at")})
}
async fn event(tx: &mut Tx<'_>, ai: Uuid, version: i64, task: Uuid, kind: &str) -> Result<()> {
    sqlx::query(
        "INSERT INTO cloud_node_events(ai_id,binding_version,task_id,kind) VALUES($1,$2,$3,$4)",
    )
    .bind(ai)
    .bind(version)
    .bind(task)
    .bind(kind)
    .execute(&mut **tx)
    .await?;
    Ok(())
}
async fn publish(tx: &mut Tx<'_>, tid: Uuid) -> Result<()> {
    let r = sqlx::query("SELECT * FROM cloud_node_tasks WHERE id=$1")
        .bind(tid)
        .fetch_one(&mut **tx)
        .await?;
    let response = json!({"taskId":tid,"status":r.get::<String,_>("state"),"error":r.get::<Option<String>,_>("error_code")});
    sqlx::query("UPDATE cloud_messages SET ai_response=$2 WHERE id=$1")
        .bind(r.get::<Uuid, _>("input_id"))
        .bind(response)
        .execute(&mut **tx)
        .await?;
    let cid: Uuid = r.get("conversation");
    sqlx::query("INSERT INTO cloud_events(owner,conversation) SELECT first_id,id FROM cloud_conversations WHERE id=$1 UNION ALL SELECT second_id,id FROM cloud_conversations WHERE id=$1").bind(cid).execute(&mut **tx).await?;
    event(
        tx,
        r.get("ai_id"),
        r.get("binding_version"),
        tid,
        if matches!(
            r.get::<String, _>("state").as_str(),
            "cancelled" | "expired"
        ) {
            "cancel"
        } else {
            "tasksChanged"
        },
    )
    .await
}
pub(crate) async fn enqueue(
    tx: &mut Tx<'_>,
    cid: Uuid,
    mid: Uuid,
    sender: Uuid,
    pair: (Uuid, Uuid),
) -> Result<()> {
    let ai = if pair.0 == sender { pair.1 } else { pair.0 };
    let binding:Option<i64>=sqlx::query_scalar("SELECT b.version FROM cloud_ai_bindings b JOIN accounts a ON a.id=b.ai_id WHERE a.id=$1 AND a.kind='ai'").bind(ai).fetch_optional(&mut **tx).await?;
    let Some(version) = binding else {
        return Ok(());
    };
    let tid = Uuid::new_v4();
    sqlx::query("INSERT INTO cloud_node_tasks(id,ai_id,conversation,input_id,input_revision,binding_version,expires_at) VALUES($1,$2,$3,$4,1,$5,now()+interval '5 minutes')").bind(tid).bind(ai).bind(cid).bind(mid).bind(version).execute(&mut **tx).await?;
    publish(tx, tid).await
}
async fn invalidate_rows(
    tx: &mut Tx<'_>,
    rows: Vec<sqlx::postgres::PgRow>,
    reason: &str,
) -> Result<()> {
    for r in rows {
        let tid: Uuid = r.get("id");
        sqlx::query("UPDATE cloud_node_tasks SET state='cancelled',revision=revision+1,error_code=$2,delta_text='',finished_at=now() WHERE id=$1").bind(tid).bind(reason).execute(&mut **tx).await?;
        if let Some(mid) = r.get::<Option<Uuid>, _>("message_id") {
            if reason == "cancelled" {
                finish_message(tx, &r, &r.get::<String, _>("delta_text"), "interrupted").await?;
            } else {
                sqlx::query("DELETE FROM cloud_messages WHERE id=$1 AND status<>'committed'")
                    .bind(mid)
                    .execute(&mut **tx)
                    .await?;
            }
        }
        publish(tx, tid).await?;
    }
    Ok(())
}
async fn invalidate_ai(tx: &mut Tx<'_>, ai: Uuid, reason: &str) -> Result<()> {
    let rows=sqlx::query("SELECT * FROM cloud_node_tasks WHERE ai_id=$1 AND state IN ('queued','running','uncertain')").bind(ai).fetch_all(&mut **tx).await?;
    invalidate_rows(tx, rows, reason).await
}
pub(crate) async fn source_changed(tx: &mut Tx<'_>, mid: Uuid) -> Result<()> {
    let rows=sqlx::query("SELECT * FROM cloud_node_tasks WHERE state IN ('queued','running','uncertain') AND (input_id=$1 OR EXISTS(SELECT 1 FROM jsonb_array_elements(sources) s WHERE s->>'messageId'=$1::text))").bind(mid).fetch_all(&mut **tx).await?;
    invalidate_rows(tx, rows, "source_changed").await
}
async fn expire(tx: &mut Tx<'_>, ai: Uuid) -> Result<()> {
    let rows=sqlx::query("UPDATE cloud_node_tasks SET state='expired',revision=revision+1,finished_at=now(),error_code='expired' WHERE ai_id=$1 AND state IN ('queued','running') AND expires_at<=now() RETURNING *").bind(ai).fetch_all(&mut **tx).await?;
    for r in rows {
        if r.get::<Option<Uuid>, _>("message_id").is_some() {
            finish_message(tx, &r, &r.get::<String, _>("delta_text"), "interrupted").await?;
        }
        publish(tx, r.get("id")).await?;
    }
    Ok(())
}
async fn authorized_task(tx: &mut Tx<'_>, n: &Node, tid: Uuid) -> Result<sqlx::postgres::PgRow> {
    sqlx::query("SELECT t.* FROM cloud_node_tasks t JOIN cloud_conversations c ON c.id=t.conversation WHERE t.id=$1 AND t.ai_id=$2 AND t.binding_version=$3 AND (c.first_id=$2 OR c.second_id=$2)").bind(tid).bind(n.ai).bind(n.version).fetch_optional(&mut **tx).await?.ok_or_else(AppError::not_found)
}
async fn tasks(State(s): State<Arc<AppState>>, h: HeaderMap) -> Result<Json<Value>> {
    let mut tx = s.pool.begin().await?;
    lock(&mut tx).await?;
    let n = authenticate(&mut tx, &h).await?;
    expire(&mut tx, n.ai).await?;
    let rows=sqlx::query("SELECT * FROM cloud_node_tasks WHERE ai_id=$1 AND binding_version=$2 AND state IN ('queued','running') ORDER BY created_at,id").bind(n.ai).bind(n.version).fetch_all(&mut *tx).await?;
    tx.commit().await?;
    Ok(Json(json!(rows.iter().map(task_value).collect::<Vec<_>>())))
}
async fn task(
    State(s): State<Arc<AppState>>,
    h: HeaderMap,
    Path(tid): Path<Uuid>,
) -> Result<Json<Value>> {
    let mut tx = s.pool.begin().await?;
    lock(&mut tx).await?;
    let n = authenticate(&mut tx, &h).await?;
    expire(&mut tx, n.ai).await?;
    let r = authorized_task(&mut tx, &n, tid).await?;
    tx.commit().await?;
    Ok(Json(task_value(&r)))
}
async fn history(tx: &mut Tx<'_>, r: &sqlx::postgres::PgRow) -> Result<Vec<sqlx::postgres::PgRow>> {
    let trigger = sqlx::query("SELECT position,revision,withdrawn FROM cloud_messages WHERE id=$1")
        .bind(r.get::<Uuid, _>("input_id"))
        .fetch_one(&mut **tx)
        .await?;
    if trigger.get::<bool, _>("withdrawn")
        || trigger.get::<i64, _>("revision") != r.get::<i64, _>("input_revision")
    {
        return Err(AppError::conflict("触发原文已变化"));
    }
    let mut rows=sqlx::query("SELECT * FROM cloud_messages WHERE conversation=$1 AND position<=$2 AND NOT withdrawn AND status='committed' ORDER BY position DESC LIMIT 100").bind(r.get::<Uuid,_>("conversation")).bind(trigger.get::<i64,_>("position")).fetch_all(&mut **tx).await?;
    rows.reverse();
    Ok(rows)
}
fn sources(rows: &[sqlx::postgres::PgRow]) -> Value {
    json!(rows.iter().map(|m|json!({"messageId":m.get::<Uuid,_>("id"),"revision":m.get::<i64,_>("revision").to_string()})).collect::<Vec<_>>())
}
async fn check_sources(tx: &mut Tx<'_>, r: &sqlx::postgres::PgRow) -> Result<()> {
    let saved: Value = r.get("sources");
    if sources(&history(tx, r).await?) != saved {
        return Err(AppError::conflict("上下文原文已变化"));
    }
    Ok(())
}
async fn materials(
    State(s): State<Arc<AppState>>,
    h: HeaderMap,
    Path(tid): Path<Uuid>,
) -> Result<Json<Value>> {
    let mut tx = s.pool.begin().await?;
    lock(&mut tx).await?;
    let n = authenticate(&mut tx, &h).await?;
    expire(&mut tx, n.ai).await?;
    let r = authorized_task(&mut tx, &n, tid).await?;
    if !matches!(r.get::<String, _>("state").as_str(), "queued" | "running") {
        return Err(AppError::conflict("任务已结束"));
    }
    let rows = history(&mut tx, &r).await?;
    let src = sources(&rows);
    let cid: Uuid = r.get("conversation");
    let peer:Uuid=sqlx::query_scalar("SELECT CASE WHEN first_id=$2 THEN second_id ELSE first_id END FROM cloud_conversations WHERE id=$1").bind(cid).bind(n.ai).fetch_one(&mut *tx).await?;
    let messages:Vec<Value>=rows.iter().map(|m|json!({"id":m.get::<Uuid,_>("id"),"senderId":m.get::<Uuid,_>("sender"),"text":m.get::<String,_>("body"),"revision":m.get::<i64,_>("revision").to_string(),"createdAt":m.get::<chrono::DateTime<chrono::Utc>,_>("created_at"),"attachments":m.get::<Value,_>("attachments")})).collect();
    let out = json!({"task":task_value(&r),"ai":account(&mut tx,n.ai).await?,"peer":account(&mut tx,peer).await?,"messages":messages,"sources":src});
    tx.commit().await?;
    Ok(Json(out))
}
#[derive(Deserialize)]
struct AttachmentQuery {
    revision: String,
}
async fn attachment(
    State(s): State<Arc<AppState>>,
    h: HeaderMap,
    Path((tid, mid, index)): Path<(Uuid, Uuid, usize)>,
    Query(q): Query<AttachmentQuery>,
) -> Result<Response> {
    let mut tx = s.pool.begin().await?;
    lock(&mut tx).await?;
    let n = authenticate(&mut tx, &h).await?;
    let r = authorized_task(&mut tx, &n, tid).await?;
    if !matches!(r.get::<String, _>("state").as_str(), "queued" | "running")
        || r.get::<chrono::DateTime<chrono::Utc>, _>("expires_at") <= chrono::Utc::now()
            && r.get::<String, _>("state") == "queued"
    {
        return Err(AppError::conflict("任务不再有效"));
    }
    let rows = history(&mut tx, &r).await?;
    let m = rows
        .iter()
        .find(|m| m.get::<Uuid, _>("id") == mid)
        .ok_or_else(AppError::not_found)?;
    if q.revision != m.get::<i64, _>("revision").to_string() {
        return Err(AppError::conflict("附件版本已变化"));
    }
    let attachments: Value = m.get("attachments");
    let part = attachments.get(index).ok_or_else(AppError::not_found)?;
    if part["kind"] != "image" {
        return Err(AppError::forbidden());
    }
    let bytes =
        tokio::fs::read(s.attachments.join(mid.to_string()).join(index.to_string())).await?;
    Ok((
        [
            (header::CONTENT_TYPE, part["mime"].as_str().unwrap()),
            (header::CACHE_CONTROL, "no-store"),
            (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
        ],
        bytes,
    )
        .into_response())
}
fn start_receipt(r: &sqlx::postgres::PgRow) -> Value {
    json!({"taskId":r.get::<Uuid,_>("id"),"runId":r.get::<Uuid,_>("run_id"),"messageId":r.get::<Uuid,_>("message_id"),"confirmationId":r.get::<Uuid,_>("confirmation_id"),"bindingVersion":r.get::<i64,_>("binding_version").to_string()})
}
async fn start(
    State(s): State<Arc<AppState>>,
    h: HeaderMap,
    Path(tid): Path<Uuid>,
    Json(v): Json<connector::node::StartRequest>,
) -> Result<Json<Value>> {
    let v = serde_json::to_value(v).unwrap();
    let rid = id(&v, "requestId")?;
    let fp = hash(&v.to_string());
    let config = field(&v, "configurationRevision")?;
    if config.is_empty() || config.len() > 256 {
        return Err(AppError::bad("配置快照标识无效"));
    }
    let mut tx = s.pool.begin().await?;
    lock(&mut tx).await?;
    let n = authenticate(&mut tx, &h).await?;
    expire(&mut tx, n.ai).await?;
    let r = authorized_task(&mut tx, &n, tid).await?;
    let state: String = r.get("state");
    if let Some(previous) = r.get::<Option<Uuid>, _>("start_request") {
        if previous != rid
            || r.get::<Option<String>, _>("start_fingerprint").as_deref() != Some(&fp)
        {
            return Err(AppError::conflict("任务已经使用其他开始请求"));
        }
        if state != "running"
            || r.get::<chrono::DateTime<chrono::Utc>, _>("expires_at") <= chrono::Utc::now()
        {
            return Err(AppError::conflict("任务已结束或已过期"));
        }
        check_sources(&mut tx, &r).await?;
        let response = start_receipt(&r);
        tx.commit().await?;
        return Ok(Json(response));
    }
    if state != "queued" || r.get::<i64, _>("revision") != revision(&v, "taskRevision")? {
        return Err(AppError::conflict("任务已开始、失效或版本已变化"));
    }
    let rows = history(&mut tx, &r).await?;
    if sources(&rows) != v["sources"] {
        return Err(AppError::conflict("上下文原文已变化"));
    }
    if sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM cloud_node_tasks WHERE conversation=$1 AND state='running')",
    )
    .bind(r.get::<Uuid, _>("conversation"))
    .fetch_one(&mut *tx)
    .await?
    {
        return Err(AppError::conflict("窗口已有运行任务"));
    }
    let r=sqlx::query("UPDATE cloud_node_tasks SET state='running',revision=revision+1,start_request=$2,start_fingerprint=$3,sources=$4,snapshot_id=$5,run_id=$6,message_id=$7,confirmation_id=$8,started_at=now() WHERE id=$1 RETURNING *").bind(tid).bind(rid).bind(fp).bind(&v["sources"]).bind(config).bind(Uuid::new_v4()).bind(Uuid::new_v4()).bind(Uuid::new_v4()).fetch_one(&mut *tx).await?;
    let response = start_receipt(&r);
    publish(&mut tx, tid).await?;
    tx.commit().await?;
    Ok(Json(response))
}
fn check_ids(r: &sqlx::postgres::PgRow, v: &Value) -> Result<()> {
    for (wire, column) in [
        ("runId", "run_id"),
        ("messageId", "message_id"),
        ("confirmationId", "confirmation_id"),
    ] {
        if Some(id(v, wire)?) != r.get::<Option<Uuid>, _>(column) {
            return Err(AppError::conflict("执行确认不匹配"));
        }
    }
    Ok(())
}
async fn finish_message(
    tx: &mut Tx<'_>,
    r: &sqlx::postgres::PgRow,
    text: &str,
    status: &str,
) -> Result<()> {
    let mid: Uuid = r.get("message_id");
    if text.is_empty() {
        sqlx::query("DELETE FROM cloud_messages WHERE id=$1 AND status='generating'")
            .bind(mid)
            .execute(&mut **tx)
            .await?;
    } else {
        sqlx::query("INSERT INTO cloud_messages(id,conversation,sender,body,status) VALUES($1,$2,$3,$4,$5) ON CONFLICT(id) DO UPDATE SET body=$4,status=$5,position=nextval(pg_get_serial_sequence('cloud_messages','position')),updated_at=now(),revision=cloud_messages.revision+1").bind(mid).bind(r.get::<Uuid,_>("conversation")).bind(r.get::<Uuid,_>("ai_id")).bind(text).bind(status).execute(&mut **tx).await?;
    }
    Ok(())
}
async fn result(
    State(s): State<Arc<AppState>>,
    h: HeaderMap,
    Path(tid): Path<Uuid>,
    Json(v): Json<connector::node::ResultRequest>,
) -> Result<Json<Value>> {
    let v = serde_json::to_value(v).unwrap();
    let rid = id(&v, "requestId")?;
    let fp = hash(&v.to_string());
    let status = field(&v, "status")?;
    let text = field(&v, "text")?;
    if !matches!(status, "completed" | "failed" | "interrupted" | "uncertain")
        || text.len() > 256 * 1024
        || status == "completed" && text.trim().is_empty()
    {
        return Err(AppError::bad("执行终态或正文无效"));
    }
    if v["errorCode"].as_str().is_some_and(|s| s.len() > 256) {
        return Err(AppError::bad("错误标识过长"));
    }
    let mut tx = s.pool.begin().await?;
    lock(&mut tx).await?;
    let n = authenticate(&mut tx, &h).await?;
    let r = authorized_task(&mut tx, &n, tid).await?;
    check_ids(&r, &v)?;
    if r.get::<String, _>("state") == "cancelled" {
        return Err(AppError::conflict("任务已取消"));
    }
    if let Some(previous) = r.get::<Option<Uuid>, _>("result_request") {
        if previous != rid
            || r.get::<Option<String>, _>("result_fingerprint").as_deref() != Some(&fp)
        {
            return Err(AppError::conflict("结果回执已用于其他内容"));
        }
        return Ok(Json(task_value(&r)));
    }
    check_sources(&mut tx, &r).await?;
    if !matches!(
        r.get::<String, _>("state").as_str(),
        "running" | "uncertain"
    ) || r.get::<chrono::DateTime<chrono::Utc>, _>("expires_at") <= chrono::Utc::now()
    {
        return Err(AppError::conflict("任务不接受结果或已过期"));
    }
    finish_message(
        &mut tx,
        &r,
        text,
        if status == "completed" {
            "committed"
        } else if status == "failed" {
            "failed"
        } else {
            "interrupted"
        },
    )
    .await?;
    let r=sqlx::query("UPDATE cloud_node_tasks SET state=$2,revision=revision+1,result_request=$3,result_fingerprint=$4,result=$5,error_code=$6,delta_text='',finished_at=now() WHERE id=$1 RETURNING *").bind(tid).bind(status).bind(rid).bind(fp).bind(json!({"status":status,"usage":v["usage"],"errorCode":v["errorCode"]})).bind(v["errorCode"].as_str()).fetch_one(&mut *tx).await?;
    publish(&mut tx, tid).await?;
    tx.commit().await?;
    Ok(Json(task_value(&r)))
}
async fn reject(
    State(s): State<Arc<AppState>>,
    h: HeaderMap,
    Path(tid): Path<Uuid>,
    Json(v): Json<connector::node::RejectRequest>,
) -> Result<Json<Value>> {
    let v = serde_json::to_value(v).unwrap();
    let rid = id(&v, "requestId")?;
    let fp = hash(&v.to_string());
    let code = field(&v, "code")?;
    let message = field(&v, "message")?;
    if code.is_empty() || code.len() > 256 || message.len() > 4096 {
        return Err(AppError::bad("拒绝原因无效"));
    }
    let mut tx = s.pool.begin().await?;
    lock(&mut tx).await?;
    let n = authenticate(&mut tx, &h).await?;
    expire(&mut tx, n.ai).await?;
    let r = authorized_task(&mut tx, &n, tid).await?;
    if r.get::<Option<Uuid>, _>("result_request") == Some(rid)
        && r.get::<Option<String>, _>("result_fingerprint").as_deref() == Some(&fp)
    {
        return Ok(Json(task_value(&r)));
    }
    if r.get::<String, _>("state") != "queued"
        || r.get::<i64, _>("revision") != revision(&v, "taskRevision")?
    {
        return Err(AppError::conflict("任务已开始或失效"));
    }
    let r=sqlx::query("UPDATE cloud_node_tasks SET state='failed',revision=revision+1,result_request=$2,result_fingerprint=$3,error_code=$4,finished_at=now() WHERE id=$1 RETURNING *").bind(tid).bind(rid).bind(fp).bind(code).fetch_one(&mut *tx).await?;
    publish(&mut tx, tid).await?;
    tx.commit().await?;
    Ok(Json(task_value(&r)))
}
async fn cancel(
    State(s): State<Arc<AppState>>,
    h: HeaderMap,
    Path(tid): Path<Uuid>,
    Json(v): Json<Value>,
) -> Result<Json<Value>> {
    let p = auth::authenticate(&s, &h).await?;
    let mut tx = s.pool.begin().await?;
    lock(&mut tx).await?;
    let r=sqlx::query("SELECT t.* FROM cloud_node_tasks t JOIN cloud_conversations c ON c.id=t.conversation WHERE t.id=$1 AND (c.first_id=$2 OR c.second_id=$2)").bind(tid).bind(p.id).fetch_optional(&mut *tx).await?.ok_or_else(AppError::not_found)?;
    if admin_receipt(&mut tx, p.id, &v, "cancel", tid)
        .await?
        .is_some()
    {
        return Ok(Json(task_value(&r)));
    }
    if matches!(
        r.get::<String, _>("state").as_str(),
        "queued" | "running" | "uncertain"
    ) {
        invalidate_rows(&mut tx, vec![r], "cancelled").await?;
    }
    let r = sqlx::query("SELECT * FROM cloud_node_tasks WHERE id=$1")
        .bind(tid)
        .fetch_one(&mut *tx)
        .await?;
    let response = task_value(&r);
    save_admin_receipt(&mut tx, p.id, &v, "cancel", tid, &response).await?;
    tx.commit().await?;
    Ok(Json(response))
}
pub async fn recover(pool: &sqlx::PgPool) -> Result<()> {
    let mut tx = pool.begin().await?;
    lock(&mut tx).await?;
    sqlx::query("UPDATE cloud_ai_bindings SET connection_id=NULL")
        .execute(&mut *tx)
        .await?;
    let rows=sqlx::query("UPDATE cloud_node_tasks SET state='uncertain',revision=revision+1,error_code='server_restarted',finished_at=now() WHERE state='running' RETURNING *").fetch_all(&mut *tx).await?;
    for r in rows {
        finish_message(
            &mut tx,
            &r,
            &r.get::<String, _>("delta_text"),
            "interrupted",
        )
        .await?;
        publish(&mut tx, r.get("id")).await?;
    }
    tx.commit().await?;
    Ok(())
}
async fn expire_pending(pool: &sqlx::PgPool) -> Result<()> {
    let mut tx = pool.begin().await?;
    lock(&mut tx).await?;
    let ais: Vec<Uuid> = sqlx::query_scalar("SELECT DISTINCT ai_id FROM cloud_node_tasks WHERE state IN ('queued','running') AND expires_at<=now()")
        .fetch_all(&mut *tx).await?;
    for ai in ais {
        expire(&mut tx, ai).await?;
    }
    tx.commit().await?;
    Ok(())
}
/// Expiration is a cloud responsibility even while every node is offline.
pub async fn maintain(pool: sqlx::PgPool) {
    let mut tick = tokio::time::interval(Duration::from_secs(30));
    loop {
        tick.tick().await;
        if let Err(error) = expire_pending(&pool).await {
            eprintln!("节点任务过期维护失败：{}", error.code);
        }
    }
}
async fn ws(
    State(s): State<Arc<AppState>>,
    h: HeaderMap,
    upgrade: WebSocketUpgrade,
) -> Result<Response> {
    let mut tx = s.pool.begin().await?;
    let n = authenticate(&mut tx, &h).await?;
    Ok(upgrade
        .max_message_size(1024 * 1024)
        .max_frame_size(1024 * 1024)
        .on_upgrade(move |socket| socket_loop(s, h, n, socket)))
}
async fn send_frame(
    socket: &mut WebSocket,
    frame: connector::node::ServerFrame,
) -> std::result::Result<(), axum::Error> {
    socket
        .send(Message::Text(serde_json::to_string(&frame).unwrap().into()))
        .await
}
async fn delta(
    s: &AppState,
    h: &HeaderMap,
    frame: connector::node::ClientFrame,
) -> Result<(Uuid, u64)> {
    let connector::node::ClientFrame::Delta {
        task_id,
        run_id,
        message_id,
        confirmation_id,
        sequence,
        text,
    } = frame
    else {
        return Err(AppError::bad("预期草稿帧"));
    };
    if text.len() > 256 * 1024 || sequence == 0 || sequence > i64::MAX as u64 {
        return Err(AppError::bad("草稿过长或序号无效"));
    }
    let v = json!({"runId":run_id,"messageId":message_id,"confirmationId":confirmation_id});
    let tid = task_id.parse().map_err(|_| AppError::bad("任务标识无效"))?;
    let mut tx = s.pool.begin().await?;
    lock(&mut tx).await?;
    let n = authenticate(&mut tx, h).await?;
    let r = authorized_task(&mut tx, &n, tid).await?;
    check_ids(&r, &v)?;
    if r.get::<String, _>("state") != "running"
        || r.get::<chrono::DateTime<chrono::Utc>, _>("expires_at") <= chrono::Utc::now()
    {
        return Err(AppError::conflict("任务不接受草稿"));
    }
    check_sources(&mut tx, &r).await?;
    let old = r.get::<i64, _>("delta_sequence") as u64;
    if sequence < old || sequence == old && text != r.get::<String, _>("delta_text") {
        return Err(AppError::conflict("草稿序号已过期"));
    }
    if sequence == old {
        return Ok((tid, sequence));
    }
    sqlx::query("UPDATE cloud_node_tasks SET delta_sequence=$2,delta_text=$3 WHERE id=$1")
        .bind(tid)
        .bind(sequence as i64)
        .bind(&text)
        .execute(&mut *tx)
        .await?;
    if !text.is_empty() {
        sqlx::query("INSERT INTO cloud_messages(id,conversation,sender,body,status) VALUES($1,$2,$3,$4,'generating') ON CONFLICT(id) DO UPDATE SET body=$4,updated_at=now(),revision=cloud_messages.revision+1").bind(r.get::<Uuid,_>("message_id")).bind(r.get::<Uuid,_>("conversation")).bind(n.ai).bind(text).execute(&mut *tx).await?;
    } else {
        sqlx::query("DELETE FROM cloud_messages WHERE id=$1 AND status='generating'")
            .bind(r.get::<Uuid, _>("message_id"))
            .execute(&mut *tx)
            .await?;
    }
    publish(&mut tx, tid).await?;
    tx.commit().await?;
    Ok((tid, sequence))
}
async fn socket_loop(s: Arc<AppState>, h: HeaderMap, n: Node, mut socket: WebSocket) {
    use connector::node::{ClientFrame, ServerFrame};
    let hello = tokio::time::timeout(Duration::from_secs(10), socket.recv()).await;
    let valid = match hello {
        Ok(Some(Ok(Message::Text(text)))) => {
            matches!(serde_json::from_str::<ClientFrame>(&text),Ok(ClientFrame::Hello{protocol_version:1,node_id,ai_id,binding_version}) if node_id==n.node.to_string()&&ai_id==n.ai.to_string()&&binding_version==n.version.to_string())
        }
        _ => false,
    };
    if !valid {
        let _ = send_frame(
            &mut socket,
            ServerFrame::Error {
                code: "invalid_hello".into(),
                message: "节点身份或协议版本不匹配".into(),
            },
        )
        .await;
        return;
    }
    let connection = Uuid::new_v4();
    let initialized:Result<i64>=async {
        let mut tx=s.pool.begin().await?; lock(&mut tx).await?; authenticate(&mut tx,&h).await?;
        sqlx::query("UPDATE cloud_ai_bindings SET connection_id=$2,last_seen_at=now() WHERE ai_id=$1").bind(n.ai).bind(connection).execute(&mut *tx).await?;
        let cursor=sqlx::query_scalar("SELECT COALESCE(max(id),0) FROM cloud_node_events WHERE ai_id=$1").bind(n.ai).fetch_one(&mut *tx).await?;
        sqlx::query("INSERT INTO cloud_events(owner) SELECT administrator FROM cloud_ai_admins WHERE ai_id=$1").bind(n.ai).execute(&mut *tx).await?;
        tx.commit().await?; Ok(cursor)
    }.await;
    let Ok(mut cursor) = initialized else {
        return;
    };
    if send_frame(
        &mut socket,
        ServerFrame::Ready {
            protocol_version: 1,
        },
    )
    .await
    .is_err()
    {
        return;
    }
    let mut tick = tokio::time::interval(Duration::from_secs(1));
    let mut ticks = 0;
    let mut last_received = tokio::time::Instant::now();
    let mut last_received_at = chrono::Utc::now();
    loop {
        tokio::select! {
            incoming=socket.recv()=>{
                match incoming {
                    Some(Ok(Message::Text(text)))=>{
                        let result=match serde_json::from_str::<ClientFrame>(&text){Ok(frame)=>delta(&s,&h,frame).await,Err(_)=>Err(AppError::bad("节点帧格式无效"))};
                        if result.is_ok() { last_received = tokio::time::Instant::now(); last_received_at = chrono::Utc::now(); }
                        let frame=match result {Ok((tid,sequence))=>ServerFrame::DeltaAccepted{task_id:tid.to_string(),sequence},Err(e)=>ServerFrame::Error{code:e.code.into(),message:e.message}};
                        if send_frame(&mut socket,frame).await.is_err(){break;}
                    }
                    Some(Ok(Message::Ping(bytes)))=>{last_received = tokio::time::Instant::now(); last_received_at = chrono::Utc::now(); if socket.send(Message::Pong(bytes)).await.is_err(){break;}}
                    Some(Ok(Message::Pong(_)))=>{last_received = tokio::time::Instant::now(); last_received_at = chrono::Utc::now();},
                    _=>break,
                }
            }
            _=tick.tick()=>{
                if last_received.elapsed() > Duration::from_secs(45) { break; }
                let events:Result<(Vec<sqlx::postgres::PgRow>,bool)>=async {
                    let mut tx=s.pool.begin().await?; lock(&mut tx).await?;
                    if authenticate(&mut tx,&h).await.is_err() {
                        // The previously authenticated socket may receive only its cancellation notices after revocation.
                        let rows=sqlx::query("SELECT id,task_id,kind FROM cloud_node_events WHERE ai_id=$1 AND binding_version=$2 AND id>$3 AND kind='cancel' ORDER BY id").bind(n.ai).bind(n.version).bind(cursor).fetch_all(&mut *tx).await?;
                        tx.commit().await?; return Ok((rows,false));
                    }
                    let updated=sqlx::query("UPDATE cloud_ai_bindings SET last_seen_at=$3 WHERE ai_id=$1 AND connection_id=$2").bind(n.ai).bind(connection).bind(last_received_at).execute(&mut *tx).await?;
                    if updated.rows_affected()!=1{return Err(AppError::unauthorized());}
                    expire(&mut tx,n.ai).await?;
                    let rows=sqlx::query("SELECT id,task_id,kind FROM cloud_node_events WHERE ai_id=$1 AND binding_version=$2 AND id>$3 ORDER BY id LIMIT 100").bind(n.ai).bind(n.version).bind(cursor).fetch_all(&mut *tx).await?;
                    tx.commit().await?;Ok((rows,true))
                }.await;
                let Ok((events,active))=events else{break;};
                let mut failed=false;
                for r in events {
                    cursor=r.get("id");
                    let frame=if r.get::<String,_>("kind")=="cancel" {ServerFrame::Cancel{task_id:r.get::<Uuid,_>("task_id").to_string()}}else{ServerFrame::TasksChanged};
                    if send_frame(&mut socket,frame).await.is_err(){failed=true;break;}
                }
                if failed || !active {break;}
                ticks+=1; if ticks%15==0 && socket.send(Message::Ping(vec![].into())).await.is_err(){break;}
            }
        }
    }
    let _:Result<()>=async {
        let mut tx=s.pool.begin().await?;lock(&mut tx).await?;
        let updated=sqlx::query("UPDATE cloud_ai_bindings SET connection_id=NULL WHERE ai_id=$1 AND connection_id=$2").bind(n.ai).bind(connection).execute(&mut *tx).await?;
        if updated.rows_affected()==1{sqlx::query("INSERT INTO cloud_events(owner) SELECT administrator FROM cloud_ai_admins WHERE ai_id=$1").bind(n.ai).execute(&mut *tx).await?;}
        tx.commit().await?; Ok(())
    }.await;
}
#[cfg(test)]
#[path = "nodes_tests.rs"]
mod tests;
