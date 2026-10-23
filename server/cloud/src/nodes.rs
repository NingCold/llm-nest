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
    routing::{get, post, put},
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
        .merge(crate::consciousness::routes())
        .merge(crate::management::routes())
        .route("/ai", get(managed).post(create))
        .route("/ai/{id}/status", put(status))
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
pub(crate) struct Node {
    pub(crate) ai: Uuid,
    pub(crate) node: Uuid,
    pub(crate) version: i64,
}
pub(crate) async fn authenticate(tx: &mut Tx<'_>, h: &HeaderMap) -> Result<Node> {
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
pub(crate) async fn admin(tx: &mut Tx<'_>, ai: Uuid, owner: Uuid) -> Result<()> {
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
    crate::chat::account(tx, ai).await
}
pub(crate) async fn view(tx: &mut Tx<'_>, ai: Uuid, owner: Uuid) -> Result<Value> {
    let r=sqlx::query("SELECT a.initial,ac.status_revision,ac.profile_revision,ac.administrators_revision,jsonb_build_object('id',ac.id,'name',ac.name,'bio',ac.bio,'kind',ac.kind,'status',ac.status) AS account,b.version,b.capabilities,b.handover_state,b.node_id,b.node_name,b.last_seen_at,b.connection_id IS NOT NULL AND b.last_seen_at>now()-interval '45 seconds' AS online FROM cloud_ai_admins a JOIN cloud_ai_bindings b ON b.ai_id=a.ai_id JOIN accounts ac ON ac.id=a.ai_id WHERE a.ai_id=$1 AND a.administrator=$2").bind(ai).bind(owner).fetch_optional(&mut **tx).await?.ok_or_else(AppError::forbidden)?;
    let nid: Option<Uuid> = r.get("node_id");
    Ok(
        json!({"account":r.get::<Value,_>("account"),"role":if r.get::<bool,_>("initial"){"initial"}else{"admin"},"statusRevision":r.get::<i64,_>("status_revision").to_string(),"profileRevision":r.get::<i64,_>("profile_revision").to_string(),"administratorsRevision":r.get::<i64,_>("administrators_revision").to_string(),"bindingVersion":r.get::<i64,_>("version").to_string(),"node":nid.map(|nid|json!({"id":nid,"name":r.get::<Option<String>,_>("node_name"),"online":r.get::<bool,_>("online"),"capabilities":r.get::<Value,_>("capabilities"),"handoverState":r.get::<String,_>("handover_state"),"lastSeenAt":r.get::<Option<chrono::DateTime<chrono::Utc>>,_>("last_seen_at")}))}),
    )
}
pub(crate) async fn admin_receipt(
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
pub(crate) async fn save_admin_receipt(
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
        let ai = id(&r["account"], "id")?;
        admin(&mut tx, ai, p.id).await?;
        return Ok(Json(view(&mut tx, ai, p.id).await?));
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
    if !revoke && crate::handover::encrypted(&mut tx, ai).await? {
        let b = sqlx::query("SELECT node_id,handover_state FROM cloud_ai_bindings WHERE ai_id=$1")
            .bind(ai)
            .fetch_one(&mut *tx)
            .await?;
        if b.get::<Option<Uuid>, _>("node_id").is_some()
            && b.get::<String, _>("handover_state") != "ready"
            && v.get("force").and_then(Value::as_bool) != Some(true)
        {
            return Err(AppError::conflict(
                "加密节点尚未完成移交；旧节点失联时需明确强制更换",
            ));
        }
    }
    invalidate_ai(&mut tx, ai, "node_changed").await?;
    if revoke {
        sqlx::query("UPDATE cloud_ai_bindings SET version=version+1,node_id=NULL,node_name=NULL,token_hash=NULL,last_seen_at=NULL,connection_id=NULL,capabilities='[]',handover_state='active' WHERE ai_id=$1").bind(ai).execute(&mut *tx).await?;
    } else {
        let r=sqlx::query("SELECT * FROM cloud_node_pairings WHERE user_code=$1 AND expires_at>now() AND ai_id IS NULL").bind(field(&v,"pairingCode")?.trim().to_uppercase()).fetch_optional(&mut *tx).await?.ok_or_else(||AppError::bad("配对码无效、已使用或已过期"))?;
        sqlx::query("UPDATE cloud_ai_bindings SET version=version+1,node_id=$2,node_name=$3,token_hash=$4,last_seen_at=NULL,connection_id=NULL,capabilities='[]',handover_state='active' WHERE ai_id=$1").bind(ai).bind(r.get::<Uuid,_>("node_id")).bind(r.get::<String,_>("node_name")).bind(r.get::<String,_>("token_hash")).execute(&mut *tx).await?;
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
    json!({"id":r.get::<Uuid,_>("id"),"aiId":r.get::<Uuid,_>("ai_id"),"conversationId":r.get::<Uuid,_>("conversation"),"bindingVersion":r.get::<i64,_>("binding_version").to_string(),"revision":r.get::<i64,_>("revision").to_string(),"trigger":r.get::<Value,_>("trigger"),"automatic":r.get::<bool,_>("automatic"),"status":r.get::<String,_>("state"),"expiresAt":r.get::<chrono::DateTime<chrono::Utc>,_>("expires_at")})
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
pub(crate) async fn publish(tx: &mut Tx<'_>, tid: Uuid) -> Result<()> {
    let r = sqlx::query("SELECT * FROM cloud_node_tasks WHERE id=$1")
        .bind(tid)
        .fetch_one(&mut **tx)
        .await?;
    let cid: Uuid = r.get("conversation");
    crate::participation::task_changed(tx, &r).await?;
    sqlx::query("UPDATE cloud_messages m SET ai_responses=(SELECT coalesce(jsonb_agg(jsonb_build_object('aiId',ai_id,'taskId',id,'status',state,'error',error_code) ORDER BY queue_position),'[]'::jsonb) FROM cloud_node_tasks WHERE m.id=ANY(input_ids)) WHERE m.id=ANY($1)").bind(r.get::<Vec<Uuid>,_>("input_ids")).execute(&mut **tx).await?;
    sqlx::query("INSERT INTO cloud_events(owner,conversation) SELECT account_id,conversation FROM cloud_members WHERE conversation=$1").bind(cid).execute(&mut **tx).await?;
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
    .await?;
    if !matches!(r.get::<String, _>("state").as_str(), "queued" | "running")
        && let Some(next)=sqlx::query("SELECT ai_id,binding_version,id FROM cloud_node_tasks WHERE conversation=$1 AND state='queued' ORDER BY queue_position LIMIT 1").bind(cid).fetch_optional(&mut **tx).await? {
        event(tx,next.get("ai_id"),next.get("binding_version"),next.get("id"),"tasksChanged").await?;
    }
    Ok(())
}
pub(crate) async fn enqueue(tx: &mut Tx<'_>, cid: Uuid, mid: Uuid, sender: Uuid) -> Result<()> {
    let c = sqlx::query("SELECT kind,revision FROM cloud_conversations WHERE id=$1")
        .bind(cid)
        .fetch_one(&mut **tx)
        .await?;
    let m =
        sqlx::query("SELECT mentions,reply_to,context_reference FROM cloud_messages WHERE id=$1")
            .bind(mid)
            .fetch_one(&mut **tx)
            .await?;
    crate::participation::ensure(tx, cid).await?;
    let p = sqlx::query("SELECT * FROM cloud_participation WHERE conversation=$1")
        .bind(cid)
        .fetch_one(&mut **tx)
        .await?;
    let old = sqlx::query(
        "SELECT * FROM cloud_node_tasks WHERE conversation=$1 AND automatic AND contact_intent_id IS NULL AND state='queued'",
    )
    .bind(cid)
    .fetch_all(&mut **tx)
    .await?;
    invalidate_rows(tx, old, "input_pending").await?;
    sqlx::query("UPDATE cloud_participation SET consecutive_ai=0,bout_id=coalesce(bout_id,$2),bout_calls=CASE WHEN bout_id IS NULL THEN 0 ELSE bout_calls END,last_activity=greatest(last_activity,now()),reason='input_pending' WHERE conversation=$1").bind(cid).bind(Uuid::new_v4()).execute(&mut **tx).await?;
    if p.get::<bool, _>("paused") {
        return Ok(());
    }
    let ordinary = m.get::<Vec<Uuid>, _>("mentions").is_empty()
        && m.get::<Option<Uuid>, _>("reply_to").is_none()
        && m.get::<Option<Value>, _>("context_reference").is_none();
    if c.get::<String, _>("kind") == "direct" && ordinary {
        let prior=sqlx::query("SELECT t.* FROM cloud_node_tasks t JOIN cloud_messages m ON m.id=t.input_id WHERE t.conversation=$1 AND t.state='queued' AND NOT t.automatic AND cardinality(t.input_ids)<20 AND t.not_before>now() AND m.reply_to IS NULL AND m.mentions='{}' AND m.context_reference IS NULL ORDER BY t.queue_position DESC LIMIT 1").bind(cid).fetch_optional(&mut **tx).await?;
        if let Some(prior) = prior {
            let tid: Uuid = prior.get("id");
            let ms = p.get::<Value, _>("policy")["directMergeMs"]
                .as_i64()
                .unwrap();
            sqlx::query("UPDATE cloud_node_tasks SET ready_notified=false,input_id=$2,input_revision=1,input_ids=array_append(input_ids,$2),trigger=jsonb_build_object('type','message','messageId',$2::text,'inputIds',array_append(input_ids,$2)),not_before=now()+($3*interval '1 millisecond'),expires_at=now()+interval '5 minutes',revision=revision+1 WHERE id=$1").bind(tid).bind(mid).bind(ms as f64).execute(&mut **tx).await?;
            publish(tx, tid).await?;
            return Ok(());
        }
    }
    let mut targets: Vec<Uuid> = if c.get::<String, _>("kind") == "direct" {
        sqlx::query_scalar(
            "SELECT account_id FROM cloud_members WHERE conversation=$1 AND account_id<>$2",
        )
        .bind(cid)
        .bind(sender)
        .fetch_all(&mut **tx)
        .await?
    } else {
        m.get("mentions")
    };
    if let Some(reply) = m.get::<Option<Uuid>, _>("reply_to") {
        let a: Uuid = sqlx::query_scalar("SELECT sender FROM cloud_messages WHERE id=$1")
            .bind(reply)
            .fetch_one(&mut **tx)
            .await?;
        targets.push(a);
    }
    let mut seen = std::collections::HashSet::new();
    for ai in targets {
        if !seen.insert(ai) {
            continue;
        }
        if p.get::<Vec<Uuid>, _>("paused_ai").contains(&ai) {
            continue;
        }
        let binding:Option<i64>=sqlx::query_scalar("SELECT b.version FROM cloud_ai_bindings b JOIN accounts a ON a.id=b.ai_id JOIN cloud_members m ON m.account_id=a.id AND m.conversation=$2 WHERE a.id=$1 AND a.kind='ai' AND a.status='active' AND b.handover_state='active'").bind(ai).bind(cid).fetch_optional(&mut **tx).await?;
        if let Some(version) = binding {
            let tid = Uuid::new_v4();
            sqlx::query("INSERT INTO cloud_node_tasks(id,ai_id,conversation,input_id,input_revision,binding_version,expires_at,conversation_revision) VALUES($1,$2,$3,$4,1,$5,now()+interval '5 minutes',$6)").bind(tid).bind(ai).bind(cid).bind(mid).bind(version).bind(c.get::<i64,_>("revision")).execute(&mut **tx).await?;
            let ms = if ordinary && c.get::<String, _>("kind") == "direct" {
                p.get::<Value, _>("policy")["directMergeMs"]
                    .as_i64()
                    .unwrap()
            } else {
                0
            };
            sqlx::query("UPDATE cloud_node_tasks SET input_ids=ARRAY[$2],trigger=jsonb_build_object('type','message','messageId',$2::text,'inputIds',ARRAY[$2]),not_before=now()+($3*interval '1 millisecond') WHERE id=$1").bind(tid).bind(mid).bind(ms as f64).execute(&mut **tx).await?;
            publish(tx, tid).await?;
        }
    }
    if ordinary {
        crate::participation::automatic(tx, cid, mid, chrono::Utc::now()).await?;
    }
    Ok(())
}
pub(crate) async fn conversation_changed(tx: &mut Tx<'_>, cid: Uuid, reason: &str) -> Result<()> {
    let rows=sqlx::query("SELECT * FROM cloud_node_tasks WHERE conversation=$1 AND state IN ('queued','running','uncertain')").bind(cid).fetch_all(&mut **tx).await?;
    invalidate_rows(tx, rows, reason).await?;
    crate::participation::reconcile(tx).await
}
pub(crate) async fn invalidate_rows(
    tx: &mut Tx<'_>,
    rows: Vec<sqlx::postgres::PgRow>,
    reason: &str,
) -> Result<()> {
    for r in rows {
        let tid: Uuid = r.get("id");
        sqlx::query("UPDATE cloud_node_tasks SET state='cancelled',revision=revision+1,error_code=$2,delta_text='',finished_at=now() WHERE id=$1").bind(tid).bind(reason).execute(&mut **tx).await?;
        if let Some(mid) = r.get::<Option<Uuid>, _>("message_id") {
            if matches!(
                reason,
                "cancelled"
                    | "blocked"
                    | "account_disabled"
                    | "expired"
                    | "quiet"
                    | "authorization_closed"
                    | "authorization_changed"
            ) {
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
    sqlx::query("UPDATE cloud_summary_calls SET state='stale',error_code=$2,finished_at=now() WHERE ai_id=$1 AND state='running'").bind(ai).bind(reason).execute(&mut **tx).await?;
    let rows=sqlx::query("SELECT * FROM cloud_node_tasks WHERE ai_id=$1 AND state IN ('queued','running','uncertain')").bind(ai).fetch_all(&mut **tx).await?;
    invalidate_rows(tx, rows, reason).await
}
pub(crate) async fn source_changed(tx: &mut Tx<'_>, mid: Uuid) -> Result<()> {
    let rows=sqlx::query("SELECT * FROM cloud_node_tasks WHERE state IN ('queued','running','uncertain') AND ($1=ANY(input_ids) OR input_id=$1 OR reference->>'messageId'=$1::text OR EXISTS(SELECT 1 FROM cloud_messages m WHERE m.id=cloud_node_tasks.input_id AND m.reply_to=$1) OR EXISTS(SELECT 1 FROM jsonb_array_elements(sources || derived_sources) s WHERE s->>'messageId'=$1::text))").bind(mid).fetch_all(&mut **tx).await?;
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
    sqlx::query("SELECT t.* FROM cloud_node_tasks t JOIN cloud_members c ON c.conversation=t.conversation JOIN accounts a ON a.id=t.ai_id WHERE t.id=$1 AND t.ai_id=$2 AND t.binding_version=$3 AND c.account_id=$2 AND a.status='active'").bind(tid).bind(n.ai).bind(n.version).fetch_optional(&mut **tx).await?.ok_or_else(AppError::not_found)
}
async fn tasks(State(s): State<Arc<AppState>>, h: HeaderMap) -> Result<Json<Value>> {
    let mut tx = s.pool.begin().await?;
    lock(&mut tx).await?;
    let n = authenticate(&mut tx, &h).await?;
    expire(&mut tx, n.ai).await?;
    let rows=sqlx::query("SELECT t.* FROM cloud_node_tasks t JOIN accounts a ON a.id=t.ai_id JOIN cloud_members m ON m.account_id=t.ai_id AND m.conversation=t.conversation WHERE t.ai_id=$1 AND t.binding_version=$2 AND a.status='active' AND (t.state='running' OR t.state='queued' AND EXISTS(SELECT 1 FROM cloud_ai_bindings b WHERE b.ai_id=t.ai_id AND b.handover_state='active') AND t.not_before<=now() AND NOT EXISTS(SELECT 1 FROM cloud_node_tasks q WHERE q.conversation=t.conversation AND (q.state='running' OR q.state='queued' AND (q.contact_intent_id IS NULL OR q.not_before<=now()) AND q.queue_position<t.queue_position))) ORDER BY t.queue_position").bind(n.ai).bind(n.version).fetch_all(&mut *tx).await?;
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
    check_conversation(tx, r).await?;
    if r.get::<Option<Uuid>, _>("contact_intent_id").is_some() {
        return Ok(sqlx::query("SELECT * FROM cloud_messages WHERE conversation=$1 AND NOT withdrawn AND status='committed' ORDER BY position DESC LIMIT 100").bind(r.get::<Uuid,_>("conversation")).fetch_all(&mut **tx).await?.into_iter().rev().collect());
    }
    let trigger =
        sqlx::query("SELECT position,revision,withdrawn,reply_to FROM cloud_messages WHERE id=$1")
            .bind(r.get::<Option<Uuid>, _>("input_id"))
            .fetch_one(&mut **tx)
            .await?;
    if trigger.get::<bool, _>("withdrawn")
        || trigger.get::<i64, _>("revision") != r.get::<i64, _>("input_revision")
    {
        return Err(AppError::conflict("触发原文已变化"));
    }
    let mut rows=sqlx::query("SELECT m.* FROM cloud_messages m WHERE m.conversation=$1 AND NOT m.withdrawn AND m.status='committed' AND (m.position<=$2 OR EXISTS(SELECT 1 FROM cloud_node_tasks prior JOIN cloud_messages input ON input.id=prior.input_id WHERE prior.message_id=m.id AND prior.conversation=$1 AND prior.queue_position<$3 AND prior.state='completed' AND input.position<=$2)) ORDER BY m.position DESC LIMIT 100").bind(r.get::<Uuid,_>("conversation")).bind(trigger.get::<i64,_>("position")).bind(r.get::<i64,_>("queue_position")).fetch_all(&mut **tx).await?;
    let mut required = r.get::<Vec<Uuid>, _>("input_ids");
    if let Some(reply) = trigger.get::<Option<Uuid>, _>("reply_to") {
        required.push(reply);
    }
    for id in required {
        if rows.iter().any(|m| m.get::<Uuid, _>("id") == id) {
            continue;
        }
        let row=sqlx::query("SELECT * FROM cloud_messages WHERE id=$1 AND conversation=$2 AND NOT withdrawn AND status='committed'").bind(id).bind(r.get::<Uuid,_>("conversation")).fetch_optional(&mut **tx).await?.ok_or_else(||AppError::conflict("必要原文已失效"))?;
        rows.push(row);
    }
    rows.sort_by_key(|m| m.get::<i64, _>("position"));
    Ok(rows)
}
fn sources(rows: &[sqlx::postgres::PgRow]) -> Value {
    json!(rows.iter().map(|m|json!({"messageId":m.get::<Uuid,_>("id"),"revision":m.get::<i64,_>("revision").to_string()})).collect::<Vec<_>>())
}
async fn check_conversation(tx: &mut Tx<'_>, r: &sqlx::postgres::PgRow) -> Result<()> {
    crate::participation::validate_task(tx, r).await?;
    let cid = r.get::<Uuid, _>("conversation");
    if crate::crypto::enabled(tx, cid).await? {
        let capable:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM cloud_ai_bindings WHERE ai_id=$1 AND version=$2 AND capabilities ? 'sealedV1')").bind(r.get::<Uuid,_>("ai_id")).bind(r.get::<i64,_>("binding_version")).fetch_one(&mut **tx).await?;
        if !capable {
            return Err(crate::crypto::required());
        }
        if r.get::<Option<Uuid>, _>("contact_intent_id").is_some() {
            return Err(crate::crypto::required());
        }
    }
    let rev: i64 = sqlx::query_scalar("SELECT revision FROM cloud_conversations WHERE id=$1")
        .bind(cid)
        .fetch_one(&mut **tx)
        .await?;
    if rev != r.get::<i64, _>("conversation_revision")
        || crate::groups::direct_blocked(tx, cid).await?
    {
        return Err(AppError::conflict("窗口参与权限已变化"));
    }
    Ok(())
}
async fn frozen_history(
    tx: &mut Tx<'_>,
    r: &sqlx::postgres::PgRow,
) -> Result<Vec<sqlx::postgres::PgRow>> {
    check_conversation(tx, r).await?;
    let saved: Value = r.get("sources");
    let mut rows = vec![];
    for source in saved.as_array().unwrap() {
        let row=sqlx::query("SELECT * FROM cloud_messages WHERE id=$1 AND conversation=$2 AND revision=$3 AND NOT withdrawn AND status='committed'").bind(id(source,"messageId")?).bind(r.get::<Uuid,_>("conversation")).bind(revision(source,"revision")?).fetch_optional(&mut **tx).await?.ok_or_else(||AppError::conflict("上下文原文已变化"))?;
        rows.push(row);
    }
    Ok(rows)
}
async fn check_sources(tx: &mut Tx<'_>, r: &sqlx::postgres::PgRow) -> Result<()> {
    frozen_history(tx, r).await?;
    let reference = crate::consciousness::task_reference(tx, r, true).await?;
    let derived =
        serde_json::from_value::<Vec<connector::node::ScopedSource>>(r.get("derived_sources"))
            .unwrap();
    let scopes =
        serde_json::from_value::<Vec<connector::node::MaterialScope>>(r.get("material_scopes"))
            .unwrap();
    crate::consciousness::task_materials_valid(tx, r, &derived, &scopes, reference.as_ref()).await
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
    crate::crypto_resources::ready(&mut tx, n.ai, r.get("conversation")).await?;
    let rows = if r.get::<String, _>("state") == "running" {
        frozen_history(&mut tx, &r).await?
    } else {
        history(&mut tx, &r).await?
    };
    let src = sources(&rows);
    let cid: Uuid = r.get("conversation");
    let conversation = crate::consciousness::node_conversation(&mut tx, cid).await?;
    let reference =
        crate::consciousness::task_reference(&mut tx, &r, r.get::<String, _>("state") == "running")
            .await?;
    let out = json!({"task":task_value(&r),"ai":account(&mut tx,n.ai).await?,"conversation":conversation,"messages":rows.iter().map(crate::consciousness::node_message).collect::<Vec<_>>(),"sources":src,"reference":reference});
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
    crate::crypto_resources::ready(&mut tx, n.ai, r.get("conversation")).await?;
    let rows = if r.get::<String, _>("state") == "running" {
        frozen_history(&mut tx, &r).await?
    } else {
        history(&mut tx, &r).await?
    };
    let mut rows = rows;
    if let Some(reference) =
        crate::consciousness::task_reference(&mut tx, &r, r.get::<String, _>("state") == "running")
            .await?
    {
        for source in reference["sources"].as_array().unwrap() {
            let mid: Uuid = source["messageId"].as_str().unwrap().parse().unwrap();
            rows.push(
                sqlx::query("SELECT * FROM cloud_messages WHERE id=$1")
                    .bind(mid)
                    .fetch_one(&mut *tx)
                    .await?,
            );
        }
    }
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
    crate::handover::active(&mut tx, n.ai).await?;
    if state != "queued" {
        return Err(AppError::conflict("任务已开始或失效"));
    }
    if r.get::<i64, _>("revision") != revision(&v, "taskRevision")? {
        let mut error = AppError::conflict("尚未开始的任务输入已更新，请重新读取");
        error.code = "task_not_ready";
        return Err(error);
    }
    if revision(&v, "conversationRevision")? != r.get::<i64, _>("conversation_revision") {
        return Err(AppError::conflict("窗口版本已变化"));
    }
    if r.get::<chrono::DateTime<chrono::Utc>, _>("not_before") > chrono::Utc::now() {
        let mut e = AppError::conflict("任务仍在等待");
        e.code = "task_not_ready";
        return Err(e);
    }
    let blocked:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM cloud_node_tasks WHERE conversation=$1 AND state='queued' AND (contact_intent_id IS NULL OR not_before<=now()) AND queue_position<$2)").bind(r.get::<Uuid,_>("conversation")).bind(r.get::<i64,_>("queue_position")).fetch_one(&mut *tx).await?;
    if blocked {
        let mut e = AppError::conflict("任务尚未轮到");
        e.code = "task_not_ready";
        return Err(e);
    }
    crate::crypto_resources::ready(&mut tx, n.ai, r.get("conversation")).await?;
    let rows = if r.get::<String, _>("state") == "running" {
        frozen_history(&mut tx, &r).await?
    } else {
        history(&mut tx, &r).await?
    };
    if sources(&rows) != v["sources"] {
        let mut error = AppError::conflict("上下文原文已变化");
        if r.get::<Option<Uuid>, _>("contact_intent_id").is_some() {
            error.code = "task_not_ready";
        }
        return Err(error);
    }
    if sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM cloud_node_tasks WHERE conversation=$1 AND state='running')",
    )
    .bind(r.get::<Uuid, _>("conversation"))
    .fetch_one(&mut *tx)
    .await?
    {
        let mut e = AppError::conflict("窗口已有运行任务");
        e.code = "conversation_busy";
        return Err(e);
    }
    let reference = crate::consciousness::task_reference(&mut tx, &r, false).await?;
    let derived: Vec<connector::node::ScopedSource> =
        serde_json::from_value(v["derivedSources"].clone()).unwrap();
    let scopes: Vec<connector::node::MaterialScope> =
        serde_json::from_value(v["materialScopes"].clone()).unwrap();
    crate::consciousness::task_materials_valid(&mut tx, &r, &derived, &scopes, reference.as_ref())
        .await?;
    let used_reference = reference.filter(|reference| {
        reference["required"] == true
            || derived
                .iter()
                .any(|s| s.conversation_id == reference["conversation"]["id"])
    });
    let reference_metadata=used_reference.map(|reference|json!({"messageId":reference["messageId"],"revision":reference["revision"],"required":reference["required"]}));
    sqlx::query("UPDATE cloud_node_tasks SET derived_sources=$2,material_scopes=$3,reference=$4 WHERE id=$1").bind(tid).bind(&v["derivedSources"]).bind(&v["materialScopes"]).bind(reference_metadata).execute(&mut *tx).await?;
    crate::participation::reserve(&mut tx, &r).await?;
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
pub(crate) async fn finish_message(
    tx: &mut Tx<'_>,
    r: &sqlx::postgres::PgRow,
    text: &str,
    status: &str,
) -> Result<()> {
    let mid: Uuid = r.get("message_id");
    let sealed: Option<Value> = r.get("delta_sealed");
    if text.is_empty() && sealed.is_none() {
        sqlx::query("DELETE FROM cloud_messages WHERE id=$1 AND status='generating'")
            .bind(mid)
            .execute(&mut **tx)
            .await?;
    } else {
        sqlx::query("INSERT INTO cloud_messages(id,conversation,sender,body,status,sender_name,sealed) VALUES($1,$2,$3,$4,$5,(SELECT name FROM accounts WHERE id=$3),$6) ON CONFLICT(id) DO UPDATE SET body=$4,status=$5,sealed=$6,position=nextval(pg_get_serial_sequence('cloud_messages','position')),updated_at=now(),revision=CASE WHEN $6 IS NULL THEN cloud_messages.revision+1 ELSE 1 END").bind(mid).bind(r.get::<Uuid,_>("conversation")).bind(r.get::<Uuid,_>("ai_id")).bind(text).bind(status).bind(sealed).execute(&mut **tx).await?;
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
        || status == "completed" && text.trim().is_empty() && v["sealed"].is_null()
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
    let sealed: Option<connector::crypto::SealedEnvelope> =
        serde_json::from_value(v["sealed"].clone()).unwrap();
    crate::crypto::content(
        &mut tx,
        r.get("conversation"),
        &crate::crypto::Actor {
            id: n.ai,
            node: Some(n.clone()),
        },
        r.get("message_id"),
        1,
        None,
        text,
        sealed.as_ref(),
    )
    .await?;
    if !matches!(
        r.get::<String, _>("state").as_str(),
        "running" | "uncertain"
    ) || r.get::<chrono::DateTime<chrono::Utc>, _>("expires_at") <= chrono::Utc::now()
    {
        return Err(AppError::conflict("任务不接受结果或已过期"));
    }
    let r = sqlx::query("UPDATE cloud_node_tasks SET delta_sealed=$2 WHERE id=$1 RETURNING *")
        .bind(tid)
        .bind(sealed.as_ref().map(|e| json!(e)))
        .fetch_one(&mut *tx)
        .await?;
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
    if status == "completed" {
        crate::participation::completed(&mut tx, &r).await?;
    }
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
    if crate::crypto::enabled(&mut tx, r.get("conversation")).await? && !message.is_empty() {
        return Err(crate::crypto::required());
    }
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
    let r=sqlx::query("SELECT t.* FROM cloud_node_tasks t JOIN cloud_members c ON c.conversation=t.conversation WHERE t.id=$1 AND c.account_id=$2").bind(tid).bind(p.id).fetch_optional(&mut *tx).await?.ok_or_else(AppError::not_found)?;
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
    sqlx::query("UPDATE cloud_summary_calls SET state='interrupted',finished_at=now() WHERE state='running'").execute(&mut *tx).await?;
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
    let mut tick = tokio::time::interval(Duration::from_millis(250));
    loop {
        tick.tick().await;
        if let Err(error) = crate::participation::tick(&pool, chrono::Utc::now()).await {
            eprintln!("Participation maintenance failed: {}", error.code);
        }
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
        sealed,
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
    crate::crypto::content(
        &mut tx,
        r.get("conversation"),
        &crate::crypto::Actor {
            id: n.ai,
            node: Some(n.clone()),
        },
        r.get("message_id"),
        1,
        Some(sequence),
        &text,
        sealed.as_ref(),
    )
    .await?;
    let old = r.get::<i64, _>("delta_sequence") as u64;
    if sequence < old
        || sequence == old
            && (text != r.get::<String, _>("delta_text")
                || sealed.as_ref().map(|e| json!(e)) != r.get::<Option<Value>, _>("delta_sealed"))
    {
        return Err(AppError::conflict("草稿序号已过期"));
    }
    if sequence == old {
        return Ok((tid, sequence));
    }
    sqlx::query(
        "UPDATE cloud_node_tasks SET delta_sequence=$2,delta_text=$3,delta_sealed=$4 WHERE id=$1",
    )
    .bind(tid)
    .bind(sequence as i64)
    .bind(&text)
    .bind(sealed.as_ref().map(|e| json!(e)))
    .execute(&mut *tx)
    .await?;
    if !text.is_empty() || sealed.is_some() {
        sqlx::query("INSERT INTO cloud_messages(id,conversation,sender,body,status,sender_name,sealed) VALUES($1,$2,$3,$4,'generating',(SELECT name FROM accounts WHERE id=$3),$5) ON CONFLICT(id) DO UPDATE SET body=$4,sealed=$5,updated_at=now(),revision=CASE WHEN $5 IS NULL THEN cloud_messages.revision+1 ELSE 1 END").bind(r.get::<Uuid,_>("message_id")).bind(r.get::<Uuid,_>("conversation")).bind(n.ai).bind(text).bind(sealed.as_ref().map(|e|json!(e))).execute(&mut *tx).await?;
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
    let capabilities = match hello {
        Ok(Some(Ok(Message::Text(text)))) => match serde_json::from_str::<ClientFrame>(&text) {
            Ok(ClientFrame::Hello {
                protocol_version: 4,
                node_id,
                ai_id,
                binding_version,
                capabilities,
            }) if node_id == n.node.to_string()
                && ai_id == n.ai.to_string()
                && binding_version == n.version.to_string() =>
            {
                Some(capabilities)
            }
            _ => None,
        },
        _ => None,
    };
    if capabilities.is_none() {
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
    let capabilities = capabilities.unwrap();
    let connection = Uuid::new_v4();
    let initialized:Result<i64>=async {
        let mut tx=s.pool.begin().await?; lock(&mut tx).await?; authenticate(&mut tx,&h).await?;
        sqlx::query("UPDATE cloud_ai_bindings SET connection_id=$2,last_seen_at=now(),capabilities=$3 WHERE ai_id=$1").bind(n.ai).bind(connection).bind(json!(capabilities)).execute(&mut *tx).await?;
        let cursor=sqlx::query_scalar("SELECT COALESCE(max(id),0) FROM cloud_node_events WHERE ai_id=$1").bind(n.ai).fetch_one(&mut *tx).await?;
        sqlx::query("INSERT INTO cloud_events(owner) SELECT administrator FROM cloud_ai_admins WHERE ai_id=$1").bind(n.ai).execute(&mut *tx).await?;
        tx.commit().await?; Ok(cursor)
    }.await;
    let Ok(mut cursor) = initialized else {
        return;
    };
    let (relay_sender, mut relay_receiver) = tokio::sync::mpsc::channel(32);
    s.relay.register(n.ai, n.version, connection, relay_sender);
    s.relay.set_capabilities(n.ai, connection, capabilities);
    if send_frame(
        &mut socket,
        ServerFrame::Ready {
            protocol_version: 4,
        },
    )
    .await
    .is_err()
    {
        s.relay.disconnect(n.ai, connection);
        return;
    }

    let mut tick = tokio::time::interval(Duration::from_secs(1));
    let mut ticks = 0;
    let mut last_received = tokio::time::Instant::now();
    let mut last_received_at = chrono::Utc::now();
    loop {
        tokio::select! {
            frame=relay_receiver.recv()=>{match frame {Some(frame)=>if send_frame(&mut socket,frame).await.is_err(){break;},None=>break,}}
            incoming=socket.recv()=>{
                match incoming {
                    Some(Ok(Message::Text(text)))=>{
                        let parsed=serde_json::from_str::<ClientFrame>(&text);
                        if let Ok(ClientFrame::ManagementResponse{relay_id,request_id,view,error})=&parsed {
                            if let Ok(relay)=relay_id.parse(){s.relay.response(n.ai,n.version,connection,relay,json!({"requestId":request_id,"view":view,"error":error}));}
                            last_received=tokio::time::Instant::now();last_received_at=chrono::Utc::now();continue;
                        }
                        if let Ok(ClientFrame::ConsciousnessResponse{relay_id,request_id,view,error})=parsed {
                            if let Ok(relay)=relay_id.parse(){s.relay.response(n.ai,n.version,connection,relay,json!({"requestId":request_id,"view":view,"error":error}));}
                            last_received=tokio::time::Instant::now();last_received_at=chrono::Utc::now();continue;
                        }
                        let result=match parsed{Ok(frame)=>delta(&s,&h,frame).await,Err(_)=>Err(AppError::bad("节点帧格式无效"))};
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
    s.relay.disconnect(n.ai, connection);
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

async fn status(
    State(s): State<Arc<AppState>>,
    h: HeaderMap,
    Path(ai): Path<Uuid>,
    Json(v): Json<Value>,
) -> Result<Json<Value>> {
    let p = auth::authenticate(&s, &h).await?;
    let mut tx = s.pool.begin().await?;
    lock(&mut tx).await?;
    admin(&mut tx, ai, p.id).await?;
    if admin_receipt(&mut tx, p.id, &v, "status", ai)
        .await?
        .is_some()
    {
        return Ok(Json(view(&mut tx, ai, p.id).await?));
    }
    let status = field(&v, "status")?;
    if !matches!(status, "active" | "disabled") {
        return Err(AppError::bad("账号状态无效"));
    }
    let rev: i64 = sqlx::query_scalar("SELECT status_revision FROM accounts WHERE id=$1")
        .bind(ai)
        .fetch_one(&mut *tx)
        .await?;
    if revision(&v, "expectedStatusRevision")? != rev {
        return Err(AppError::conflict("账号状态版本已变化"));
    }
    sqlx::query("UPDATE accounts SET status=$2,status_revision=status_revision+1 WHERE id=$1")
        .bind(ai)
        .bind(status)
        .execute(&mut *tx)
        .await?;
    if status == "disabled" {
        invalidate_ai(&mut tx, ai, "account_disabled").await?;
        crate::participation::reconcile(&mut tx).await?;
    }
    sqlx::query("INSERT INTO cloud_events(owner) SELECT administrator FROM cloud_ai_admins WHERE ai_id=$1 UNION SELECT m.account_id FROM cloud_members m JOIN cloud_members own ON own.conversation=m.conversation WHERE own.account_id=$1 UNION SELECT owner FROM cloud_contacts WHERE target=$1").bind(ai).execute(&mut *tx).await?;
    let out = view(&mut tx, ai, p.id).await?;
    save_admin_receipt(&mut tx, p.id, &v, "status", ai, &out).await?;
    tx.commit().await?;
    Ok(Json(out))
}
