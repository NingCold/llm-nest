//! Explicit conversion of existing authorized history; content identity is unchanged.
use crate::{AppError, AppState, Result, auth, chat, crypto, db as sqlx, nodes};
use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::HeaderMap,
    routing::{get, post, put},
};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::Row;
use std::{path::PathBuf, sync::Arc};
use uuid::Uuid;
type Tx<'a> = sqlx::Transaction<'a, sqlx::Postgres>;
fn digest(v: &Value) -> String {
    format!("{:x}", Sha256::digest(v.to_string().as_bytes()))
}
fn field<'a>(v: &'a Value, k: &str) -> Result<&'a str> {
    crypto::field(v, k)
}
fn uid(v: &Value, k: &str) -> Result<Uuid> {
    crypto::uid(field(v, k)?)
}
fn hash(v: &Value, k: &str) -> Result<String> {
    let s = field(v, k)?;
    if s.len() != 64 || !s.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(AppError::bad("确认摘要须为SHA256"));
    }
    Ok(s.to_owned())
}
pub(crate) fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route(
            "/crypto/history-migration-receipts/{request}",
            get(read_receipt),
        )
        .route("/crypto/history-migrations/preview", post(preview))
        .route("/crypto/history-migrations", post(begin).get(discover))
        .route("/crypto/history-migrations/{id}", get(status))
        .route("/crypto/history-migrations/{id}/items", get(items))
        .route(
            "/crypto/history-migrations/{id}/items/{kind}/{item}",
            put(convert),
        )
        .route("/crypto/history-migrations/{id}/finish", post(finish))
}
async fn read_receipt(
    State(s): State<Arc<AppState>>,
    h: HeaderMap,
    Path(request): Path<Uuid>,
) -> Result<Json<Value>> {
    let p = auth::authenticate(&s, &h).await?;
    let mut tx = s.pool.begin().await?;
    nodes::lock(&mut tx).await?;
    let row = sqlx::query("SELECT r.fingerprint,h.* FROM cloud_history_receipts r JOIN cloud_history_migrations h ON h.id=r.migration_id WHERE r.actor_id=$1 AND r.request_id=$2")
        .bind(p.id).bind(request).fetch_optional(&mut *tx).await?;
    let Some(row) = row else {
        // Absence is not evidence that an earlier unknown submission never committed.
        return Ok(Json(json!({"receipt":null})));
    };
    if let Err(error) = authority(&mut tx, row.get("conversation"), p.id).await {
        if matches!(
            error.status,
            axum::http::StatusCode::FORBIDDEN | axum::http::StatusCode::NOT_FOUND
        ) {
            return Err(AppError::forbidden());
        }
        return Err(error);
    }
    let migration = status_value(&mut tx, &row).await?;
    Ok(Json(
        json!({"receipt":{"fingerprint":row.get::<String,_>("fingerprint"),"migration":migration}}),
    ))
}
async fn authority(tx: &mut Tx<'_>, cid: Uuid, actor: Uuid) -> Result<i64> {
    chat::member(tx, cid, actor).await?;
    let r=sqlx::query("SELECT c.*,a.kind account_kind,m.role FROM cloud_conversations c JOIN cloud_members m ON m.conversation=c.id JOIN accounts a ON a.id=m.account_id WHERE c.id=$1 AND a.id=$2").bind(cid).bind(actor).fetch_one(&mut **tx).await?;
    if r.get::<String, _>("account_kind") != "human"
        || r.get::<String, _>("kind") == "group"
            && !matches!(r.get::<String, _>("role").as_str(), "owner" | "admin")
    {
        return Err(AppError::forbidden());
    }
    Ok(r.get("revision"))
}
async fn actor(s: &AppState, h: &HeaderMap, tx: &mut Tx<'_>, v: &Value) -> Result<crypto::Actor> {
    let p = auth::authenticate(s, h).await?;
    let a = crypto::Actor {
        id: p.id,
        node: None,
    };
    crypto::device(tx, &a, field(v, "deviceId")?).await?;
    Ok(a)
}
async fn migration(tx: &mut Tx<'_>, id: Uuid, actor: Uuid) -> Result<sqlx::postgres::PgRow> {
    let r = sqlx::query("SELECT * FROM cloud_history_migrations WHERE id=$1")
        .bind(id)
        .fetch_optional(&mut **tx)
        .await?
        .ok_or_else(AppError::not_found)?;
    let rev = authority(tx, r.get("conversation"), actor).await?;
    if rev != r.get::<i64, _>("conversation_revision") {
        return Err(AppError::conflict("成员或窗口版本改变，请重新预览"));
    }
    Ok(r)
}
pub(crate) async fn frozen(tx: &mut Tx<'_>, cid: Uuid) -> Result<()> {
    let active:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM cloud_conversations c JOIN cloud_history_migrations h ON h.id=c.history_migration WHERE c.id=$1 AND h.state='active')").bind(cid).fetch_one(&mut **tx).await?;
    if active {
        let mut e = AppError::conflict("历史转换期间窗口内容与调度已冻结");
        e.code = "history_migration_active";
        return Err(e);
    }
    Ok(())
}
pub(crate) async fn attachment_path(
    tx: &mut Tx<'_>,
    root: &std::path::Path,
    mid: Uuid,
    index: usize,
) -> Result<PathBuf> {
    let generation: Option<Uuid> =
        sqlx::query_scalar("SELECT attachment_generation FROM cloud_messages WHERE id=$1")
            .bind(mid)
            .fetch_one(&mut **tx)
            .await?;
    Ok(match generation {
        Some(g) => root.join(".sealed").join(g.to_string()),
        None => root.to_path_buf(),
    }
    .join(mid.to_string())
    .join(index.to_string()))
}
pub(crate) async fn node_capability(
    tx: &mut Tx<'_>,
    ai: Uuid,
    version: i64,
    cid: Uuid,
) -> Result<()> {
    let migrated:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM cloud_messages WHERE conversation=$1 AND history_migration IS NOT NULL) OR EXISTS(SELECT 1 FROM cloud_contact_intents WHERE conversation=$1 AND history_migration IS NOT NULL)").bind(cid).fetch_one(&mut **tx).await?;
    if migrated {
        let supported:bool=sqlx::query_scalar("SELECT capabilities ? 'historyMigrationV1' FROM cloud_ai_bindings WHERE ai_id=$1 AND version=$2").bind(ai).bind(version).fetch_one(&mut **tx).await?;
        if !supported {
            let mut e = AppError::conflict("节点需升级以读取迁移历史");
            e.code = "history_migration_unsupported";
            return Err(e);
        }
    }
    Ok(())
}
async fn source_digest(
    s: &AppState,
    tx: &mut Tx<'_>,
    kind: &str,
    record: &Value,
) -> Result<String> {
    if kind != "message" {
        return Ok(digest(record));
    }
    let id = uid(record, "id")?;
    let mut hashes = Vec::new();
    for part in record["attachments"].as_array().unwrap() {
        let index = part["index"]
            .as_u64()
            .ok_or_else(|| AppError::bad("附件索引无效"))? as usize;
        let bytes = tokio::fs::read(attachment_path(tx, &s.attachments, id, index).await?).await?;
        hashes.push(format!("{:x}", Sha256::digest(bytes)));
    }
    Ok(digest(&json!([record, hashes])))
}
struct Item {
    kind: &'static str,
    id: Uuid,
    revision: i64,
    digest: String,
    converted: bool,
    record: Value,
}
async fn snapshot(s: &AppState, tx: &mut Tx<'_>, cid: Uuid) -> Result<Vec<Item>> {
    let mut items = Vec::new();
    for row in
        sqlx::query("SELECT * FROM cloud_messages WHERE conversation=$1 ORDER BY position,id")
            .bind(cid)
            .fetch_all(&mut **tx)
            .await?
    {
        let id: Uuid = row.get("id");
        let record = chat::message(&row);
        let source = source_digest(s, tx, "message", &record).await?;
        items.push(Item {
            kind: "message",
            id,
            revision: row.get("revision"),
            digest: source,
            converted: row.get::<Option<Value>, _>("sealed").is_some()
                || row.get::<bool, _>("withdrawn"),
            record,
        });
    }
    for row in sqlx::query(
        "SELECT * FROM cloud_contact_intents WHERE conversation=$1 ORDER BY created_at,id",
    )
    .bind(cid)
    .fetch_all(&mut **tx)
    .await?
    {
        let record = crate::participation::intent(&row);
        items.push(Item {
            kind: "contactIntent",
            id: row.get("id"),
            revision: row.get("revision"),
            digest: digest(&record),
            converted: row.get::<Option<Value>, _>("sealed").is_some(),
            record,
        });
    }
    Ok(items)
}
fn manifest_digest(cid: Uuid, revision: i64, items: &[Item]) -> String {
    digest(&json!([
        cid,
        revision,
        items
            .iter()
            .map(|i| json!([i.kind, i.id, i.revision, i.digest]))
            .collect::<Vec<_>>()
    ]))
}
async fn status_value(tx: &mut Tx<'_>, r: &sqlx::postgres::PgRow) -> Result<Value> {
    let remaining: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM cloud_history_items WHERE migration_id=$1 AND NOT converted",
    )
    .bind(r.get::<Uuid, _>("id"))
    .fetch_one(&mut **tx)
    .await?;
    let current: i64 = sqlx::query_scalar("SELECT revision FROM cloud_conversations WHERE id=$1")
        .bind(r.get::<Uuid, _>("conversation"))
        .fetch_one(&mut **tx)
        .await?;
    let stored: String = r.get("state");
    let state = if stored == "active" && current != r.get::<i64, _>("conversation_revision") {
        "conflicted"
    } else {
        stored.as_str()
    };
    Ok(
        json!({"id":r.get::<Uuid,_>("id"),"conversationId":r.get::<Uuid,_>("conversation"),"conversationRevision":r.get::<i64,_>("conversation_revision").to_string(),"sourceDigest":r.get::<String,_>("source_digest"),"state":state,"currentConversationRevision":current.to_string(),"remaining":remaining,"backupDigest":r.get::<String,_>("backup_digest"),"recoveryDigest":r.get::<String,_>("recovery_digest"),"verificationDigest":r.get::<Option<String>,_>("verification_digest")}),
    )
}
async fn receipt(tx: &mut Tx<'_>, a: Uuid, v: &Value, operation: &str) -> Result<Option<Uuid>> {
    let rid = uid(v, "requestId")?;
    let fp = digest(&json!([operation, v]));
    if let Some(r) =
        sqlx::query("SELECT * FROM cloud_history_receipts WHERE actor_id=$1 AND request_id=$2")
            .bind(a)
            .bind(rid)
            .fetch_optional(&mut **tx)
            .await?
    {
        if r.get::<String, _>("fingerprint") != fp {
            return Err(AppError::conflict("请求ID已用于其他转换内容"));
        }
        return Ok(Some(r.get("migration_id")));
    }
    Ok(None)
}
async fn save_receipt(
    tx: &mut Tx<'_>,
    a: Uuid,
    v: &Value,
    operation: &str,
    id: Uuid,
) -> Result<()> {
    sqlx::query("INSERT INTO cloud_history_receipts(actor_id,request_id,fingerprint,migration_id) VALUES($1,$2,$3,$4)").bind(a).bind(uid(v,"requestId")?).bind(digest(&json!([operation,v]))).bind(id).execute(&mut **tx).await?;
    Ok(())
}
async fn blockers(tx: &mut Tx<'_>, cid: Uuid) -> Result<Value> {
    let row=sqlx::query("SELECT (SELECT count(*) FROM cloud_node_tasks WHERE conversation=$1 AND state IN ('queued','running','uncertain')) tasks,(SELECT count(*) FROM cloud_summary_calls WHERE conversation=$1 AND state='running') summaries,(SELECT count(*) FROM cloud_messages WHERE conversation=$1 AND status='generating') drafts,(SELECT count(*) FROM cloud_contact_intents WHERE conversation=$1 AND status IN ('pending','claimed','executing','uncertain')) contacts").bind(cid).fetch_one(&mut **tx).await?;
    Ok(
        json!({"nodeTasks":row.get::<i64,_>("tasks"),"summaryCalls":row.get::<i64,_>("summaries"),"generatingMessages":row.get::<i64,_>("drafts"),"contactIntents":row.get::<i64,_>("contacts")}),
    )
}
fn has_blockers(value: &Value) -> bool {
    value
        .as_object()
        .unwrap()
        .values()
        .any(|v| v.as_i64().unwrap() > 0)
}
async fn active_current(tx: &mut Tx<'_>, cid: Uuid) -> Result<Option<Uuid>> {
    sqlx::query_scalar("SELECT h.id FROM cloud_conversations c JOIN cloud_history_migrations h ON h.id=c.history_migration WHERE c.id=$1 AND h.state='active' AND h.conversation_revision=c.revision").bind(cid).fetch_optional(&mut **tx).await.map_err(Into::into)
}
async fn preview(
    State(s): State<Arc<AppState>>,
    h: HeaderMap,
    Json(v): Json<Value>,
) -> Result<Json<Value>> {
    let p = auth::authenticate(&s, &h).await?;
    let cid = uid(&v, "conversationId")?;
    let mut tx = s.pool.begin().await?;
    nodes::lock(&mut tx).await?;
    let revision = authority(&mut tx, cid, p.id).await?;
    let list = snapshot(&s, &mut tx, cid).await?;
    let blockers = blockers(&mut tx, cid).await?;
    let active = active_current(&mut tx, cid).await?;
    let can_begin = !has_blockers(&blockers) && active.is_none();
    Ok(Json(
        json!({"conversationId":cid,"conversationRevision":revision.to_string(),"sourceDigest":manifest_digest(cid,revision,&list),"messageCount":list.iter().filter(|i|i.kind=="message").count(),"contactIntentCount":list.iter().filter(|i|i.kind=="contactIntent").count(),"attachmentCount":list.iter().map(|i|i.record["attachments"].as_array().map_or(0,Vec::len)).sum::<usize>(),"backupRequired":true,"blockers":blockers,"canBegin":can_begin,"activeMigrationId":active}),
    ))
}
async fn begin(
    State(s): State<Arc<AppState>>,
    h: HeaderMap,
    Json(v): Json<Value>,
) -> Result<Json<Value>> {
    let mut tx = s.pool.begin().await?;
    nodes::lock(&mut tx).await?;
    let a = actor(&s, &h, &mut tx, &v).await?;
    let cid = uid(&v, "conversationId")?;
    let rev = authority(&mut tx, cid, a.id).await?;
    if let Some(id) = receipt(&mut tx, a.id, &v, "begin").await? {
        let r = migration(&mut tx, id, a.id).await?;
        return Ok(Json(status_value(&mut tx, &r).await?));
    }
    if field(&v, "expectedConversationRevision")? != rev.to_string()
        || v["localPendingResolved"] != true
    {
        return Err(AppError::conflict("请先解决待同步记录并重新预览"));
    }
    if active_current(&mut tx, cid).await?.is_some() {
        let mut e = AppError::conflict("已有进行中的转换，请先读取状态并恢复原意图");
        e.code = "history_migration_active";
        return Err(e);
    }
    if has_blockers(&blockers(&mut tx, cid).await?) {
        return Err(AppError::conflict("未完成任务、摘要、片段或约定须先解决"));
    }
    let list = snapshot(&s, &mut tx, cid).await?;
    let source = manifest_digest(cid, rev, &list);
    if field(&v, "sourceDigest")? != source {
        return Err(AppError::conflict("预览内容已改变"));
    }
    let backup = hash(&v, "backupDigest")?;
    let recovery = hash(&v, "recoveryDigest")?;
    let id = Uuid::new_v4();
    sqlx::query("UPDATE cloud_history_migrations SET state='conflicted' WHERE conversation=$1 AND state='active'").bind(cid).execute(&mut *tx).await?;
    sqlx::query("INSERT INTO cloud_history_migrations(id,conversation,actor_id,conversation_revision,source_digest,backup_digest,recovery_digest) VALUES($1,$2,$3,$4,$5,$6,$7)").bind(id).bind(cid).bind(a.id).bind(rev).bind(source).bind(backup).bind(recovery).execute(&mut *tx).await?;
    for item in list {
        sqlx::query("INSERT INTO cloud_history_items(migration_id,kind,item_id,source_revision,source_digest,original_audience_revision,converted) VALUES($1,$2,$3,$4,$5,$6,$7)").bind(id).bind(item.kind).bind(item.id).bind(item.revision).bind(item.digest).bind(item.record["audienceRevision"].as_str()).bind(item.converted).execute(&mut *tx).await?;
    }
    sqlx::query("UPDATE cloud_conversations SET history_migration=$2 WHERE id=$1")
        .bind(cid)
        .bind(id)
        .execute(&mut *tx)
        .await?;
    save_receipt(&mut tx, a.id, &v, "begin", id).await?;
    let r = migration(&mut tx, id, a.id).await?;
    let result = status_value(&mut tx, &r).await?;
    tx.commit().await?;
    Ok(Json(result))
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Lookup {
    conversation_id: Uuid,
}
async fn discover(
    State(s): State<Arc<AppState>>,
    h: HeaderMap,
    Query(q): Query<Lookup>,
) -> Result<Json<Value>> {
    let p = auth::authenticate(&s, &h).await?;
    let mut tx = s.pool.begin().await?;
    authority(&mut tx, q.conversation_id, p.id).await?;
    let row=sqlx::query("SELECT h.* FROM cloud_history_migrations h WHERE h.conversation=$1 ORDER BY h.created_at DESC,h.id DESC LIMIT 1").bind(q.conversation_id).fetch_optional(&mut *tx).await?;
    let migration = match row {
        Some(r) => status_value(&mut tx, &r).await?,
        None => Value::Null,
    };
    Ok(Json(json!({"migration":migration})))
}
async fn status(
    State(s): State<Arc<AppState>>,
    h: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>> {
    let p = auth::authenticate(&s, &h).await?;
    let mut tx = s.pool.begin().await?;
    let r = sqlx::query("SELECT * FROM cloud_history_migrations WHERE id=$1")
        .bind(id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(AppError::not_found)?;
    authority(&mut tx, r.get("conversation"), p.id).await?;
    Ok(Json(status_value(&mut tx, &r).await?))
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Page {
    after: Option<i64>,
    limit: Option<i64>,
    #[serde(default)]
    include_converted: bool,
}
async fn current(tx: &mut Tx<'_>, kind: &str, id: Uuid, cid: Uuid) -> Result<Value> {
    match kind {
        "message" => {
            let row = sqlx::query("SELECT * FROM cloud_messages WHERE id=$1 AND conversation=$2")
                .bind(id)
                .bind(cid)
                .fetch_one(&mut **tx)
                .await?;
            Ok(chat::message(&row))
        }
        "contactIntent" => {
            let row =
                sqlx::query("SELECT * FROM cloud_contact_intents WHERE id=$1 AND conversation=$2")
                    .bind(id)
                    .bind(cid)
                    .fetch_one(&mut **tx)
                    .await?;
            Ok(crate::participation::intent(&row))
        }
        _ => Err(AppError::bad("转换类型无效")),
    }
}
async fn items(
    State(s): State<Arc<AppState>>,
    h: HeaderMap,
    Path(id): Path<Uuid>,
    Query(page): Query<Page>,
) -> Result<Json<Value>> {
    let p = auth::authenticate(&s, &h).await?;
    let mut tx = s.pool.begin().await?;
    let r = migration(&mut tx, id, p.id).await?;
    let limit = page.limit.unwrap_or(32).clamp(1, 100);
    let mut rows=sqlx::query("SELECT * FROM cloud_history_items WHERE migration_id=$1 AND ordinal>$2 AND ($3 OR NOT converted) ORDER BY ordinal LIMIT $4").bind(id).bind(page.after.unwrap_or(0)).bind(page.include_converted).bind(limit+1).fetch_all(&mut *tx).await?;
    let more = rows.len() > limit as usize;
    rows.truncate(limit as usize);
    let next = rows
        .last()
        .map_or(page.after.unwrap_or(0), |r| r.get::<i64, _>("ordinal"));
    let mut out = Vec::new();
    for row in rows {
        let kind: String = row.get("kind");
        let item: Uuid = row.get("item_id");
        out.push(json!({"kind":kind,"id":item,"revision":row.get::<i64,_>("source_revision").to_string(),"sourceDigest":row.get::<String,_>("source_digest"),"converted":row.get::<bool,_>("converted"),"originalAudienceRevision":row.get::<Option<String>,_>("original_audience_revision"),"record":current(&mut tx,&kind,item,r.get("conversation")).await?}));
    }
    Ok(Json(
        json!({"items":out,"next":next.to_string(),"hasMore":more}),
    ))
}
async fn convert(
    State(s): State<Arc<AppState>>,
    h: HeaderMap,
    Path((id, kind, item)): Path<(Uuid, String, Uuid)>,
    Json(v): Json<Value>,
) -> Result<Json<Value>> {
    let mut tx = s.pool.begin().await?;
    nodes::lock(&mut tx).await?;
    let a = actor(&s, &h, &mut tx, &v).await?;
    let r = migration(&mut tx, id, a.id).await?;
    let cid: Uuid = r.get("conversation");
    let operation = format!("{id}:{kind}:{item}");
    let entry = sqlx::query(
        "SELECT * FROM cloud_history_items WHERE migration_id=$1 AND kind=$2 AND item_id=$3",
    )
    .bind(id)
    .bind(&kind)
    .bind(item)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or_else(AppError::not_found)?;
    let output = |record: Value| json!({"kind":kind,"id":item,"revision":entry.get::<i64,_>("source_revision").to_string(),"sourceDigest":entry.get::<String,_>("source_digest"),"record":record});
    if receipt(&mut tx, a.id, &v, &operation).await?.is_some() {
        return Ok(Json(output(current(&mut tx, &kind, item, cid).await?)));
    }
    if r.get::<String, _>("state") != "active" || entry.get::<bool, _>("converted") {
        return Err(AppError::conflict("项目已转换或转换已关闭"));
    }
    let revision = entry.get::<i64, _>("source_revision");
    let source: String = entry.get("source_digest");
    if field(&v, "expectedRevision")? != revision.to_string()
        || field(&v, "sourceDigest")? != source
    {
        return Err(AppError::conflict("来源修订或摘要改变"));
    }
    let record = current(&mut tx, &kind, item, cid).await?;
    if record["revision"].as_str() != Some(field(&v, "expectedRevision")?)
        || !record["sealed"].is_null()
    {
        return Err(AppError::conflict("历史项目已经改变"));
    }
    if source_digest(&s, &mut tx, &kind, &record).await? != source {
        return Err(AppError::conflict("来源正文或附件摘要已改变"));
    }
    let sealed: connector::crypto::SealedEnvelope =
        serde_json::from_value(v["sealed"].clone()).map_err(|_| AppError::bad("密文无效"))?;
    if sealed.device_id != field(&v, "deviceId")? {
        return Err(AppError::bad("迁移设备不匹配"));
    }
    crypto::envelope(
        &mut tx,
        &a,
        &format!("!{cid}:llmn"),
        &format!(
            "${item}.{}.migration.{id}",
            if kind == "contactIntent" { 1 } else { revision }
        ),
        &sealed,
    )
    .await?;
    let proof = json!({"id":id,"actorId":a.id,"sourceDigest":source});
    let audience: i64 = r.get("conversation_revision");
    if kind == "message" {
        let uploads: Vec<connector::crypto::EncryptedAttachment> = serde_json::from_value(
            v.get("encryptedAttachments")
                .cloned()
                .unwrap_or_else(|| json!([])),
        )
        .map_err(|_| AppError::bad("附件密文无效"))?;
        if uploads.len() != record["attachments"].as_array().unwrap().len() {
            return Err(AppError::bad("附件清单不完整"));
        }
        let generation_root = s.attachments.join(".sealed").join(id.to_string());
        let parts = chat::save_encrypted_uploads(&generation_root, item, &uploads).await?;
        let old_generation: Option<Uuid> =
            sqlx::query_scalar("SELECT attachment_generation FROM cloud_messages WHERE id=$1")
                .bind(item)
                .fetch_one(&mut *tx)
                .await?;
        if !uploads.is_empty() {
            sqlx::query("INSERT INTO cloud_history_attachment_backups VALUES($1,$2,$3,$4)")
                .bind(id)
                .bind(item)
                .bind(old_generation)
                .bind(&record["attachments"])
                .execute(&mut *tx)
                .await?;
        }
        sqlx::query("UPDATE cloud_messages SET body='',sealed=$2,history_migration=$3,attachments=$4,attachment_generation=$5,crypto_audience_revision=$6 WHERE id=$1").bind(item).bind(json!(sealed)).bind(proof).bind(parts).bind(id).bind(audience).execute(&mut *tx).await?;
    } else if kind == "contactIntent" {
        sqlx::query("UPDATE cloud_contact_intents SET content='',sealed=$2,history_migration=$3,crypto_audience_revision=$4 WHERE id=$1").bind(item).bind(json!(sealed)).bind(proof).bind(audience).execute(&mut *tx).await?;
    } else {
        return Err(AppError::bad("转换类型无效"));
    }
    sqlx::query("UPDATE cloud_history_items SET converted=true WHERE migration_id=$1 AND kind=$2 AND item_id=$3").bind(id).bind(&kind).bind(item).execute(&mut *tx).await?;
    save_receipt(&mut tx, a.id, &v, &operation, id).await?;
    let result = output(current(&mut tx, &kind, item, cid).await?);
    let members = chat::member(&mut tx, cid, a.id).await?;
    chat::notify(&mut tx, cid, members).await?;
    tx.commit().await?;
    Ok(Json(result))
}
async fn finish(
    State(s): State<Arc<AppState>>,
    h: HeaderMap,
    Path(id): Path<Uuid>,
    Json(v): Json<Value>,
) -> Result<Json<Value>> {
    let mut tx = s.pool.begin().await?;
    nodes::lock(&mut tx).await?;
    let a = actor(&s, &h, &mut tx, &v).await?;
    let r = migration(&mut tx, id, a.id).await?;
    let operation = format!("finish:{id}");
    if receipt(&mut tx, a.id, &v, &operation).await?.is_some() {
        return Ok(Json(status_value(&mut tx, &r).await?));
    }
    if r.get::<String, _>("state") != "active" {
        return Err(AppError::conflict("转换已关闭"));
    }
    let cid: Uuid = r.get("conversation");
    let verification = hash(&v, "verificationDigest")?;
    let left:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM cloud_history_items WHERE migration_id=$1 AND NOT converted) OR EXISTS(SELECT 1 FROM cloud_messages WHERE conversation=$2 AND NOT withdrawn AND (sealed IS NULL OR body<>'')) OR EXISTS(SELECT 1 FROM cloud_contact_intents WHERE conversation=$2 AND (sealed IS NULL OR content<>''))").bind(id).bind(cid).fetch_one(&mut *tx).await?;
    if left {
        return Err(AppError::conflict("转换清单尚未完成"));
    }
    sqlx::query("UPDATE cloud_history_migrations SET state='complete',verification_digest=$2,completed_at=now() WHERE id=$1").bind(id).bind(verification).execute(&mut *tx).await?;
    sqlx::query("UPDATE cloud_conversations SET crypto_enabled=true,crypto_history_complete=true,crypto_from_position=0 WHERE id=$1").bind(cid).execute(&mut *tx).await?;
    save_receipt(&mut tx, a.id, &v, &operation, id).await?;
    let members = chat::member(&mut tx, cid, a.id).await?;
    chat::notify(&mut tx, cid, members).await?;
    let done = migration(&mut tx, id, a.id).await?;
    let result = status_value(&mut tx, &done).await?;
    tx.commit().await?;
    Ok(Json(result))
}
