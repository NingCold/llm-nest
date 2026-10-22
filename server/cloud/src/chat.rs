use crate::db as sqlx;
use crate::{AppError, AppState, Result, auth};
use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::{HeaderMap, header},
    response::{
        IntoResponse, Response,
        sse::{Event, KeepAlive, Sse},
    },
    routing::{get, patch, post, put},
};
use base64::Engine;
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::{Postgres, Row, Transaction};
use std::{convert::Infallible, io::Write, sync::Arc, time::Duration};
use tokio::io::AsyncWriteExt;
use uuid::Uuid;
type Tx<'a> = Transaction<'a, Postgres>;
fn bad(s: &str) -> AppError {
    AppError::bad(s)
}
pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/chat/init", get(init))
        .route("/chat/accounts", get(accounts))
        .route("/chat/contacts", post(contacts))
        .route("/chat/direct", post(direct))
        .route(
            "/chat/conversations/{id}/messages",
            get(messages).post(send),
        )
        .route("/chat/messages/{id}", patch(edit))
        .route("/chat/messages/{id}/withdraw", post(withdraw))
        .route("/chat/conversations/{id}/list", put(list))
        .route("/chat/search", get(search))
        .route("/chat/messages/{id}/context", get(context))
        .route("/chat/messages/{id}/attachments/{index}", get(download))
        .route("/chat/conversations/{id}/export", get(export))
        .route("/chat/events", get(events))
}
async fn lock(tx: &mut Tx<'_>) -> Result<()> {
    sqlx::query("SELECT pg_advisory_xact_lock(731921)")
        .execute(&mut **tx)
        .await?;
    Ok(())
}
async fn member(tx: &mut Tx<'_>, id: Uuid, owner: Uuid) -> Result<(Uuid, Uuid)> {
    let r=sqlx::query("SELECT first_id,second_id FROM cloud_conversations WHERE id=$1 AND (first_id=$2 OR second_id=$2)").bind(id).bind(owner).fetch_optional(&mut **tx).await?.ok_or_else(AppError::not_found)?;
    Ok((r.get(0), r.get(1)))
}
async fn notify(tx: &mut Tx<'_>, id: Uuid, pair: (Uuid, Uuid)) -> Result<()> {
    for owner in [pair.0, pair.1] {
        sqlx::query("INSERT INTO cloud_events(owner,conversation) VALUES($1,$2)")
            .bind(owner)
            .bind(id)
            .execute(&mut **tx)
            .await?;
    }
    Ok(())
}
async fn account(tx: &mut Tx<'_>, id: Uuid) -> Result<Value> {
    let r = sqlx::query("SELECT id,name,bio,kind FROM accounts WHERE id=$1")
        .bind(id)
        .fetch_optional(&mut **tx)
        .await?
        .ok_or_else(AppError::not_found)?;
    Ok(
        json!({"id":r.get::<Uuid,_>(0),"name":r.get::<String,_>(1),"bio":r.get::<String,_>(2),"kind":r.get::<String,_>(3)}),
    )
}
fn message(r: &sqlx::postgres::PgRow) -> Value {
    json!({"id":r.get::<Uuid,_>("id"),"conversationId":r.get::<Uuid,_>("conversation"),"senderId":r.get::<Uuid,_>("sender"),"text":r.get::<String,_>("body"),"createdAt":r.get::<chrono::DateTime<chrono::Utc>,_>("created_at"),"updatedAt":r.get::<chrono::DateTime<chrono::Utc>,_>("updated_at"),"revision":r.get::<i64,_>("revision").to_string(),"position":r.get::<i64,_>("position").to_string(),"status":if r.get::<bool,_>("withdrawn"){String::from("withdrawn")}else{r.get::<String,_>("status")},"attachments":r.get::<Value,_>("attachments"),"aiResponse":r.get::<Option<Value>,_>("ai_response")})
}
async fn conversation(tx: &mut Tx<'_>, id: Uuid, owner: Uuid) -> Result<Value> {
    let pair = member(tx, id, owner).await?;
    let peer = account(tx, if pair.0 == owner { pair.1 } else { pair.0 }).await?;
    let r = sqlx::query("SELECT l.*, (SELECT coalesce(max(position),0) FROM cloud_messages WHERE conversation=$2 AND status<>'generating') AS latest_position FROM cloud_list l WHERE owner=$1 AND conversation=$2")
        .bind(owner)
        .bind(id)
        .fetch_one(&mut **tx)
        .await?;
    let pos: i64 = r.get("read_position");
    let last = sqlx::query(
        "SELECT * FROM cloud_messages WHERE conversation=$1 ORDER BY position DESC LIMIT 1",
    )
    .bind(id)
    .fetch_optional(&mut **tx)
    .await?;
    let unread:i64=sqlx::query_scalar("SELECT count(*) FROM cloud_messages WHERE conversation=$1 AND sender<>$2 AND position>$3 AND NOT withdrawn AND status<>'generating'").bind(id).bind(owner).bind(pos).fetch_one(&mut **tx).await?;
    Ok(
        json!({"id":id,"peer":peer,"revision":"1","lastMessage":last.as_ref().map(message),"latestPosition":r.get::<i64,_>("latest_position").to_string(),"unreadCount":unread,"manualUnread":r.get::<bool,_>("manual_unread"),"pinned":r.get::<bool,_>("pinned"),"readPosition":pos.to_string(),"listRevision":r.get::<i64,_>("revision").to_string()}),
    )
}
async fn init(State(s): State<Arc<AppState>>, h: HeaderMap) -> Result<Json<Value>> {
    let p = auth::authenticate(&s, &h).await?;
    let mut tx = s.pool.begin().await?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ")
        .execute(&mut *tx)
        .await?;
    let me = account(&mut tx, p.id).await?;
    let ids: Vec<Uuid> = sqlx::query_scalar("SELECT target FROM cloud_contacts WHERE owner=$1")
        .bind(p.id)
        .fetch_all(&mut *tx)
        .await?;
    let mut contacts = vec![];
    for id in ids {
        contacts.push(account(&mut tx, id).await?)
    }
    let ids: Vec<Uuid> =
        sqlx::query_scalar("SELECT id FROM cloud_conversations WHERE first_id=$1 OR second_id=$1")
            .bind(p.id)
            .fetch_all(&mut *tx)
            .await?;
    let mut cs = vec![];
    for id in ids {
        cs.push(conversation(&mut tx, id, p.id).await?)
    }
    let cursor: i64 =
        sqlx::query_scalar("SELECT coalesce(max(id),0) FROM cloud_events WHERE owner=$1")
            .bind(p.id)
            .fetch_one(&mut *tx)
            .await?;
    tx.commit().await?;
    Ok(Json(
        json!({"self":me,"contacts":contacts,"conversations":cs,"cursor":cursor.to_string()}),
    ))
}
#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct Q {
    query: Option<String>,
    before: Option<i64>,
    limit: Option<i64>,
    conversation_id: Option<Uuid>,
    before_position: Option<i64>,
    revision: Option<i64>,
    inline: Option<bool>,
    format: Option<String>,
    include_images: Option<bool>,
    include_files: Option<bool>,
    after: Option<i64>,
}
async fn accounts(
    State(s): State<Arc<AppState>>,
    h: HeaderMap,
    Query(q): Query<Q>,
) -> Result<Json<Value>> {
    auth::authenticate(&s, &h).await?;
    let rows=sqlx::query("SELECT id,name,bio,kind FROM accounts WHERE strpos(lower(name),lower($1))>0 OR id::text=$1 ORDER BY name,id LIMIT 50").bind(q.query.unwrap_or_default()).fetch_all(&s.pool).await?;
    Ok(Json(Value::Array(rows.iter().map(|r|json!({"id":r.get::<Uuid,_>(0),"name":r.get::<String,_>(1),"bio":r.get::<String,_>(2),"kind":r.get::<String,_>(3)})).collect())))
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Target {
    account_id: Uuid,
    listed: Option<bool>,
}
async fn contacts(
    State(s): State<Arc<AppState>>,
    h: HeaderMap,
    Json(v): Json<Target>,
) -> Result<Json<Value>> {
    let p = auth::authenticate(&s, &h).await?;
    if p.id == v.account_id {
        return Err(bad("不能添加自己"));
    }
    let mut tx = s.pool.begin().await?;
    lock(&mut tx).await?;
    account(&mut tx, v.account_id).await?;
    if v.listed.unwrap_or(true) {
        sqlx::query("INSERT INTO cloud_contacts VALUES($1,$2) ON CONFLICT DO NOTHING")
            .bind(p.id)
            .bind(v.account_id)
            .execute(&mut *tx)
            .await?;
    } else {
        sqlx::query("DELETE FROM cloud_contacts WHERE owner=$1 AND target=$2")
            .bind(p.id)
            .bind(v.account_id)
            .execute(&mut *tx)
            .await?;
    }
    sqlx::query("INSERT INTO cloud_events(owner) VALUES($1)")
        .bind(p.id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(Json(json!({"ok":true})))
}
async fn direct(
    State(s): State<Arc<AppState>>,
    h: HeaderMap,
    Json(v): Json<Target>,
) -> Result<Json<Value>> {
    let p = auth::authenticate(&s, &h).await?;
    if p.id == v.account_id {
        return Err(bad("不能与自己创建私聊"));
    }
    let mut tx = s.pool.begin().await?;
    lock(&mut tx).await?;
    account(&mut tx, v.account_id).await?;
    let (a, b) = if p.id < v.account_id {
        (p.id, v.account_id)
    } else {
        (v.account_id, p.id)
    };
    let id:Uuid=sqlx::query_scalar("INSERT INTO cloud_conversations(id,first_id,second_id) VALUES($1,$2,$3) ON CONFLICT(first_id,second_id) DO UPDATE SET first_id=excluded.first_id RETURNING id").bind(Uuid::new_v4()).bind(a).bind(b).fetch_one(&mut *tx).await?;
    for owner in [a, b] {
        sqlx::query(
            "INSERT INTO cloud_list(owner,conversation) VALUES($1,$2) ON CONFLICT DO NOTHING",
        )
        .bind(owner)
        .bind(id)
        .execute(&mut *tx)
        .await?;
    }
    notify(&mut tx, id, (a, b)).await?;
    let result = conversation(&mut tx, id, p.id).await?;
    tx.commit().await?;
    Ok(Json(result))
}
async fn messages(
    State(s): State<Arc<AppState>>,
    h: HeaderMap,
    Path(id): Path<Uuid>,
    Query(q): Query<Q>,
) -> Result<Json<Value>> {
    let p = auth::authenticate(&s, &h).await?;
    let mut tx = s.pool.begin().await?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ")
        .execute(&mut *tx)
        .await?;
    member(&mut tx, id, p.id).await?;
    let limit = q.limit.unwrap_or(50).clamp(1, 100);
    let mut rows=sqlx::query("SELECT * FROM cloud_messages WHERE conversation=$1 AND position<$2 ORDER BY position DESC LIMIT $3").bind(id).bind(q.before.unwrap_or(i64::MAX)).bind(limit+1).fetch_all(&mut *tx).await?;
    let more = rows.len() > limit as usize;
    rows.truncate(limit as usize);
    rows.reverse();
    let latest: i64 = sqlx::query_scalar(
        "SELECT coalesce(max(position),0) FROM cloud_messages WHERE conversation=$1 AND status<>'generating'",
    )
    .bind(id)
    .fetch_one(&mut *tx)
    .await?;
    Ok(Json(
        json!({"messages":rows.iter().map(message).collect::<Vec<_>>(),"hasMore":more,"latestPosition":latest.to_string()}),
    ))
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Input {
    request_id: Uuid,
    text: Option<String>,
    expected_revision: Option<String>,
    #[serde(default)]
    images: Vec<Upload>,
    #[serde(default)]
    files: Vec<Upload>,
    pinned: Option<bool>,
    read_position: Option<String>,
    manual_unread: Option<bool>,
}
#[derive(Deserialize)]
struct Upload {
    name: Option<String>,
    mime: String,
    data: String,
}
async fn receipt(
    tx: &mut Tx<'_>,
    owner: Uuid,
    v: &Input,
    kind: &str,
    id: Uuid,
) -> Result<Option<Value>> {
    let fingerprint = serde_json::to_string(&json!([
        kind,
        id,
        v.text,
        v.expected_revision,
        v.pinned,
        v.read_position,
        v.manual_unread,
        v.images
            .iter()
            .map(|x| (&x.name, &x.mime, &x.data))
            .collect::<Vec<_>>(),
        v.files
            .iter()
            .map(|x| (&x.name, &x.mime, &x.data))
            .collect::<Vec<_>>()
    ]))
    .map_err(|_| AppError::internal())?;
    let fingerprint = format!("{:x}", Sha256::digest(fingerprint.as_bytes()));
    if let Some(r) =
        sqlx::query("SELECT fingerprint,response FROM cloud_receipts WHERE owner=$1 AND request=$2")
            .bind(owner)
            .bind(v.request_id)
            .fetch_optional(&mut **tx)
            .await?
    {
        if r.get::<String, _>(0) != fingerprint {
            return Err(AppError::conflict("请求 ID 已用于其他内容"));
        }
        if kind == "list" {
            return Ok(Some(conversation(tx, id, owner).await?));
        }
        let receipt: Value = r.get(1);
        let message_id: Uuid =
            serde_json::from_value(receipt["id"].clone()).map_err(|_| AppError::internal())?;
        let row = sqlx::query("SELECT * FROM cloud_messages WHERE id=$1")
            .bind(message_id)
            .fetch_one(&mut **tx)
            .await?;
        return Ok(Some(message(&row)));
    }
    Ok(None)
}
async fn save_receipt(
    tx: &mut Tx<'_>,
    owner: Uuid,
    v: &Input,
    kind: &str,
    id: Uuid,
    result: &Value,
) -> Result<()> {
    let fingerprint = serde_json::to_string(&json!([
        kind,
        id,
        v.text,
        v.expected_revision,
        v.pinned,
        v.read_position,
        v.manual_unread,
        v.images
            .iter()
            .map(|x| (&x.name, &x.mime, &x.data))
            .collect::<Vec<_>>(),
        v.files
            .iter()
            .map(|x| (&x.name, &x.mime, &x.data))
            .collect::<Vec<_>>()
    ]))
    .map_err(|_| AppError::internal())?;
    let fingerprint = format!("{:x}", Sha256::digest(fingerprint.as_bytes()));
    sqlx::query("INSERT INTO cloud_receipts VALUES($1,$2,$3,$4)")
        .bind(owner)
        .bind(v.request_id)
        .bind(fingerprint)
        .bind(json!({"id":result["id"]}))
        .execute(&mut **tx)
        .await?;
    Ok(())
}
async fn uploads(s: &AppState, id: Uuid, v: &Input) -> Result<Value> {
    if v.images.len() > 4 || v.files.len() > 4 {
        return Err(bad("附件数量超限"));
    }
    let mut out = vec![];
    let mut image_total = 0;
    let mut file_total = 0;
    for (kind, items) in [("image", &v.images), ("file", &v.files)] {
        for item in items {
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(&item.data)
                .map_err(|_| bad("无效 base64"))?;
            if kind == "image" {
                image_total += bytes.len();
                if bytes.len() > 4 * 1024 * 1024 || image_total > 8 * 1024 * 1024 {
                    return Err(bad("图片过大"));
                }
                let format = image::guess_format(&bytes).map_err(|_| bad("无效图片"))?;
                let mime = match format {
                    image::ImageFormat::Png => "image/png",
                    image::ImageFormat::Jpeg => "image/jpeg",
                    image::ImageFormat::WebP => "image/webp",
                    _ => return Err(bad("图片格式不支持")),
                };
                if item.mime != mime {
                    return Err(bad("图片类型不匹配"));
                }
                let reader = image::ImageReader::with_format(std::io::Cursor::new(&bytes), format);
                let (w, h) = reader.into_dimensions().map_err(|_| bad("无效图片"))?;
                if w > 4096 || h > 4096 {
                    return Err(bad("图片尺寸超限"));
                }
                image::load_from_memory_with_format(&bytes, format)
                    .map_err(|_| bad("图片解码失败"))?;
            } else {
                file_total += bytes.len();
                if bytes.len() > 10 * 1024 * 1024 || file_total > 20 * 1024 * 1024 {
                    return Err(bad("文件过大"));
                }
                if let Some(name) = &item.name
                    && !social::attachment::valid_file_name(name)
                {
                    return Err(bad("无效文件名"));
                }
                if item.mime.is_empty()
                    || item.mime.len() > 255
                    || item.mime.chars().any(char::is_control)
                {
                    return Err(bad("无效文件类型"));
                }
            }
            let index = out.len();
            let dir = s.attachments.join(id.to_string());
            tokio::fs::create_dir_all(&dir).await?;
            let mut file = tokio::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(dir.join(index.to_string()))
                .await?;
            file.write_all(&bytes).await?;
            file.sync_all().await?;
            out.push(json!({"index":index,"kind":kind,"name":item.name,"mime":item.mime,"byteLength":bytes.len()}));
        }
    }
    Ok(Value::Array(out))
}
async fn send(
    State(s): State<Arc<AppState>>,
    h: HeaderMap,
    Path(id): Path<Uuid>,
    Json(v): Json<Input>,
) -> Result<Json<Value>> {
    let p = auth::authenticate(&s, &h).await?;
    let mut tx = s.pool.begin().await?;
    lock(&mut tx).await?;
    let pair = member(&mut tx, id, p.id).await?;
    if let Some(r) = receipt(&mut tx, p.id, &v, "send", id).await? {
        return Ok(Json(r));
    }
    let text = v.text.as_deref().unwrap_or("");
    if text.len() > 1024 * 1024
        || text.trim().is_empty() && v.images.is_empty() && v.files.is_empty()
    {
        return Err(bad("消息为空或过长"));
    }
    let mid = Uuid::new_v4();
    let attachments = uploads(&s, mid, &v).await?;
    sqlx::query("INSERT INTO cloud_messages(id,conversation,sender,body,attachments) VALUES($1,$2,$3,$4,$5)").bind(mid).bind(id).bind(p.id).bind(text).bind(attachments).execute(&mut *tx).await?;
    crate::nodes::enqueue(&mut tx, id, mid, p.id, pair).await?;
    let row = sqlx::query("SELECT * FROM cloud_messages WHERE id=$1")
        .bind(mid)
        .fetch_one(&mut *tx)
        .await?;
    let result = message(&row);
    notify(&mut tx, id, pair).await?;
    save_receipt(&mut tx, p.id, &v, "send", id, &result).await?;
    tx.commit().await?;
    Ok(Json(result))
}
async fn mutate(
    s: Arc<AppState>,
    h: HeaderMap,
    id: Uuid,
    v: Input,
    remove: bool,
) -> Result<Json<Value>> {
    let p = auth::authenticate(&s, &h).await?;
    let mut tx = s.pool.begin().await?;
    lock(&mut tx).await?;
    let row = sqlx::query("SELECT * FROM cloud_messages WHERE id=$1")
        .bind(id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(AppError::not_found)?;
    let cid: Uuid = row.get("conversation");
    let pair = member(&mut tx, cid, p.id).await?;
    if row.get::<Uuid, _>("sender") != p.id {
        return Err(AppError::forbidden());
    }
    let kind = if remove { "withdraw" } else { "edit" };
    if let Some(r) = receipt(&mut tx, p.id, &v, kind, id).await? {
        return Ok(Json(r));
    }
    if v.expected_revision.as_deref() != Some(row.get::<i64, _>("revision").to_string().as_str())
        || row.get::<bool, _>("withdrawn")
    {
        return Err(AppError::conflict("消息版本已变化"));
    }
    let text = if remove {
        ""
    } else {
        v.text.as_deref().ok_or_else(|| bad("缺少正文"))?
    };
    if text.len() > 1024 * 1024
        || !remove
            && text.trim().is_empty()
            && row
                .get::<Value, _>("attachments")
                .as_array()
                .unwrap()
                .is_empty()
    {
        return Err(bad("消息为空或过长"));
    }
    sqlx::query("UPDATE cloud_messages SET body=$2,withdrawn=$3,attachments=CASE WHEN $3 THEN '[]'::jsonb ELSE attachments END,revision=revision+1,updated_at=now() WHERE id=$1").bind(id).bind(text).bind(remove).execute(&mut *tx).await?;
    crate::nodes::source_changed(&mut tx, id).await?;
    let r = sqlx::query("SELECT * FROM cloud_messages WHERE id=$1")
        .bind(id)
        .fetch_one(&mut *tx)
        .await?;
    let result = message(&r);
    notify(&mut tx, cid, pair).await?;
    save_receipt(&mut tx, p.id, &v, kind, id, &result).await?;
    tx.commit().await?;
    if remove {
        let path = s.attachments.join(id.to_string());
        if path.exists() {
            tokio::fs::remove_dir_all(path).await?;
        }
    }
    Ok(Json(result))
}
async fn edit(
    State(s): State<Arc<AppState>>,
    h: HeaderMap,
    Path(id): Path<Uuid>,
    Json(v): Json<Input>,
) -> Result<Json<Value>> {
    mutate(s, h, id, v, false).await
}
async fn withdraw(
    State(s): State<Arc<AppState>>,
    h: HeaderMap,
    Path(id): Path<Uuid>,
    Json(v): Json<Input>,
) -> Result<Json<Value>> {
    mutate(s, h, id, v, true).await
}
async fn list(
    State(s): State<Arc<AppState>>,
    h: HeaderMap,
    Path(id): Path<Uuid>,
    Json(v): Json<Input>,
) -> Result<Json<Value>> {
    let p = auth::authenticate(&s, &h).await?;
    let mut tx = s.pool.begin().await?;
    lock(&mut tx).await?;
    member(&mut tx, id, p.id).await?;
    if let Some(r) = receipt(&mut tx, p.id, &v, "list", id).await? {
        return Ok(Json(r));
    }
    let row = sqlx::query("SELECT * FROM cloud_list WHERE owner=$1 AND conversation=$2")
        .bind(p.id)
        .bind(id)
        .fetch_one(&mut *tx)
        .await?;
    if v.expected_revision.as_deref() != Some(row.get::<i64, _>("revision").to_string().as_str()) {
        return Err(AppError::conflict("列表版本已变化"));
    }
    let mut position = row.get::<i64, _>("read_position");
    if let Some(raw) = &v.read_position {
        let pos: i64 = raw.parse().map_err(|_| bad("无效已读位置"))?;
        if pos != 0
            && !sqlx::query_scalar::<_, bool>(
                "SELECT EXISTS(SELECT 1 FROM cloud_messages WHERE conversation=$1 AND position=$2 AND status<>'generating')",
            )
            .bind(id)
            .bind(pos)
            .fetch_one(&mut *tx)
            .await?
        {
            return Err(bad("无效已读位置"));
        }
        position = position.max(pos)
    }
    sqlx::query("UPDATE cloud_list SET revision=revision+1,pinned=$3,read_position=$4,manual_unread=$5 WHERE owner=$1 AND conversation=$2").bind(p.id).bind(id).bind(v.pinned.unwrap_or(row.get("pinned"))).bind(position).bind(v.manual_unread.unwrap_or(row.get("manual_unread"))).execute(&mut *tx).await?;
    let result = conversation(&mut tx, id, p.id).await?;
    sqlx::query("INSERT INTO cloud_events(owner,conversation) VALUES($1,$2)")
        .bind(p.id)
        .bind(id)
        .execute(&mut *tx)
        .await?;
    save_receipt(&mut tx, p.id, &v, "list", id, &result).await?;
    tx.commit().await?;
    Ok(Json(result))
}
async fn search(
    State(s): State<Arc<AppState>>,
    h: HeaderMap,
    Query(q): Query<Q>,
) -> Result<Json<Value>> {
    let p = auth::authenticate(&s, &h).await?;
    let query = q.query.unwrap_or_default();
    if query.trim().is_empty() {
        return Err(bad("搜索内容为空"));
    }
    let mut rows=sqlx::query("SELECT m.* FROM cloud_messages m JOIN cloud_conversations c ON c.id=m.conversation WHERE (c.first_id=$1 OR c.second_id=$1) AND ($2::uuid IS NULL OR c.id=$2) AND NOT m.withdrawn AND m.status<>'generating' AND strpos(translate(m.body,'ABCDEFGHIJKLMNOPQRSTUVWXYZ','abcdefghijklmnopqrstuvwxyz'),translate($3,'ABCDEFGHIJKLMNOPQRSTUVWXYZ','abcdefghijklmnopqrstuvwxyz'))>0 AND m.position<$4 ORDER BY m.position DESC LIMIT 51").bind(p.id).bind(q.conversation_id).bind(query).bind(q.before_position.unwrap_or(i64::MAX)).fetch_all(&s.pool).await?;
    let more = rows.len() > 50;
    rows.truncate(50);
    Ok(Json(
        json!({"messages":rows.iter().map(message).collect::<Vec<_>>(),"hasMore":more}),
    ))
}
async fn context(
    State(s): State<Arc<AppState>>,
    h: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>> {
    let p = auth::authenticate(&s, &h).await?;
    let mut tx = s.pool.begin().await?;
    let row = sqlx::query(
        "SELECT conversation,position FROM cloud_messages WHERE id=$1 AND NOT withdrawn AND status<>'generating'",
    )
    .bind(id)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or_else(AppError::not_found)?;
    let cid: Uuid = row.get(0);
    let pos: i64 = row.get(1);
    member(&mut tx, cid, p.id).await?;
    let rows=sqlx::query("SELECT * FROM ((SELECT * FROM cloud_messages WHERE conversation=$1 AND position<=$2 ORDER BY position DESC LIMIT 21) UNION ALL (SELECT * FROM cloud_messages WHERE conversation=$1 AND position>$2 ORDER BY position LIMIT 20)) m ORDER BY position").bind(cid).bind(pos).fetch_all(&mut *tx).await?;
    Ok(Json(
        json!({"conversationId":cid,"messages":rows.iter().map(message).collect::<Vec<_>>()}),
    ))
}
fn image_extension(mime: &str) -> &str {
    match mime {
        "image/png" => "png",
        "image/webp" => "webp",
        "image/jpeg" => "jpg",
        _ => unreachable!("validated image format"),
    }
}
fn binary(bytes: Vec<u8>, mime: &str, name: &str, inline: bool) -> Response {
    let mut r = bytes.into_response();
    r.headers_mut()
        .insert(header::CONTENT_TYPE, mime.parse().unwrap());
    r.headers_mut()
        .insert(header::CACHE_CONTROL, "no-store".parse().unwrap());
    r.headers_mut()
        .insert("x-content-type-options", "nosniff".parse().unwrap());
    let escaped = percent_encoding::utf8_percent_encode(name, percent_encoding::NON_ALPHANUMERIC);
    r.headers_mut().insert(
        header::CONTENT_DISPOSITION,
        format!(
            "{}; filename*=UTF-8''{}",
            if inline { "inline" } else { "attachment" },
            escaped
        )
        .parse()
        .unwrap(),
    );
    r
}
async fn download(
    State(s): State<Arc<AppState>>,
    h: HeaderMap,
    Path((id, index)): Path<(Uuid, usize)>,
    Query(q): Query<Q>,
) -> Result<Response> {
    let p = auth::authenticate(&s, &h).await?;
    let mut tx = s.pool.begin().await?;
    let row = sqlx::query("SELECT * FROM cloud_messages WHERE id=$1 FOR SHARE")
        .bind(id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(AppError::not_found)?;
    member(&mut tx, row.get("conversation"), p.id).await?;
    if q.revision != Some(row.get("revision")) || row.get::<bool, _>("withdrawn") {
        return Err(AppError::conflict("附件版本已变化"));
    }
    let parts: Value = row.get("attachments");
    let part = parts.get(index).ok_or_else(AppError::not_found)?;
    let inline = q.inline.unwrap_or(false);
    if inline && part["kind"] != "image" {
        return Err(bad("普通文件不可内联"));
    }
    let bytes = tokio::fs::read(s.attachments.join(id.to_string()).join(index.to_string())).await?;
    let name = if part["kind"] == "image" {
        format!(
            "{id}-{index}.{}",
            image_extension(part["mime"].as_str().unwrap())
        )
    } else {
        social::attachment::file_name(part["name"].as_str())
    };
    Ok(binary(
        bytes,
        if inline {
            part["mime"].as_str().unwrap()
        } else {
            "application/octet-stream"
        },
        &name,
        inline,
    ))
}
async fn export(
    State(s): State<Arc<AppState>>,
    h: HeaderMap,
    Path(id): Path<Uuid>,
    Query(q): Query<Q>,
) -> Result<Response> {
    let p = auth::authenticate(&s, &h).await?;
    let json_format = match q.format.as_deref().unwrap_or("markdown") {
        "markdown" => false,
        "json" => true,
        _ => return Err(bad("无效导出格式")),
    };
    let mut tx = s.pool.begin().await?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ")
        .execute(&mut *tx)
        .await?;
    member(&mut tx, id, p.id).await?;
    let rows = sqlx::query(
        "SELECT * FROM cloud_messages WHERE conversation=$1 ORDER BY position FOR SHARE",
    )
    .bind(id)
    .fetch_all(&mut *tx)
    .await?;
    let images = q.include_images.unwrap_or(false);
    let files = q.include_files.unwrap_or(false);
    let mut messages = vec![];
    let mut entries = vec![];
    let mut md = String::new();
    for row in &rows {
        let mut m = message(row);
        md.push_str(&format!(
            "{} — {}\n\n{}\n\n",
            m["createdAt"].as_str().unwrap(),
            m["senderId"].as_str().unwrap(),
            if m["status"] == "withdrawn" {
                "[消息已撤回]"
            } else {
                m["text"].as_str().unwrap()
            }
        ));
        let mid: Uuid = row.get("id");
        for part in m["attachments"].as_array_mut().unwrap() {
            let index = part["index"].as_u64().unwrap();
            let image = part["kind"] == "image";
            if image && images || !image && files {
                let name = if image {
                    format!(
                        "images/{mid}-{index}.{}",
                        match part["mime"].as_str().unwrap() {
                            "image/png" => "png",
                            "image/webp" => "webp",
                            _ => "jpg",
                        }
                    )
                } else {
                    format!(
                        "files/{mid}-{index}/{}",
                        social::attachment::file_name(part["name"].as_str())
                    )
                };
                let bytes =
                    tokio::fs::read(s.attachments.join(mid.to_string()).join(index.to_string()))
                        .await?;
                md.push_str(&format!(
                    "[附件]({})\n\n",
                    name.split('/')
                        .map(|s| percent_encoding::utf8_percent_encode(
                            s,
                            percent_encoding::NON_ALPHANUMERIC
                        )
                        .to_string())
                        .collect::<Vec<_>>()
                        .join("/")
                ));
                part["filePath"] = json!(name);
                entries.push((name, bytes));
            } else {
                md.push_str(&format!(
                    "[附件：{}，{} 字节]\n\n",
                    part["name"].as_str().unwrap_or("image"),
                    part["byteLength"]
                ));
            }
        }
        messages.push(m);
    }
    let doc = json!({"version":if files{3}else if images{2}else{1},"conversationId":id,"messages":messages});
    let json = serde_json::to_vec_pretty(&doc).map_err(|_| AppError::internal())?;
    if images || files {
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        for (name, bytes) in [
            ("chat.md".to_string(), md.into_bytes()),
            ("chat.json".to_string(), json),
        ]
        .into_iter()
        .chain(entries)
        {
            zip.start_file(name, zip::write::SimpleFileOptions::default())
                .map_err(|_| AppError::internal())?;
            zip.write_all(&bytes)?;
        }
        let bytes = zip.finish().map_err(|_| AppError::internal())?.into_inner();
        Ok(binary(bytes, "application/zip", "chat.zip", false))
    } else if json_format {
        Ok(binary(json, "application/json", "chat.json", false))
    } else {
        Ok(binary(
            md.into_bytes(),
            "text/markdown; charset=utf-8",
            "chat.md",
            false,
        ))
    }
}
async fn events(
    State(s): State<Arc<AppState>>,
    h: HeaderMap,
    Query(q): Query<Q>,
) -> Result<Sse<impl futures_util::Stream<Item = std::result::Result<Event, Infallible>>>> {
    let p = auth::authenticate(&s, &h).await?;
    let cursor = q
        .after
        .or_else(|| {
            h.get("last-event-id")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.parse().ok())
        })
        .unwrap_or(0);
    let stream = futures_util::stream::unfold(
        (s, h, p.id, cursor),
        |(s, h, owner, mut cursor)| async move {
            loop {
                if auth::authenticate(&s, &h).await.is_err() {
                    return None;
                }
                let row=match sqlx::query("SELECT id,conversation FROM cloud_events WHERE owner=$1 AND id>$2 ORDER BY id LIMIT 1").bind(owner).bind(cursor).fetch_optional(&s.pool).await{Ok(r)=>r,Err(_)=>return None};
                if let Some(r) = row {
                    cursor = r.get(0);
                    let cid: Option<Uuid> = r.get(1);
                    let data =
                        json!({"cursor":cursor.to_string(),"conversationId":cid}).to_string();
                    return Some((
                        Ok(Event::default()
                            .event("change")
                            .id(cursor.to_string())
                            .data(data)),
                        (s, h, owner, cursor),
                    ));
                }
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
        },
    );
    Ok(Sse::new(stream).keep_alive(KeepAlive::default()))
}
#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};
    pub(super) struct Fixture {
        pub(super) state: Arc<AppState>,
        admin: sqlx::PgPool,
        schema: String,
        _dir: tempfile::TempDir,
        pub(super) users: Vec<(Uuid, HeaderMap)>,
    }
    impl Fixture {
        pub(super) async fn new() -> Option<Self> {
            let url = std::env::var("CLOUD_TEST_DATABASE_URL").ok()?;
            let admin = sqlx::postgres::PgPoolOptions::new()
                .max_connections(2)
                .connect(&url)
                .await
                .unwrap();
            let schema = format!("chat_test_{}", Uuid::new_v4().simple());
            sqlx::query(&format!("CREATE SCHEMA {schema}"))
                .execute(&admin)
                .await
                .unwrap();
            let path = schema.clone();
            let pool = sqlx::postgres::PgPoolOptions::new()
                .max_connections(8)
                .after_connect(move |c, _| {
                    let sql = format!("SET search_path TO {path}");
                    Box::pin(async move {
                        sqlx::query(&sql).execute(c).await?;
                        Ok(())
                    })
                })
                .connect(&url)
                .await
                .unwrap();
            sqlx::raw_sql(include_str!("../migrations/0001_accounts.sql"))
                .execute(&pool)
                .await
                .unwrap();
            sqlx::raw_sql(include_str!("../migrations/0002_chat.sql"))
                .execute(&pool)
                .await
                .unwrap();
            sqlx::raw_sql(include_str!("../migrations/0003_nodes.sql"))
                .execute(&pool)
                .await
                .unwrap();
            let dir = tempfile::tempdir().unwrap();
            let state = Arc::new(AppState {
                pool,
                attachments: dir.path().join("attachments"),
                public_url: "https://test.example".into(),
                mail: auth::Mailer::test(dir.path().to_owned()),
            });
            let mut users = vec![];
            for n in 0..3 {
                let id = Uuid::new_v4();
                sqlx::query("INSERT INTO accounts(id,name) VALUES($1,$2)")
                    .bind(id)
                    .bind(format!("human{n}"))
                    .execute(&state.pool)
                    .await
                    .unwrap();
                let token = Uuid::new_v4().to_string();
                let hash = format!("{:x}", Sha256::digest(token.as_bytes()));
                sqlx::query("INSERT INTO sessions(id,account_id,token_hash,expires_at) VALUES($1,$2,$3,now()+interval '1 day')").bind(Uuid::new_v4()).bind(id).bind(hash).execute(&state.pool).await.unwrap();
                let mut h = HeaderMap::new();
                h.insert(
                    header::COOKIE,
                    format!("nest_session={token}").parse().unwrap(),
                );
                users.push((id, h));
            }
            Some(Self {
                state,
                admin,
                schema,
                _dir: dir,
                users,
            })
        }
        pub(super) async fn close(self) {
            self.state.pool.close().await;
            sqlx::query(&format!("DROP SCHEMA {} CASCADE", self.schema))
                .execute(&self.admin)
                .await
                .unwrap();
        }
    }
    pub(super) fn input(text: &str) -> Input {
        serde_json::from_value(json!({"requestId":Uuid::new_v4(),"text":text})).unwrap()
    }
    #[tokio::test]
    async fn direct_idempotency_permissions_revisions_search_and_unread() {
        let Some(f) = Fixture::new().await else {
            return;
        };
        let a = f.users[0].1.clone();
        let b = f.users[1].1.clone();
        let c = f.users[2].1.clone();
        let target = f.users[1].0;
        let (one, two) = tokio::join!(
            direct(
                State(f.state.clone()),
                a.clone(),
                Json(Target {
                    account_id: target,
                    listed: None
                })
            ),
            direct(
                State(f.state.clone()),
                a.clone(),
                Json(Target {
                    account_id: target,
                    listed: None
                })
            )
        );
        let first = one.unwrap().0;
        assert_eq!(first["id"], two.unwrap().0["id"]);
        let cid = Uuid::parse_str(first["id"].as_str().unwrap()).unwrap();
        let request = Uuid::new_v4();
        let command = json!({"requestId":request,"text":"中文 A%_ literal"});
        let m = send(
            State(f.state.clone()),
            a.clone(),
            Path(cid),
            Json(serde_json::from_value(command.clone()).unwrap()),
        )
        .await
        .unwrap()
        .0;
        let replay = send(
            State(f.state.clone()),
            a.clone(),
            Path(cid),
            Json(serde_json::from_value(command).unwrap()),
        )
        .await
        .unwrap()
        .0;
        assert_eq!(m, replay);
        assert!(
            send(
                State(f.state.clone()),
                a.clone(),
                Path(cid),
                Json(
                    serde_json::from_value(json!({"requestId":request,"text":"changed"})).unwrap()
                )
            )
            .await
            .is_err()
        );
        assert!(
            messages(State(f.state.clone()), c, Path(cid), Query(Q::default()))
                .await
                .is_err()
        );
        let init_b = init(State(f.state.clone()), b.clone()).await.unwrap().0;
        assert_eq!(init_b["conversations"][0]["unreadCount"], 1);
        let found = search(
            State(f.state.clone()),
            b.clone(),
            Query(Q {
                query: Some("a%_".into()),
                ..Q::default()
            }),
        )
        .await
        .unwrap()
        .0;
        assert_eq!(found["messages"].as_array().unwrap().len(), 1);
        let mid = Uuid::parse_str(m["id"].as_str().unwrap()).unwrap();
        assert!(
            edit(
                State(f.state.clone()),
                b.clone(),
                Path(mid),
                Json(input("bad"))
            )
            .await
            .is_err()
        );
        let edited = edit(
            State(f.state.clone()),
            a.clone(),
            Path(mid),
            Json(
                serde_json::from_value(
                    json!({"requestId":Uuid::new_v4(),"expectedRevision":"1","text":"edited"}),
                )
                .unwrap(),
            ),
        )
        .await
        .unwrap()
        .0;
        assert_eq!(edited["revision"], "2");
        assert!(
            withdraw(
                State(f.state.clone()),
                a.clone(),
                Path(mid),
                Json(
                    serde_json::from_value(
                        json!({"requestId":Uuid::new_v4(),"expectedRevision":"1"})
                    )
                    .unwrap()
                )
            )
            .await
            .is_err()
        );
        let listed=list(State(f.state.clone()),b.clone(),Path(cid),Json(serde_json::from_value(json!({"requestId":Uuid::new_v4(),"expectedRevision":"1","readPosition":m["position"],"pinned":true})).unwrap())).await.unwrap().0;
        assert_eq!(listed["unreadCount"], 0);
        assert_eq!(listed["pinned"], true);
        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM cloud_messages")
            .fetch_one(&f.state.pool)
            .await
            .unwrap();
        assert_eq!(count, 1);
        f.close().await;
    }
    #[tokio::test]
    async fn attachments_export_and_withdraw() {
        let Some(f) = Fixture::new().await else {
            return;
        };
        let a = f.users[0].1.clone();
        let c = direct(
            State(f.state.clone()),
            a.clone(),
            Json(Target {
                account_id: f.users[1].0,
                listed: None,
            }),
        )
        .await
        .unwrap()
        .0;
        let cid = Uuid::parse_str(c["id"].as_str().unwrap()).unwrap();
        let payload = json!({"requestId":Uuid::new_v4(),"text":"file","files":[{"name":"空 文件.txt","mime":"text/plain","data":base64::engine::general_purpose::STANDARD.encode(b"original")}]});
        let m = send(
            State(f.state.clone()),
            a.clone(),
            Path(cid),
            Json(serde_json::from_value(payload.clone()).unwrap()),
        )
        .await
        .unwrap()
        .0;
        let mid = Uuid::parse_str(m["id"].as_str().unwrap()).unwrap();
        let response = download(
            State(f.state.clone()),
            a.clone(),
            Path((mid, 0)),
            Query(Q {
                revision: Some(1),
                ..Q::default()
            }),
        )
        .await
        .unwrap();
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        assert_eq!(&bytes[..], b"original");
        let response = export(
            State(f.state.clone()),
            a.clone(),
            Path(cid),
            Query(Q {
                include_files: Some(true),
                ..Q::default()
            }),
        )
        .await
        .unwrap();
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
        assert_eq!(zip.len(), 3);
        let mut data = String::new();
        std::io::Read::read_to_string(&mut zip.by_name("chat.json").unwrap(), &mut data).unwrap();
        let doc: Value = serde_json::from_str(&data).unwrap();
        assert_eq!(doc["version"], 3);
        assert!(
            doc["messages"][0]["attachments"][0]["filePath"]
                .as_str()
                .unwrap()
                .ends_with("空 文件.txt")
        );
        let changed = edit(
            State(f.state.clone()),
            a.clone(),
            Path(mid),
            Json(
                serde_json::from_value(
                    json!({"requestId":Uuid::new_v4(),"expectedRevision":"1","text":"revised"}),
                )
                .unwrap(),
            ),
        )
        .await
        .unwrap()
        .0;
        assert_eq!(changed["attachments"], m["attachments"]);
        assert!(
            download(
                State(f.state.clone()),
                a.clone(),
                Path((mid, 0)),
                Query(Q {
                    revision: Some(1),
                    ..Q::default()
                })
            )
            .await
            .is_err()
        );
        let current = download(
            State(f.state.clone()),
            a.clone(),
            Path((mid, 0)),
            Query(Q {
                revision: Some(2),
                ..Q::default()
            }),
        )
        .await
        .unwrap();
        assert_eq!(
            &axum::body::to_bytes(current.into_body(), usize::MAX)
                .await
                .unwrap()[..],
            b"original"
        );
        assert!(
            download(
                State(f.state.clone()),
                a.clone(),
                Path((mid, 0)),
                Query(Q {
                    revision: Some(2),
                    inline: Some(true),
                    ..Q::default()
                })
            )
            .await
            .is_err()
        );
        let _ = withdraw(
            State(f.state.clone()),
            a.clone(),
            Path(mid),
            Json(
                serde_json::from_value(json!({"requestId":Uuid::new_v4(),"expectedRevision":"2"}))
                    .unwrap(),
            ),
        )
        .await
        .unwrap();
        for zipped in [false, true] {
            let invalid = export(
                State(f.state.clone()),
                a.clone(),
                Path(cid),
                Query(Q {
                    format: Some("invalid".into()),
                    include_files: Some(zipped),
                    ..Q::default()
                }),
            )
            .await;
            assert_eq!(
                invalid.unwrap_err().status,
                axum::http::StatusCode::BAD_REQUEST
            );
            let exported = export(
                State(f.state.clone()),
                a.clone(),
                Path(cid),
                Query(Q {
                    format: Some("markdown".into()),
                    include_files: Some(zipped),
                    ..Q::default()
                }),
            )
            .await
            .unwrap();
            let bytes = axum::body::to_bytes(exported.into_body(), usize::MAX)
                .await
                .unwrap();
            let markdown = if zipped {
                let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
                assert_eq!(archive.len(), 2);
                let mut text = String::new();
                std::io::Read::read_to_string(&mut archive.by_name("chat.md").unwrap(), &mut text)
                    .unwrap();
                text
            } else {
                String::from_utf8(bytes.to_vec()).unwrap()
            };
            assert!(markdown.contains("[消息已撤回]"));
            assert!(!markdown.contains("revised"));
            assert!(!markdown.contains("source.txt"));
        }
        let replay = send(
            State(f.state.clone()),
            a.clone(),
            Path(cid),
            Json(serde_json::from_value(payload).unwrap()),
        )
        .await
        .unwrap()
        .0;
        assert_eq!(replay["status"], "withdrawn");
        assert_eq!(replay["text"], "");
        assert_eq!(replay["attachments"], json!([]));
        let receipts = sqlx::query("SELECT fingerprint,response FROM cloud_receipts")
            .fetch_all(&f.state.pool)
            .await
            .unwrap();
        for receipt in receipts {
            assert_eq!(receipt.get::<String, _>(0).len(), 64);
            let value: Value = receipt.get(1);
            assert_eq!(value.as_object().unwrap().len(), 1);
            assert!(value["id"].is_string());
        }
        assert!(
            download(
                State(f.state.clone()),
                a,
                Path((mid, 0)),
                Query(Q {
                    revision: Some(1),
                    ..Q::default()
                })
            )
            .await
            .is_err()
        );
        f.close().await;
    }
}
#[cfg(test)]
mod replay_tests {
    use super::*;
    #[tokio::test]
    async fn durable_events_replay_and_revoked_session_rejected() {
        use futures_util::StreamExt;
        let Some(f) = super::tests::Fixture::new().await else {
            return;
        };
        let a = f.users[0].1.clone();
        let b = f.users[1].0;
        let initial = init(State(f.state.clone()), a.clone()).await.unwrap().0;
        let cursor = initial["cursor"].as_str().unwrap().parse::<i64>().unwrap();
        let conv = direct(
            State(f.state.clone()),
            a.clone(),
            Json(Target {
                account_id: b,
                listed: None,
            }),
        )
        .await
        .unwrap()
        .0;
        let stream = events(
            State(f.state.clone()),
            a.clone(),
            Query(Q {
                after: Some(cursor),
                ..Q::default()
            }),
        )
        .await
        .unwrap()
        .into_response()
        .into_body()
        .into_data_stream();
        futures_util::pin_mut!(stream);
        let bytes = tokio::time::timeout(Duration::from_secs(3), stream.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        let text = std::str::from_utf8(&bytes).unwrap();
        assert!(text.contains("event: change"));
        assert!(text.contains(conv["id"].as_str().unwrap()));
        sqlx::query("UPDATE sessions SET revoked_at=now() WHERE account_id=$1")
            .bind(f.users[0].0)
            .execute(&f.state.pool)
            .await
            .unwrap();
        assert!(
            events(
                State(f.state.clone()),
                a,
                Query(Q {
                    after: Some(cursor),
                    ..Q::default()
                })
            )
            .await
            .is_err()
        );
        f.close().await;
    }
}
/// Compatibility probe for the JSON payloads held by the local SQLite store.
/// This is deliberately test-only; production migration needs its own preview/backup workflow.
#[cfg(test)]
mod local_mapping_tests {
    use super::*;
    #[derive(Deserialize)]
    struct LocalPayloads {
        actors: Vec<social::Actor>,
        messages: Vec<social::SocialMessage>,
    }
    #[tokio::test]
    async fn local_human_payload_mapping_preserves_ids_order_public_profile_and_original_bytes() {
        let Some(f) = super::tests::Fixture::new().await else {
            return;
        };
        let mut local: LocalPayloads =
            serde_json::from_str(include_str!("../tests/fixtures/local-human-payloads.json"))
                .unwrap();
        local.messages.sort_by_key(|message| message.sequence);
        let mut tx = f.state.pool.begin().await.unwrap();
        for actor in &local.actors {
            assert_eq!(actor.kind, social::ActorKind::Human);
            assert_eq!(actor.status, social::EntityStatus::Active);
            sqlx::query(
                "INSERT INTO accounts(id,name,bio,kind,created_at) VALUES($1,$2,$3,'human',$4)",
            )
            .bind(Uuid::parse_str(&actor.id.to_string()).unwrap())
            .bind(&actor.display_name)
            .bind(&actor.bio)
            .bind(actor.created_at)
            .execute(&mut *tx)
            .await
            .unwrap();
        }
        let owner = Uuid::parse_str(&local.actors[0].id.to_string()).unwrap();
        let other = Uuid::parse_str(&local.actors[1].id.to_string()).unwrap();
        let cid = Uuid::parse_str(&local.messages[0].conversation_id.to_string()).unwrap();
        sqlx::query("INSERT INTO cloud_conversations(id,first_id,second_id) VALUES($1,$2,$3)")
            .bind(cid)
            .bind(owner.min(other))
            .bind(owner.max(other))
            .execute(&mut *tx)
            .await
            .unwrap();
        for id in [owner, other] {
            sqlx::query("INSERT INTO cloud_list(owner,conversation) VALUES($1,$2)")
                .bind(id)
                .bind(cid)
                .execute(&mut *tx)
                .await
                .unwrap();
        }
        let mut source_order = vec![];
        let mut part_mapping = vec![];
        for source in &local.messages {
            assert_eq!(source.status, social::MessageStatus::Committed);
            let id = Uuid::parse_str(&source.id.to_string()).unwrap();
            source_order.push(id);
            let mut input = super::tests::input("");
            let mut texts = vec![];
            for (old_index, part) in source.body.0.iter().enumerate() {
                match part {
                    social::SocialContent::Text { text } => texts.push(text.clone()),
                    social::SocialContent::Image { mime, data } => {
                        part_mapping.push((old_index, input.images.len()));
                        input.images.push(Upload {
                            name: None,
                            mime: mime.clone().unwrap(),
                            data: base64::engine::general_purpose::STANDARD.encode(data),
                        });
                    }
                    social::SocialContent::File { name, mime, data } => {
                        part_mapping.push((old_index, input.images.len() + input.files.len()));
                        input.files.push(Upload {
                            name: name.clone(),
                            mime: mime.clone(),
                            data: base64::engine::general_purpose::STANDARD.encode(data),
                        });
                    }
                }
            }
            let parts = uploads(&f.state, id, &input).await.unwrap();
            sqlx::query("INSERT INTO cloud_messages(id,conversation,sender,body,created_at,updated_at,revision,attachments) VALUES($1,$2,$3,$4,$5,$5,$6,$7)")
                .bind(id).bind(cid).bind(Uuid::parse_str(&source.sender_id.to_string()).unwrap()).bind(texts.join("\n")).bind(source.created_at).bind(source.revision.get() as i64).bind(parts).execute(&mut *tx).await.unwrap();
        }
        tx.commit().await.unwrap();
        let mut tx = f.state.pool.begin().await.unwrap();
        let profile = account(&mut tx, owner).await.unwrap();
        assert_eq!(profile["id"], owner.to_string());
        assert_eq!(profile["name"], "旧库真人甲");
        assert_eq!(profile["bio"], "公开简介");
        let rows =
            sqlx::query("SELECT * FROM cloud_messages WHERE conversation=$1 ORDER BY position")
                .bind(cid)
                .fetch_all(&mut *tx)
                .await
                .unwrap();
        assert_eq!(
            rows.iter()
                .map(|r| r.get::<Uuid, _>("id"))
                .collect::<Vec<_>>(),
            source_order
        );
        assert_eq!(message(&rows[0])["text"], "原文，不是摘要。");
        assert_eq!(message(&rows[1])["senderId"], other.to_string());
        assert_eq!(message(&rows[1])["revision"], "2");
        // Local indices count text parts; cloud indices address only attachment parts.
        assert_eq!(part_mapping, vec![(1, 0), (2, 1)]);
        let last = &local.messages[1];
        for (index, source) in last.body.0.iter().skip(1).enumerate() {
            let expected = match source {
                social::SocialContent::Image { data, .. }
                | social::SocialContent::File { data, .. } => data,
                _ => unreachable!(),
            };
            let bytes = tokio::fs::read(
                f.state
                    .attachments
                    .join(last.id.to_string())
                    .join(index.to_string()),
            )
            .await
            .unwrap();
            assert_eq!(&bytes, expected);
        }
        drop(tx);
        f.close().await;
    }
}
