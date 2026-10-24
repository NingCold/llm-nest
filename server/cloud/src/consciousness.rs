//! Node-owned derived material: cloud authorization, source metadata and online transport only.
use crate::{AppError, AppState, Result, auth, chat, db as sqlx, nodes};
use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    routing::{get, post},
};
use connector::node::{
    ConsciousnessOperation, ConsciousnessView, MaterialScope, ReferenceSelection, ScopedSource,
    SourceRevision, SummaryFinish, SummaryStart, ValidateSources,
};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::{Postgres, Row, Transaction};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::sync::{mpsc, oneshot};
use uuid::Uuid;
type Tx<'a> = Transaction<'a, Postgres>;
fn parse(s: &str) -> Result<Uuid> {
    s.parse().map_err(|_| AppError::bad("标识无效"))
}
fn number(s: &str) -> Result<i64> {
    s.parse().map_err(|_| AppError::bad("版本或游标无效"))
}
fn error(code: &'static str, message: &str) -> AppError {
    AppError {
        status: StatusCode::CONFLICT,
        code,
        message: message.into(),
    }
}
fn fingerprint(v: &impl serde::Serialize) -> String {
    format!("{:x}", Sha256::digest(serde_json::to_vec(v).unwrap()))
}
pub(crate) fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/chat/conversations/{cid}/reference-preview", post(preview))
        .route(
            "/chat/conversations/{cid}/ai/{ai}/consciousness",
            post(manage),
        )
        .route("/node/consciousness/changes", get(changes))
        .route("/node/consciousness/validate", post(validate))
        .route("/node/consciousness/state", post(announce))
        .route("/node/conversations/{cid}/history", get(history))
        .route("/node/summaries/start", post(summary_start))
        .route("/node/summaries/{id}/finish", post(summary_finish))
}
pub(crate) async fn node_conversation(tx: &mut Tx<'_>, cid: Uuid) -> Result<Value> {
    let row = sqlx::query("SELECT kind,name,revision,crypto_enabled,crypto_from_position,crypto_history_complete FROM cloud_conversations WHERE id=$1")
        .bind(cid)
        .fetch_optional(&mut **tx)
        .await?
        .ok_or_else(AppError::not_found)?;
    let ids: Vec<Uuid> = sqlx::query_scalar(
        "SELECT account_id FROM cloud_members WHERE conversation=$1 ORDER BY account_id",
    )
    .bind(cid)
    .fetch_all(&mut **tx)
    .await?;
    let mut members = vec![];
    for id in ids {
        members.push(chat::account(tx, id).await?);
    }
    Ok(
        json!({"id":cid,"kind":row.get::<String,_>("kind"),"name":row.get::<String,_>("name"),"revision":row.get::<i64,_>("revision").to_string(),"members":members,"encryption":crate::crypto::encryption(&row)}),
    )
}
pub(crate) fn node_message(m: &sqlx::postgres::PgRow) -> Value {
    json!({"id":m.get::<Uuid,_>("id"),"senderId":m.get::<Uuid,_>("sender"),"senderName":m.get::<String,_>("sender_name"),"replyTo":m.get::<Option<Uuid>,_>("reply_to"),"mentions":m.get::<Vec<Uuid>,_>("mentions"),"position":m.get::<i64,_>("position").to_string(),"revision":m.get::<i64,_>("revision").to_string(),"text":m.get::<String,_>("body"),"createdAt":m.get::<chrono::DateTime<chrono::Utc>,_>("created_at"),"attachments":m.get::<Value,_>("attachments"),"importOrigin":m.get::<Option<Value>,_>("import_origin"),"sealed":m.get::<Option<Value>,_>("sealed"),"historyMigration":m.get::<Option<Value>,_>("history_migration"),"audienceRevision":m.get::<Option<i64>,_>("crypto_audience_revision").map(|v|v.to_string()),"contextReference":m.get::<Option<Value>,_>("context_reference")})
}
async fn is_member(tx: &mut Tx<'_>, cid: Uuid, actor: Uuid) -> Result<bool> {
    Ok(sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM cloud_members WHERE conversation=$1 AND account_id=$2)",
    )
    .bind(cid)
    .bind(actor)
    .fetch_one(&mut **tx)
    .await?)
}
async fn valid_message(tx: &mut Tx<'_>, cid: Uuid, id: Uuid, revision: i64) -> Result<bool> {
    Ok(sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM cloud_messages WHERE id=$1 AND conversation=$2 AND revision=$3 AND status='committed' AND NOT withdrawn AND NOT coalesce((context_reference->>'stale')::boolean,false))").bind(id).bind(cid).bind(revision).fetch_one(&mut **tx).await?)
}
pub(crate) async fn scoped_valid(
    tx: &mut Tx<'_>,
    ai: Uuid,
    sources: &[ScopedSource],
) -> Result<bool> {
    if sources.len() > 512 {
        return Err(AppError::bad("来源数量超限"));
    }
    for source in sources {
        let cid = parse(&source.conversation_id)?;
        if !is_member(tx, cid, ai).await?
            || !valid_message(
                tx,
                cid,
                parse(&source.message_id)?,
                number(&source.revision)?,
            )
            .await?
        {
            return Ok(false);
        }
    }
    Ok(true)
}
pub(crate) async fn scopes_valid(
    tx: &mut Tx<'_>,
    ai: Uuid,
    scopes: &[MaterialScope],
) -> Result<bool> {
    if scopes.len() > 512 {
        return Err(AppError::bad("材料范围数量超限"));
    }
    for scope in scopes {
        let cid = parse(&scope.conversation_id)?;
        let epoch = parse(&scope.revision)?;
        if !is_member(tx, cid, ai).await? {
            return Ok(false);
        }
        let actual: Option<Uuid> = sqlx::query_scalar(
            "SELECT revision FROM cloud_material_epochs WHERE ai_id=$1 AND conversation=$2",
        )
        .bind(ai)
        .bind(cid)
        .fetch_optional(&mut **tx)
        .await?;
        if actual != Some(epoch) {
            return Ok(false);
        }
    }
    Ok(true)
}
async fn event(
    tx: &mut Tx<'_>,
    cid: Uuid,
    mid: Option<Uuid>,
    reason: &str,
    extra: &[Uuid],
) -> Result<()> {
    sqlx::query("INSERT INTO cloud_source_changes(ai_id,conversation,message_id,reason) SELECT DISTINCT a.id,$1,$2,$3 FROM accounts a WHERE a.kind='ai' AND (a.id=ANY($4) OR EXISTS(SELECT 1 FROM cloud_members m WHERE m.conversation=$1 AND m.account_id=a.id))").bind(cid).bind(mid).bind(reason).bind(extra).execute(&mut **tx).await?;
    // Existing task events are only a wakeup; the source log is the durable recovery path.
    sqlx::query("INSERT INTO cloud_node_events(ai_id,binding_version,kind) SELECT DISTINCT b.ai_id,b.version,'sourcesChanged' FROM cloud_ai_bindings b JOIN accounts a ON a.id=b.ai_id WHERE a.kind='ai' AND (a.id=ANY($2) OR EXISTS(SELECT 1 FROM cloud_members m WHERE m.conversation=$1 AND m.account_id=a.id))").bind(cid).bind(extra).execute(&mut **tx).await?;
    Ok(())
}
async fn stale_references(tx: &mut Tx<'_>, source: Option<Uuid>, group: Uuid) -> Result<()> {
    let rows=sqlx::query("UPDATE cloud_messages m SET context_reference=jsonb_set(jsonb_set(context_reference,'{stale}','true'),'{revision}',to_jsonb(((context_reference->>'revision')::bigint+1)::text)) WHERE context_reference IS NOT NULL AND NOT (context_reference->>'stale')::boolean AND EXISTS(SELECT 1 FROM cloud_reference_sources r WHERE r.message_id=m.id AND r.conversation=$1 AND ($2::uuid IS NOT NULL AND r.source_id=$2 OR $2::uuid IS NULL AND EXISTS(SELECT 1 FROM cloud_members dm WHERE dm.conversation=m.conversation AND NOT EXISTS(SELECT 1 FROM cloud_members gm WHERE gm.conversation=r.conversation AND gm.account_id=dm.account_id)))) RETURNING id,conversation").bind(group).bind(source).fetch_all(&mut **tx).await?;
    for row in rows {
        let mid = row.get("id");
        let cid = row.get("conversation");
        crate::crypto_resources::source_changed(tx, mid).await?;
        nodes::source_changed(tx, mid).await?;
        event(tx, cid, Some(mid), "reference_stale", &[]).await?;
        let members: Vec<Uuid> =
            sqlx::query_scalar("SELECT account_id FROM cloud_members WHERE conversation=$1")
                .bind(cid)
                .fetch_all(&mut **tx)
                .await?;
        chat::notify(tx, cid, members).await?;
    }
    Ok(())
}
pub(crate) async fn message_changed(tx: &mut Tx<'_>, mid: Uuid) -> Result<()> {
    let cid: Uuid = sqlx::query_scalar("SELECT conversation FROM cloud_messages WHERE id=$1")
        .bind(mid)
        .fetch_one(&mut **tx)
        .await?;
    crate::crypto_resources::source_changed(tx, mid).await?;
    event(tx, cid, Some(mid), "source_changed", &[]).await?;
    stale_references(tx, Some(mid), cid).await?;
    nodes::source_changed(tx, mid).await?;
    sqlx::query("UPDATE cloud_summary_calls SET state='stale',finished_at=now() WHERE state='running' AND EXISTS(SELECT 1 FROM jsonb_array_elements(sources) s WHERE s->>'messageId'=$1::text)").bind(mid).execute(&mut **tx).await?;
    Ok(())
}
pub(crate) async fn membership_changed(tx: &mut Tx<'_>, cid: Uuid, old: &[Uuid]) -> Result<()> {
    event(tx, cid, None, "window_changed", &[]).await?;
    let current: Vec<Uuid> =
        sqlx::query_scalar("SELECT account_id FROM cloud_members WHERE conversation=$1")
            .bind(cid)
            .fetch_all(&mut **tx)
            .await?;
    let removed: Vec<Uuid> = old
        .iter()
        .copied()
        .filter(|id| !current.contains(id))
        .collect();
    crate::crypto_resources::revoke_access(tx, cid, &removed).await?;
    sqlx::query("INSERT INTO cloud_source_changes(ai_id,conversation,reason) SELECT id,$1,'permission_revoked' FROM accounts WHERE kind='ai' AND id=ANY($2)").bind(cid).bind(&removed).execute(&mut **tx).await?;
    sqlx::query("INSERT INTO cloud_node_events(ai_id,binding_version,kind) SELECT ai_id,version,'sourcesChanged' FROM cloud_ai_bindings WHERE ai_id=ANY($1)").bind(&removed).execute(&mut **tx).await?;
    stale_references(tx, None, cid).await?;
    sqlx::query("UPDATE cloud_summary_calls SET state='stale',finished_at=now() WHERE conversation=$1 AND state='running'").bind(cid).execute(&mut **tx).await?;
    Ok(())
}
#[derive(Deserialize)]
struct Preview {
    reference: ReferenceSelection,
}
async fn direct_participants(tx: &mut Tx<'_>, cid: Uuid, human: Uuid) -> Result<(Uuid, Uuid)> {
    let members = chat::member(tx, cid, human).await?;
    let kind: String = sqlx::query_scalar("SELECT kind FROM cloud_conversations WHERE id=$1")
        .bind(cid)
        .fetch_one(&mut **tx)
        .await?;
    if kind != "direct" {
        return Err(AppError::bad("引用只可发送到真人与AI私聊"));
    }
    let peer = *members.iter().find(|&&m| m != human).unwrap();
    if chat::account(tx, peer).await?["kind"] != "ai" {
        return Err(AppError::bad("引用需要AI私聊"));
    }
    Ok((human, peer))
}
async fn reference_rows(
    tx: &mut Tx<'_>,
    direct: Uuid,
    human: Uuid,
    selection: &ReferenceSelection,
) -> Result<Vec<sqlx::postgres::PgRow>> {
    let (_, ai) = direct_participants(tx, direct, human).await?;
    let group = parse(&selection.conversation_id)?;
    let kind: Option<String> =
        sqlx::query_scalar("SELECT kind FROM cloud_conversations WHERE id=$1")
            .bind(group)
            .fetch_optional(&mut **tx)
            .await?;
    if kind.as_deref() != Some("group")
        || !is_member(tx, group, human).await?
        || !is_member(tx, group, ai).await?
    {
        return Err(AppError::forbidden());
    }
    if !(1..=8).contains(&selection.sources.len()) {
        return Err(AppError::bad("引用须含1至8条群原文"));
    }
    let mut seen = std::collections::HashSet::new();
    let mut rows = vec![];
    for source in &selection.sources {
        let mid = parse(&source.message_id)?;
        if !seen.insert(mid) {
            return Err(AppError::bad("引用来源重复"));
        }
        let rev = number(&source.revision)?;
        if !valid_message(tx, group, mid, rev).await? {
            return Err(AppError::conflict("引用原文已失效"));
        }
        rows.push(
            sqlx::query("SELECT * FROM cloud_messages WHERE id=$1")
                .bind(mid)
                .fetch_one(&mut **tx)
                .await?,
        );
    }
    rows.sort_by_key(|r| r.get::<i64, _>("position"));
    Ok(rows)
}
pub(crate) async fn direct_reference(
    tx: &mut Tx<'_>,
    cid: Uuid,
    human: Uuid,
    mid: Uuid,
    selection: &ReferenceSelection,
) -> Result<Value> {
    let rows = reference_rows(tx, cid, human, selection).await?;
    Ok(
        json!({"messageId":mid,"revision":"1","required":true,"conversation":node_conversation(tx,parse(&selection.conversation_id)?).await?,"messages":rows.iter().map(node_message).collect::<Vec<_>>(),"sources":selection.sources}),
    )
}
async fn preview(
    State(s): State<Arc<AppState>>,
    h: HeaderMap,
    Path(cid): Path<Uuid>,
    Json(v): Json<Preview>,
) -> Result<Json<Value>> {
    let p = auth::authenticate(&s, &h).await?;
    let mut tx = s.pool.begin().await?;
    nodes::lock(&mut tx).await?;
    let rows = reference_rows(&mut tx, cid, p.id, &v.reference).await?;
    Ok(Json(
        json!({"conversation":node_conversation(&mut tx,parse(&v.reference.conversation_id)?).await?,"messages":rows.iter().map(node_message).collect::<Vec<_>>()}),
    ))
}
pub(crate) async fn save_reference(
    tx: &mut Tx<'_>,
    cid: Uuid,
    mid: Uuid,
    human: Uuid,
    selection: &ReferenceSelection,
) -> Result<()> {
    reference_rows(tx, cid, human, selection).await?;
    let reference = json!({"conversationId":selection.conversation_id,"sources":selection.sources,"revision":"1","stale":false});
    sqlx::query("UPDATE cloud_messages SET context_reference=$2 WHERE id=$1")
        .bind(mid)
        .bind(reference)
        .execute(&mut **tx)
        .await?;
    for source in &selection.sources {
        sqlx::query("INSERT INTO cloud_reference_sources(message_id,conversation,source_id,source_revision) VALUES($1,$2,$3,$4)").bind(mid).bind(parse(&selection.conversation_id)?).bind(parse(&source.message_id)?).bind(number(&source.revision)?).execute(&mut **tx).await?;
    }
    Ok(())
}

pub(crate) async fn task_reference(
    tx: &mut Tx<'_>,
    task: &sqlx::postgres::PgRow,
    frozen: bool,
) -> Result<Option<Value>> {
    if task.get::<Option<Uuid>, _>("input_id").is_none() {
        return Ok(None);
    }
    let cid: Uuid = task.get("conversation");
    let ai: Uuid = task.get("ai_id");
    let ckind: String = sqlx::query_scalar("SELECT kind FROM cloud_conversations WHERE id=$1")
        .bind(cid)
        .fetch_one(&mut **tx)
        .await?;
    if ckind != "direct" {
        return Ok(None);
    }
    let human: Uuid = sqlx::query_scalar(
        "SELECT account_id FROM cloud_members WHERE conversation=$1 AND account_id<>$2",
    )
    .bind(cid)
    .bind(ai)
    .fetch_one(&mut **tx)
    .await?;
    let selected = if frozen {
        let saved: Option<Value> = task.get("reference");
        saved.map(|s| {
            (
                s["messageId"].as_str().unwrap().parse::<Uuid>().unwrap(),
                s["required"].as_bool().unwrap(),
                Some(s["revision"].as_str().unwrap().to_owned()),
            )
        })
    } else {
        let trigger = sqlx::query("SELECT * FROM cloud_messages WHERE id=$1")
            .bind(task.get::<Uuid, _>("input_id"))
            .fetch_one(&mut **tx)
            .await?;
        if trigger
            .get::<Option<Value>, _>("context_reference")
            .is_some()
        {
            Some((trigger.get("id"), true, None))
        } else if let Some(reply) = trigger.get::<Option<Uuid>, _>("reply_to") {
            let r = sqlx::query("SELECT context_reference FROM cloud_messages WHERE id=$1")
                .bind(reply)
                .fetch_one(&mut **tx)
                .await?;
            if r.get::<Option<Value>, _>(0).is_some() {
                Some((reply, true, None))
            } else {
                incidental(tx, cid, trigger.get("position")).await?
            }
        } else {
            incidental(tx, cid, trigger.get("position")).await?
        }
    };
    let Some((mid, required, expected)) = selected else {
        return Ok(None);
    };
    let row=sqlx::query("SELECT context_reference FROM cloud_messages WHERE id=$1 AND conversation=$2 AND NOT withdrawn AND status='committed'").bind(mid).bind(cid).fetch_optional(&mut **tx).await?;
    let reference = row.and_then(|r| r.get::<Option<Value>, _>(0));
    let Some(reference) = reference else {
        if required || frozen {
            return Err(error("reference_stale", "引用已失效"));
        }
        return Ok(None);
    };
    if reference["stale"] == true
        || expected
            .as_deref()
            .is_some_and(|v| reference["revision"] != v)
    {
        if required || frozen {
            return Err(error("reference_stale", "引用已失效"));
        }
        return Ok(None);
    }
    let selection: ReferenceSelection =
        serde_json::from_value(reference.clone()).map_err(|_| AppError::internal())?;
    let rows = match reference_rows(tx, cid, human, &selection).await {
        Ok(r) => r,
        Err(e)
            if !required
                && !frozen
                && matches!(
                    e.status,
                    StatusCode::FORBIDDEN | StatusCode::CONFLICT | StatusCode::NOT_FOUND
                ) =>
        {
            return Ok(None);
        }
        Err(e) => return Err(e),
    };
    Ok(Some(
        json!({"messageId":mid,"revision":reference["revision"],"required":required,"conversation":node_conversation(tx,parse(&selection.conversation_id)?).await?,"messages":rows.iter().map(node_message).collect::<Vec<_>>(),"sources":selection.sources}),
    ))
}
async fn incidental(
    tx: &mut Tx<'_>,
    cid: Uuid,
    position: i64,
) -> Result<Option<(Uuid, bool, Option<String>)>> {
    let rows=sqlx::query("SELECT id,context_reference FROM cloud_messages WHERE conversation=$1 AND position<$2 AND status='committed' AND NOT withdrawn ORDER BY position DESC LIMIT 12").bind(cid).bind(position).fetch_all(&mut **tx).await?;
    Ok(rows.into_iter().find_map(|r| {
        r.get::<Option<Value>, _>("context_reference")
            .filter(|v| v["stale"] == false)
            .map(|_| (r.get("id"), false, None))
    }))
}
pub(crate) async fn task_materials_valid(
    tx: &mut Tx<'_>,
    task: &sqlx::postgres::PgRow,
    derived: &[ScopedSource],
    scopes: &[MaterialScope],
    reference: Option<&Value>,
) -> Result<()> {
    let cid: Uuid = task.get("conversation");
    let ai: Uuid = task.get("ai_id");
    let refcid = reference
        .map(|r| parse(r["conversation"]["id"].as_str().unwrap()))
        .transpose()?;
    if derived.iter().any(|s| {
        parse(&s.conversation_id).ok() != Some(cid) && parse(&s.conversation_id).ok() != refcid
    }) || scopes.iter().any(|s| {
        parse(&s.conversation_id).ok() != Some(cid) && parse(&s.conversation_id).ok() != refcid
    }) {
        return Err(AppError::forbidden());
    }
    if !scopes.iter().any(|s| s.conversation_id == cid.to_string()) {
        return Err(AppError::bad("缺少当前窗口材料版本"));
    }
    if !scoped_valid(tx, ai, derived).await? || !scopes_valid(tx, ai, scopes).await? {
        return Err(error("material_stale", "意识材料来源或版本已失效"));
    }
    if let Some(reference) = reference {
        let used = reference["required"] == true
            || derived
                .iter()
                .any(|s| Some(parse(&s.conversation_id).unwrap()) == refcid);
        if used {
            for source in reference["sources"].as_array().unwrap() {
                if !derived.iter().any(|s| {
                    Some(parse(&s.conversation_id).unwrap()) == refcid
                        && s.message_id == source["messageId"]
                        && s.revision == source["revision"]
                }) {
                    return Err(AppError::bad("缺少必要引用来源"));
                }
            }
        }
    }
    Ok(())
}
#[derive(Deserialize)]
struct ChangesQuery {
    after: Option<String>,
}
async fn changes(
    State(s): State<Arc<AppState>>,
    h: HeaderMap,
    Query(q): Query<ChangesQuery>,
) -> Result<Json<Value>> {
    let after = number(q.after.as_deref().unwrap_or("0"))?;
    if after < 0 {
        return Err(AppError::bad("游标无效"));
    }
    let mut tx = s.pool.begin().await?;
    let n = nodes::authenticate(&mut tx, &h).await?;
    let mut rows = sqlx::query(
        "SELECT * FROM cloud_source_changes WHERE ai_id=$1 AND id>$2 ORDER BY id LIMIT 201",
    )
    .bind(n.ai)
    .bind(after)
    .fetch_all(&mut *tx)
    .await?;
    let more = rows.len() > 200;
    rows.truncate(200);
    let cursor = rows.last().map(|r| r.get::<i64, _>("id")).unwrap_or(after);
    Ok(Json(
        json!({"cursor":cursor.to_string(),"hasMore":more,"changes":rows.iter().map(|r|json!({"id":r.get::<i64,_>("id").to_string(),"conversationId":r.get::<Uuid,_>("conversation"),"messageId":r.get::<Option<Uuid>,_>("message_id"),"reason":r.get::<String,_>("reason")})).collect::<Vec<_>>()}),
    ))
}
async fn validate(
    State(s): State<Arc<AppState>>,
    h: HeaderMap,
    Json(v): Json<ValidateSources>,
) -> Result<Json<Value>> {
    let mut tx = s.pool.begin().await?;
    nodes::lock(&mut tx).await?;
    let n = nodes::authenticate(&mut tx, &h).await?;
    if v.conversations.len() > 512 {
        return Err(AppError::bad("范围数量超限"));
    }
    let mut valid = scoped_valid(&mut tx, n.ai, &v.sources).await?;
    for c in &v.conversations {
        let cid = parse(&c.conversation_id)?;
        let rev = number(&c.revision)?;
        let yes:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM cloud_conversations c JOIN cloud_members m ON m.conversation=c.id WHERE c.id=$1 AND c.revision=$2 AND m.account_id=$3)").bind(cid).bind(rev).bind(n.ai).fetch_one(&mut *tx).await?;
        valid &= yes;
    }
    Ok(Json(json!({"valid":valid})))
}
async fn announce(
    State(s): State<Arc<AppState>>,
    h: HeaderMap,
    Json(v): Json<MaterialScope>,
) -> Result<Json<Value>> {
    let cid = parse(&v.conversation_id)?;
    let epoch = parse(&v.revision)?;
    let mut tx = s.pool.begin().await?;
    nodes::lock(&mut tx).await?;
    let n = nodes::authenticate(&mut tx, &h).await?;
    if !is_member(&mut tx, cid, n.ai).await? {
        return Err(AppError::forbidden());
    }
    let previous: Option<Uuid> = sqlx::query_scalar(
        "SELECT revision FROM cloud_material_epochs WHERE ai_id=$1 AND conversation=$2",
    )
    .bind(n.ai)
    .bind(cid)
    .fetch_optional(&mut *tx)
    .await?;
    if previous != Some(epoch) {
        sqlx::query("INSERT INTO cloud_material_epochs(ai_id,conversation,revision) VALUES($1,$2,$3) ON CONFLICT(ai_id,conversation) DO UPDATE SET revision=$3").bind(n.ai).bind(cid).bind(epoch).execute(&mut *tx).await?;
        let rows=sqlx::query("SELECT * FROM cloud_node_tasks WHERE ai_id=$1 AND state IN ('queued','running','uncertain') AND EXISTS(SELECT 1 FROM jsonb_array_elements(material_scopes) s WHERE s->>'conversationId'=$2::text)").bind(n.ai).bind(cid).fetch_all(&mut *tx).await?;
        nodes::invalidate_rows(&mut tx, rows, "material_changed").await?;
        sqlx::query("UPDATE cloud_summary_calls SET state='stale',finished_at=now() WHERE ai_id=$1 AND conversation=$2 AND state='running' AND material_scope->>'revision'<>$3").bind(n.ai).bind(cid).bind(epoch.to_string()).execute(&mut *tx).await?;
    }
    tx.commit().await?;
    Ok(Json(json!({"ok":true})))
}
#[derive(Deserialize)]
struct HistoryQuery {
    after: Option<String>,
    limit: Option<i64>,
}
async fn history(
    State(s): State<Arc<AppState>>,
    h: HeaderMap,
    Path(cid): Path<Uuid>,
    Query(q): Query<HistoryQuery>,
) -> Result<Json<Value>> {
    let mut tx = s.pool.begin().await?;
    nodes::lock(&mut tx).await?;
    let n = nodes::authenticate(&mut tx, &h).await?;
    chat::member(&mut tx, cid, n.ai).await?;
    crate::history_migration::node_capability(&mut tx, n.ai, n.version, cid).await?;
    let rows=sqlx::query("SELECT * FROM cloud_messages WHERE conversation=$1 AND position>$2 AND status='committed' AND NOT withdrawn ORDER BY position LIMIT $3").bind(cid).bind(number(q.after.as_deref().unwrap_or("0"))?).bind(q.limit.unwrap_or(100).clamp(1,100)).fetch_all(&mut *tx).await?;
    let boundary:Option<i64>=sqlx::query_scalar("SELECT min(position) FROM (SELECT position FROM cloud_messages WHERE conversation=$1 AND status='committed' AND NOT withdrawn ORDER BY position DESC LIMIT 12) m").bind(cid).fetch_one(&mut *tx).await?;
    Ok(Json(
        json!({"conversation":node_conversation(&mut tx,cid).await?,"messages":rows.iter().map(node_message).collect::<Vec<_>>(),"recentBoundary":boundary.map(|p|p.to_string())}),
    ))
}

#[derive(Clone)]
struct Live {
    version: i64,
    connection: Uuid,
    sender: mpsc::Sender<connector::node::ServerFrame>,
    capabilities: Vec<String>,
}
struct Pending {
    ai: Uuid,
    version: i64,
    connection: Uuid,
    sender: oneshot::Sender<Value>,
}
#[derive(Default)]
struct HubState {
    live: HashMap<Uuid, Live>,
    pending: HashMap<Uuid, Pending>,
}
#[derive(Default)]
pub struct RelayHub {
    inner: Mutex<HubState>,
}
impl RelayHub {
    pub(crate) fn register(
        &self,
        ai: Uuid,
        version: i64,
        connection: Uuid,
        sender: mpsc::Sender<connector::node::ServerFrame>,
    ) {
        let mut h = self.inner.lock().unwrap();
        h.pending.retain(|_, p| p.ai != ai);
        h.live.insert(
            ai,
            Live {
                version,
                connection,
                sender,
                capabilities: vec![],
            },
        );
    }
    pub(crate) fn set_capabilities(&self, ai: Uuid, connection: Uuid, capabilities: Vec<String>) {
        if let Some(live) = self
            .inner
            .lock()
            .unwrap()
            .live
            .get_mut(&ai)
            .filter(|l| l.connection == connection)
        {
            live.capabilities = capabilities;
        }
    }
    fn supports_profile(&self, ai: Uuid, version: i64, connection: Uuid) -> Result<()> {
        let h = self.inner.lock().unwrap();
        let live = h
            .live
            .get(&ai)
            .filter(|l| l.version == version && l.connection == connection)
            .ok_or_else(|| error("node_offline", "节点不在线，未排队修改"))?;
        if !live.capabilities.iter().any(|v| v == "profileManagement") {
            return Err(error(
                "unsupported_capability",
                "节点不支持角色和模型在线管理",
            ));
        }
        Ok(())
    }
    pub(crate) fn disconnect(&self, ai: Uuid, connection: Uuid) {
        let mut h = self.inner.lock().unwrap();
        if h.live.get(&ai).is_some_and(|l| l.connection == connection) {
            h.live.remove(&ai);
        }
        h.pending
            .retain(|_, p| p.ai != ai || p.connection != connection);
    }
    pub(crate) fn response(
        &self,
        ai: Uuid,
        version: i64,
        connection: Uuid,
        relay: Uuid,
        response: Value,
    ) {
        let mut h = self.inner.lock().unwrap();
        if h.pending
            .get(&relay)
            .is_some_and(|p| p.ai == ai && p.version == version && p.connection == connection)
            && let Some(p) = h.pending.remove(&relay)
        {
            let _ = p.sender.send(response);
        }
    }
    fn dispatch(
        &self,
        ai: Uuid,
        version: i64,
        connection: Uuid,
        relay: Uuid,
        frame: connector::node::ServerFrame,
    ) -> Result<oneshot::Receiver<Value>> {
        let mut h = self.inner.lock().unwrap();
        let live = h
            .live
            .get(&ai)
            .filter(|l| l.version == version && l.connection == connection)
            .cloned()
            .ok_or_else(|| error("node_offline", "节点不在线，未排队修改"))?;
        if h.pending.len() >= 128 {
            return Err(error("relay_busy", "在线管理请求过多，请稍后重试"));
        }
        let (sender, receiver) = oneshot::channel();
        h.pending.insert(
            relay,
            Pending {
                ai,
                version,
                connection,
                sender,
            },
        );
        if live.sender.try_send(frame).is_err() {
            h.pending.remove(&relay);
            return Err(error("node_offline", "节点连接不可用，未排队修改"));
        }
        Ok(receiver)
    }
    fn forget(&self, relay: Uuid) {
        self.inner.lock().unwrap().pending.remove(&relay);
    }
}
// The HTTP future may be dropped before its timeout or node response arrives.
struct RelayLease<'a> {
    hub: &'a RelayHub,
    id: Uuid,
}
impl Drop for RelayLease<'_> {
    fn drop(&mut self) {
        self.hub.forget(self.id);
    }
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Management {
    request_id: Uuid,
    operation: ConsciousnessOperation,
}
async fn management_permission(
    tx: &mut Tx<'_>,
    cid: Uuid,
    ai: Uuid,
    human: Uuid,
    summary: bool,
) -> Result<()> {
    if !is_member(tx, cid, human).await? || !is_member(tx, cid, ai).await? {
        return Err(AppError::forbidden());
    }
    if chat::account(tx, ai).await?["kind"] != "ai" {
        return Err(AppError::bad("目标不是AI账号"));
    }
    if summary {
        nodes::admin(tx, ai, human).await?;
    }
    Ok(())
}
async fn manage(
    State(s): State<Arc<AppState>>,
    h: HeaderMap,
    Path((cid, ai)): Path<(Uuid, Uuid)>,
    Json(v): Json<Management>,
) -> Result<axum::response::Response> {
    use axum::response::IntoResponse;
    let p = auth::authenticate(&s, &h).await?;
    let summary = matches!(
        v.operation,
        ConsciousnessOperation::Summaries
            | ConsciousnessOperation::ConfigureSummaries { .. }
            | ConsciousnessOperation::GenerateSummary { .. }
    );
    if serde_json::to_vec(&v.operation).unwrap().len() > 64 * 1024 {
        return Err(AppError::bad("管理请求过大"));
    }
    let mut tx = s.pool.begin().await?;
    nodes::lock(&mut tx).await?;
    management_permission(&mut tx, cid, ai, p.id, summary).await?;
    if s.require_encryption {
        return Err(crate::crypto::required());
    }
    crate::history_migration::frozen(&mut tx, cid).await?;
    crate::crypto::plaintext(&mut tx, cid).await?;
    if !matches!(
        v.operation,
        ConsciousnessOperation::Memories | ConsciousnessOperation::Summaries
    ) {
        chat::writable(&mut tx, cid).await?;
    }
    let b = sqlx::query("SELECT version,connection_id FROM cloud_ai_bindings WHERE ai_id=$1")
        .bind(ai)
        .fetch_one(&mut *tx)
        .await?;
    let version: i64 = b.get("version");
    let connection = b
        .get::<Option<Uuid>, _>("connection_id")
        .ok_or_else(|| error("node_offline", "节点不在线，未排队修改"))?;
    let relay = Uuid::new_v4();
    let frame:connector::node::ServerFrame=serde_json::from_value(json!({"type":"consciousnessRequest","relayId":relay,"requestId":v.request_id,"operatorId":p.id,"conversation":node_conversation(&mut tx,cid).await?,"operation":v.operation})).map_err(|_|AppError::internal())?;
    let receiver = s.relay.dispatch(ai, version, connection, relay, frame)?;
    let _lease = RelayLease {
        hub: &s.relay,
        id: relay,
    };
    tx.commit().await?;
    let response = tokio::time::timeout(Duration::from_secs(30), receiver).await;
    let response = match response {
        Ok(Ok(v)) => v,
        Ok(Err(_)) => {
            return Err(error(
                "node_disconnected",
                "节点连接中断，保存结果尚未确认，请保留原命令重试",
            ));
        }
        Err(_) => {
            return Err(error(
                "node_timeout",
                "节点响应超时，保存结果尚未确认，请保留原命令重试",
            ));
        }
    };
    let current = auth::authenticate(&s, &h).await?;
    let mut tx = s.pool.begin().await?;
    nodes::lock(&mut tx).await?;
    management_permission(&mut tx, cid, ai, current.id, summary).await?;
    let same:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM cloud_ai_bindings WHERE ai_id=$1 AND version=$2 AND connection_id=$3)").bind(ai).bind(version).bind(connection).fetch_one(&mut *tx).await?;
    if !same {
        return Err(error("node_replaced", "节点连接已变化，未确认此次结果"));
    }
    if let Some(e) = response.get("error").filter(|e| !e.is_null()) {
        let e: connector::node::ConsciousnessError =
            serde_json::from_value(e.clone()).map_err(|_| AppError::bad("节点错误响应无效"))?;
        if e.code.len() > 128 || e.message.len() > 4096 {
            return Err(AppError::bad("节点错误响应过长"));
        }
        return Ok((
            StatusCode::CONFLICT,
            Json(json!({"code":e.code,"message":e.message})),
        )
            .into_response());
    }
    let view: ConsciousnessView = serde_json::from_value(response["view"].clone())
        .map_err(|_| AppError::bad("节点未返回有效管理结果"))?;
    Ok(Json(view).into_response())
}
async fn summary_sources(
    tx: &mut Tx<'_>,
    ai: Uuid,
    cid: Uuid,
    revision: i64,
    sources: &[SourceRevision],
    scope: &MaterialScope,
) -> Result<bool> {
    if sources.is_empty() || sources.len() > 32 {
        return Err(AppError::bad("摘要来源数量须为1至32"));
    }
    let active:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM accounts a JOIN cloud_members m ON m.account_id=a.id JOIN cloud_conversations c ON c.id=m.conversation WHERE a.id=$1 AND a.status='active' AND c.id=$2 AND c.revision=$3)").bind(ai).bind(cid).bind(revision).fetch_one(&mut **tx).await?;
    if !active
        || scope.conversation_id != cid.to_string()
        || !scopes_valid(tx, ai, std::slice::from_ref(scope)).await?
    {
        return Ok(false);
    }
    let scoped: Vec<ScopedSource> = sources
        .iter()
        .map(|s| ScopedSource {
            conversation_id: cid.to_string(),
            message_id: s.message_id.clone(),
            revision: s.revision.clone(),
        })
        .collect();
    scoped_valid(tx, ai, &scoped).await
}
async fn summary_start(
    State(s): State<Arc<AppState>>,
    h: HeaderMap,
    Json(v): Json<SummaryStart>,
) -> Result<Json<Value>> {
    let cid = parse(&v.conversation_id)?;
    let rid = parse(&v.request_id)?;
    let rev = number(&v.conversation_revision)?;
    if v.settings_revision.is_empty() || v.settings_revision.len() > 256 {
        return Err(AppError::bad("摘要配置版本无效"));
    }
    let mut tx = s.pool.begin().await?;
    nodes::lock(&mut tx).await?;
    let n = nodes::authenticate(&mut tx, &h).await?;
    crate::history_migration::frozen(&mut tx, cid).await?;
    crate::history_migration::node_capability(&mut tx, n.ai, n.version, cid).await?;
    if s.require_encryption && !crate::crypto::enabled(&mut tx, cid).await? {
        return Err(crate::crypto::required());
    }
    if !summary_sources(&mut tx, n.ai, cid, rev, &v.sources, &v.material_scope).await? {
        return Err(error("source_stale", "摘要来源、权限或配置已失效"));
    }
    chat::writable(&mut tx, cid).await?;
    let fp = fingerprint(&v);
    if let Some(row) =
        sqlx::query("SELECT * FROM cloud_summary_calls WHERE ai_id=$1 AND request_id=$2")
            .bind(n.ai)
            .bind(rid)
            .fetch_optional(&mut *tx)
            .await?
    {
        if row.get::<String, _>("fingerprint") != fp
            || row.get::<i64, _>("binding_version") != n.version
        {
            return Err(AppError::conflict("摘要开始请求已用于其他内容或节点"));
        }
        if row.get::<String, _>("state") != "running" {
            return Err(error("summary_finished", "摘要调用已结束，不能重新开始"));
        }
        return Ok(Json(
            json!({"id":row.get::<Uuid,_>("id"),"confirmationId":row.get::<Uuid,_>("confirmation_id")}),
        ));
    }
    let id = Uuid::new_v4();
    let confirmation = Uuid::new_v4();
    sqlx::query("INSERT INTO cloud_summary_calls(id,ai_id,request_id,fingerprint,binding_version,conversation,conversation_revision,sources,settings_revision,material_scope,confirmation_id) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11)").bind(id).bind(n.ai).bind(rid).bind(fp).bind(n.version).bind(cid).bind(rev).bind(json!(v.sources)).bind(v.settings_revision).bind(json!(v.material_scope)).bind(confirmation).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(Json(json!({"id":id,"confirmationId":confirmation})))
}
async fn summary_finish(
    State(s): State<Arc<AppState>>,
    h: HeaderMap,
    Path(id): Path<Uuid>,
    Json(v): Json<SummaryFinish>,
) -> Result<Json<Value>> {
    if !matches!(
        v.status.as_str(),
        "ready" | "failed" | "interrupted" | "stale"
    ) || v.error_code.as_ref().is_some_and(|s| s.len() > 256)
    {
        return Err(AppError::bad("摘要终态无效"));
    }
    let mut tx = s.pool.begin().await?;
    nodes::lock(&mut tx).await?;
    let n = nodes::authenticate(&mut tx, &h).await?;
    let row=sqlx::query("SELECT * FROM cloud_summary_calls WHERE id=$1 AND ai_id=$2 AND binding_version=$3 AND confirmation_id=$4").bind(id).bind(n.ai).bind(n.version).bind(parse(&v.confirmation_id)?).fetch_optional(&mut *tx).await?.ok_or_else(AppError::not_found)?;
    if v.status == "ready" {
        let sources: Vec<SourceRevision> = serde_json::from_value(row.get("sources")).unwrap();
        let scope: MaterialScope = serde_json::from_value(row.get("material_scope")).unwrap();
        if !summary_sources(
            &mut tx,
            n.ai,
            row.get("conversation"),
            row.get("conversation_revision"),
            &sources,
            &scope,
        )
        .await?
        {
            return Err(error("source_stale", "摘要来源已失效"));
        }
    }
    let fp = fingerprint(&v);
    if let Some(saved) = row.get::<Option<String>, _>("finish_fingerprint") {
        if saved != fp {
            return Err(AppError::conflict("摘要终态已用于其他结果"));
        }
        return Ok(Json(json!({"ok":true})));
    }
    if row.get::<String, _>("state") != "running" {
        return Err(error("summary_stale", "摘要调用已失效"));
    }
    sqlx::query("UPDATE cloud_summary_calls SET state=$2,usage=$3,error_code=$4,finish_fingerprint=$5,finished_at=now() WHERE id=$1").bind(id).bind(v.status).bind(json!(v.usage)).bind(v.error_code).bind(fp).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(Json(json!({"ok":true})))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn profile_capability_is_scoped_to_current_connection_and_binding() {
        let hub = RelayHub::default();
        let ai = Uuid::new_v4();
        let connection = Uuid::new_v4();
        let (sender, _outgoing) = mpsc::channel(2);
        hub.register(ai, 1, connection, sender);
        assert!(hub.supports_profile(ai, 1, connection).is_err());
        hub.set_capabilities(ai, connection, vec!["profileManagement".into()]);
        assert!(hub.supports_profile(ai, 1, connection).is_ok());
        assert!(hub.supports_profile(ai, 2, connection).is_err());
        assert!(hub.supports_profile(ai, 1, Uuid::new_v4()).is_err());
        hub.disconnect(ai, connection);
        assert!(hub.supports_profile(ai, 1, connection).is_err());
    }
    #[tokio::test]
    async fn cancelled_http_owner_releases_pending_without_node_response() {
        let hub = Arc::new(RelayHub::default());
        let ai = Uuid::new_v4();
        let connection = Uuid::new_v4();
        let relay = Uuid::new_v4();
        let (sender, _outgoing) = mpsc::channel(2);
        hub.register(ai, 1, connection, sender);
        let receiver = hub
            .dispatch(
                ai,
                1,
                connection,
                relay,
                connector::node::ServerFrame::TasksChanged,
            )
            .unwrap();
        let (started, ready) = oneshot::channel();
        let owned = hub.clone();
        let owner = tokio::spawn(async move {
            let _lease = RelayLease {
                hub: &owned,
                id: relay,
            };
            started.send(()).unwrap();
            let _ = receiver.await;
        });
        ready.await.unwrap();
        assert_eq!(hub.inner.lock().unwrap().pending.len(), 1);
        owner.abort();
        assert!(owner.await.unwrap_err().is_cancelled());
        assert!(hub.inner.lock().unwrap().pending.is_empty());
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProfileManagement {
    request_id: Uuid,
    operation: connector::node::AgentManagementOperation,
}
pub(crate) async fn manage_profile(
    State(s): State<Arc<AppState>>,
    h: HeaderMap,
    Path(ai): Path<Uuid>,
    Json(v): Json<ProfileManagement>,
) -> Result<axum::response::Response> {
    use axum::response::IntoResponse;
    if s.require_encryption {
        return Err(crate::crypto::required());
    }
    let p = auth::authenticate(&s, &h).await?;
    if serde_json::to_vec(&v.operation).unwrap().len() > 64 * 1024 {
        return Err(AppError::bad("管理请求过大"));
    }
    let mut tx = s.pool.begin().await?;
    nodes::lock(&mut tx).await?;
    nodes::admin(&mut tx, ai, p.id).await?;
    let protected:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM cloud_conversations c JOIN cloud_members m ON m.conversation=c.id WHERE m.account_id=$1 AND c.crypto_enabled) OR EXISTS(SELECT 1 FROM cloud_crypto_resources WHERE ai_id=$1 AND kind='profile')").bind(ai).fetch_one(&mut *tx).await?;
    if protected {
        return Err(crate::crypto::required());
    }
    let row = sqlx::query("SELECT version,connection_id FROM cloud_ai_bindings WHERE ai_id=$1")
        .bind(ai)
        .fetch_one(&mut *tx)
        .await?;
    let version: i64 = row.get("version");
    let connection: Uuid = row
        .get::<Option<Uuid>, _>("connection_id")
        .ok_or_else(|| error("node_offline", "节点不在线，未排队修改"))?;
    s.relay.supports_profile(ai, version, connection)?;
    let relay = Uuid::new_v4();
    let frame=serde_json::from_value(json!({"type":"managementRequest","relayId":relay,"requestId":v.request_id,"operatorId":p.id,"operation":v.operation})).map_err(|_|AppError::internal())?;
    let receiver = s.relay.dispatch(ai, version, connection, relay, frame)?;
    let _lease = RelayLease {
        hub: &s.relay,
        id: relay,
    };
    tx.commit().await?;
    let response = match tokio::time::timeout(Duration::from_secs(30), receiver).await {
        Ok(Ok(v)) => v,
        Ok(Err(_)) => {
            return Err(error(
                "node_disconnected",
                "节点连接中断，保存结果尚未确认，请保留原命令重试",
            ));
        }
        Err(_) => {
            return Err(error(
                "node_timeout",
                "节点响应超时，保存结果尚未确认，请保留原命令重试",
            ));
        }
    };
    let current = auth::authenticate(&s, &h).await?;
    let mut tx = s.pool.begin().await?;
    nodes::lock(&mut tx).await?;
    nodes::admin(&mut tx, ai, current.id).await?;
    let same:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM cloud_ai_bindings WHERE ai_id=$1 AND version=$2 AND connection_id=$3)").bind(ai).bind(version).bind(connection).fetch_one(&mut *tx).await?;
    if !same {
        return Err(error("node_replaced", "节点连接已变化，未确认此次结果"));
    }
    s.relay.supports_profile(ai, version, connection)?;
    if response["requestId"] != json!(v.request_id) {
        return Err(AppError::bad("节点管理响应标识无效"));
    }
    if let Some(e) = response.get("error").filter(|v| !v.is_null()) {
        let e: connector::node::ConsciousnessError =
            serde_json::from_value(e.clone()).map_err(|_| AppError::bad("节点错误响应无效"))?;
        if e.code.len() > 128 || e.message.len() > 4096 {
            return Err(AppError::bad("节点错误响应过长"));
        }
        return Ok((
            StatusCode::CONFLICT,
            Json(json!({"code":e.code,"message":e.message})),
        )
            .into_response());
    }
    let view: connector::node::AgentManagementView =
        serde_json::from_value(response["view"].clone())
            .map_err(|_| AppError::bad("节点未返回有效管理结果"))?;
    Ok(Json(view).into_response())
}
