//! Cloud group membership, relationship controls and their transactional commands.
use crate::{AppError, AppState, Result, auth, chat, db as sqlx, nodes};
use axum::{
    Json, Router,
    extract::{Path, State},
    http::HeaderMap,
    routing::{get, post, put},
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::{Postgres, Row, Transaction};
use std::sync::Arc;
use uuid::Uuid;
type Tx<'a> = Transaction<'a, Postgres>;
fn field<'a>(v: &'a Value, k: &str) -> Result<&'a str> {
    v[k].as_str()
        .ok_or_else(|| AppError::bad("请求字段缺失或类型错误"))
}
fn uuid(v: &Value, k: &str) -> Result<Uuid> {
    field(v, k)?.parse().map_err(|_| AppError::bad("标识无效"))
}
fn fingerprint(v: &Value, op: &str, id: Uuid) -> String {
    format!(
        "{:x}",
        Sha256::digest(json!([op, id, v]).to_string().as_bytes())
    )
}
pub(crate) fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/chat/groups", post(create))
        .route("/chat/groups/{id}", put(rename))
        .route("/chat/groups/{id}/members", post(add))
        .route(
            "/chat/groups/{id}/members/{account}",
            put(role).delete(remove),
        )
        .route("/chat/groups/{id}/leave", post(leave))
        .route("/chat/groups/{id}/owner", post(transfer))
        .route("/chat/contacts/{account}", get(contact))
        .route("/chat/contacts/{account}/block", put(block))
}
async fn replay(
    tx: &mut Tx<'_>,
    owner: Uuid,
    v: &Value,
    op: &str,
    target: Uuid,
) -> Result<Option<Uuid>> {
    let row = sqlx::query(
        "SELECT fingerprint,conversation FROM cloud_group_receipts WHERE owner=$1 AND request=$2",
    )
    .bind(owner)
    .bind(uuid(v, "requestId")?)
    .fetch_optional(&mut **tx)
    .await?;
    if let Some(row) = row {
        if row.get::<String, _>(0) != fingerprint(v, op, target) {
            return Err(AppError::conflict("命令标识已用于其他内容"));
        }
        return Ok(Some(row.get::<Option<Uuid>, _>(1).unwrap_or(target)));
    }
    Ok(None)
}
async fn save(
    tx: &mut Tx<'_>,
    owner: Uuid,
    v: &Value,
    op: &str,
    target: Uuid,
    cid: Option<Uuid>,
) -> Result<()> {
    sqlx::query("INSERT INTO cloud_group_receipts(owner,request,fingerprint,conversation) VALUES($1,$2,$3,$4)").bind(owner).bind(uuid(v,"requestId")?).bind(fingerprint(v,op,target)).bind(cid).execute(&mut **tx).await?;
    Ok(())
}
fn name(v: &Value) -> Result<&str> {
    let n = field(v, "name")?.trim();
    if n.is_empty() || n.chars().count() > 100 {
        return Err(AppError::bad("群名须为1至100字"));
    }
    Ok(n)
}
async fn create(
    State(s): State<Arc<AppState>>,
    h: HeaderMap,
    Json(v): Json<Value>,
) -> Result<Json<Value>> {
    let p = auth::authenticate(&s, &h).await?;
    let mut tx = s.pool.begin().await?;
    nodes::lock(&mut tx).await?;
    if let Some(cid) = replay(&mut tx, p.id, &v, "create", p.id).await? {
        return Ok(Json(chat::conversation(&mut tx, cid, p.id).await?));
    }
    let name = name(&v)?;
    let mut members: Vec<Uuid> = serde_json::from_value(v["memberIds"].clone())
        .map_err(|_| AppError::bad("成员列表无效"))?;
    members.push(p.id);
    members.sort();
    members.dedup();
    if !(2..=100).contains(&members.len()) {
        return Err(AppError::bad("群成员须为2至100人"));
    }
    for &id in &members {
        chat::account(&mut tx, id).await?;
    }
    let cid = Uuid::new_v4();
    sqlx::query("INSERT INTO cloud_conversations(id,kind,name) VALUES($1,'group',$2)")
        .bind(cid)
        .bind(name)
        .execute(&mut *tx)
        .await?;
    for &id in &members {
        sqlx::query("INSERT INTO cloud_members(conversation,account_id,role) VALUES($1,$2,$3)")
            .bind(cid)
            .bind(id)
            .bind(if id == p.id { "owner" } else { "member" })
            .execute(&mut *tx)
            .await?;
        sqlx::query("INSERT INTO cloud_list(owner,conversation) VALUES($1,$2)")
            .bind(id)
            .bind(cid)
            .execute(&mut *tx)
            .await?;
    }
    chat::notify(&mut tx, cid, members).await?;
    save(&mut tx, p.id, &v, "create", p.id, Some(cid)).await?;
    let out = chat::conversation(&mut tx, cid, p.id).await?;
    tx.commit().await?;
    Ok(Json(out))
}
async fn command(
    s: Arc<AppState>,
    h: HeaderMap,
    cid: Uuid,
    target: Option<Uuid>,
    v: Value,
    op: &str,
) -> Result<Json<Value>> {
    let p = auth::authenticate(&s, &h).await?;
    let mut tx = s.pool.begin().await?;
    nodes::lock(&mut tx).await?;
    let receipt_target = target.unwrap_or(cid);
    let receipt_operation = format!("{op}:{cid}");
    if replay(&mut tx, p.id, &v, &receipt_operation, receipt_target)
        .await?
        .is_some()
    {
        return Ok(Json(if op == "leave" {
            json!({"ok":true})
        } else {
            chat::conversation(&mut tx, cid, p.id).await?
        }));
    }
    let old = chat::member(&mut tx, cid, p.id).await?;
    let c = sqlx::query("SELECT kind,revision FROM cloud_conversations WHERE id=$1")
        .bind(cid)
        .fetch_one(&mut *tx)
        .await?;
    if c.get::<String, _>("kind") != "group" {
        return Err(AppError::bad("仅群聊支持此操作"));
    }
    if field(&v, "expectedRevision")? != c.get::<i64, _>("revision").to_string() {
        return Err(AppError::conflict("群版本已变化"));
    }
    let role: String = sqlx::query_scalar(
        "SELECT role FROM cloud_members WHERE conversation=$1 AND account_id=$2",
    )
    .bind(cid)
    .bind(p.id)
    .fetch_one(&mut *tx)
    .await?;
    let account = match op {
        "add" | "transfer" => Some(uuid(&v, "accountId")?),
        "leave" => Some(p.id),
        _ => target,
    };
    let target_role = if let Some(account) = account {
        sqlx::query_scalar::<_, String>(
            "SELECT role FROM cloud_members WHERE conversation=$1 AND account_id=$2",
        )
        .bind(cid)
        .bind(account)
        .fetch_optional(&mut *tx)
        .await?
    } else {
        None
    };
    match op {
        "rename" => {
            if role == "member" {
                return Err(AppError::forbidden());
            }
            sqlx::query("UPDATE cloud_conversations SET name=$2 WHERE id=$1")
                .bind(cid)
                .bind(name(&v)?)
                .execute(&mut *tx)
                .await?;
        }
        "add" => {
            if role == "member" {
                return Err(AppError::forbidden());
            }
            if old.len() >= 100 {
                return Err(AppError::bad("群成员已达100人"));
            }
            let a = account.unwrap();
            chat::account(&mut tx, a).await?;
            if target_role.is_some() {
                return Err(AppError::conflict("账号已在群中"));
            }
            sqlx::query("INSERT INTO cloud_members(conversation,account_id) VALUES($1,$2)")
                .bind(cid)
                .bind(a)
                .execute(&mut *tx)
                .await?;
            sqlx::query("INSERT INTO cloud_list(owner,conversation,read_position) VALUES($1,$2,(SELECT coalesce(max(position),0) FROM cloud_messages WHERE conversation=$2 AND status<>'generating')) ON CONFLICT DO NOTHING").bind(a).bind(cid).execute(&mut *tx).await?;
        }
        "role" => {
            if role != "owner" || target_role.as_deref() == Some("owner") {
                return Err(AppError::forbidden());
            }
            let r = field(&v, "role")?;
            if !matches!(r, "admin" | "member") {
                return Err(AppError::bad("群角色无效"));
            }
            if target_role.is_none() {
                return Err(AppError::not_found());
            }
            sqlx::query("UPDATE cloud_members SET role=$3 WHERE conversation=$1 AND account_id=$2")
                .bind(cid)
                .bind(account.unwrap())
                .bind(r)
                .execute(&mut *tx)
                .await?;
        }
        "remove" | "leave" => {
            let tr = target_role.as_deref().ok_or_else(AppError::not_found)?;
            if tr == "owner"
                || op == "remove" && !(role == "owner" || role == "admin" && tr == "member")
            {
                return Err(AppError::forbidden());
            }
            sqlx::query("DELETE FROM cloud_members WHERE conversation=$1 AND account_id=$2")
                .bind(cid)
                .bind(account.unwrap())
                .execute(&mut *tx)
                .await?;
        }
        "transfer" => {
            if role != "owner" {
                return Err(AppError::forbidden());
            }
            if target_role.is_none() || account == Some(p.id) {
                return Err(AppError::bad("请选择其他当前真人成员"));
            }
            if chat::account(&mut tx, account.unwrap()).await?["kind"] != "human" {
                return Err(AppError::bad("群主须为真人"));
            }
            sqlx::query(
                "UPDATE cloud_members SET role='member' WHERE conversation=$1 AND account_id=$2",
            )
            .bind(cid)
            .bind(p.id)
            .execute(&mut *tx)
            .await?;
            sqlx::query(
                "UPDATE cloud_members SET role='owner' WHERE conversation=$1 AND account_id=$2",
            )
            .bind(cid)
            .bind(account.unwrap())
            .execute(&mut *tx)
            .await?;
        }
        _ => unreachable!(),
    }
    sqlx::query("UPDATE cloud_conversations SET revision=revision+1 WHERE id=$1")
        .bind(cid)
        .execute(&mut *tx)
        .await?;
    nodes::conversation_changed(&mut tx, cid, "membership_changed").await?;
    crate::consciousness::membership_changed(&mut tx, cid, &old).await?;
    let mut recipients = old;
    let current: Vec<Uuid> =
        sqlx::query_scalar("SELECT account_id FROM cloud_members WHERE conversation=$1")
            .bind(cid)
            .fetch_all(&mut *tx)
            .await?;
    recipients.extend(current);
    recipients.sort();
    recipients.dedup();
    chat::notify(&mut tx, cid, recipients).await?;
    save(
        &mut tx,
        p.id,
        &v,
        &receipt_operation,
        receipt_target,
        Some(cid),
    )
    .await?;
    let out = if op == "leave" {
        json!({"ok":true})
    } else {
        chat::conversation(&mut tx, cid, p.id).await?
    };
    tx.commit().await?;
    Ok(Json(out))
}
async fn rename(
    State(s): State<Arc<AppState>>,
    h: HeaderMap,
    Path(cid): Path<Uuid>,
    Json(v): Json<Value>,
) -> Result<Json<Value>> {
    command(s, h, cid, None, v, "rename").await
}
async fn add(
    State(s): State<Arc<AppState>>,
    h: HeaderMap,
    Path(cid): Path<Uuid>,
    Json(v): Json<Value>,
) -> Result<Json<Value>> {
    command(s, h, cid, None, v, "add").await
}
async fn role(
    State(s): State<Arc<AppState>>,
    h: HeaderMap,
    Path((cid, a)): Path<(Uuid, Uuid)>,
    Json(v): Json<Value>,
) -> Result<Json<Value>> {
    command(s, h, cid, Some(a), v, "role").await
}
async fn remove(
    State(s): State<Arc<AppState>>,
    h: HeaderMap,
    Path((cid, a)): Path<(Uuid, Uuid)>,
    Json(v): Json<Value>,
) -> Result<Json<Value>> {
    command(s, h, cid, Some(a), v, "remove").await
}
async fn leave(
    State(s): State<Arc<AppState>>,
    h: HeaderMap,
    Path(cid): Path<Uuid>,
    Json(v): Json<Value>,
) -> Result<Json<Value>> {
    command(s, h, cid, None, v, "leave").await
}
async fn transfer(
    State(s): State<Arc<AppState>>,
    h: HeaderMap,
    Path(cid): Path<Uuid>,
    Json(v): Json<Value>,
) -> Result<Json<Value>> {
    command(s, h, cid, None, v, "transfer").await
}
pub(crate) async fn direct_blocked(tx: &mut Tx<'_>, cid: Uuid) -> Result<bool> {
    Ok(sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM cloud_conversations c JOIN cloud_contacts r ON (r.owner=c.first_id AND r.target=c.second_id OR r.owner=c.second_id AND r.target=c.first_id) WHERE c.id=$1 AND c.kind='direct' AND r.blocked)").bind(cid).fetch_one(&mut **tx).await?)
}
pub(crate) async fn validate_send(
    tx: &mut Tx<'_>,
    cid: Uuid,
    members: &[Uuid],
    reply: Option<Uuid>,
    mentions: &[Uuid],
) -> Result<()> {
    if direct_blocked(tx, cid).await? {
        return Err(AppError::forbidden());
    }
    if mentions.len() > 100 || mentions.iter().any(|id| !members.contains(id)) {
        return Err(AppError::bad("提及账号不是当前成员"));
    }
    if let Some(id) = reply
        && !sqlx::query_scalar::<_,bool>("SELECT EXISTS(SELECT 1 FROM cloud_messages WHERE id=$1 AND conversation=$2 AND NOT withdrawn AND status='committed')").bind(id).bind(cid).fetch_one(&mut **tx).await? {
        return Err(AppError::bad("回复原文无效"));
    }
    Ok(())
}
async fn contact_view(tx: &mut Tx<'_>, owner: Uuid, target: Uuid) -> Result<Value> {
    let account = chat::account(tx, target).await?;
    let r = sqlx::query(
        "SELECT listed,blocked,revision FROM cloud_contacts WHERE owner=$1 AND target=$2",
    )
    .bind(owner)
    .bind(target)
    .fetch_optional(&mut **tx)
    .await?;
    let both:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM cloud_contacts WHERE (owner=$1 AND target=$2 OR owner=$2 AND target=$1) AND blocked)").bind(owner).bind(target).fetch_one(&mut **tx).await?;
    Ok(
        json!({"account":account,"listed":r.as_ref().map(|r|r.get::<bool,_>(0)).unwrap_or(false),"blocked":r.as_ref().map(|r|r.get::<bool,_>(1)).unwrap_or(false),"revision":r.map(|r|r.get::<i64,_>(2)).unwrap_or(0).to_string(),"directBlocked":both}),
    )
}
async fn contact(
    State(s): State<Arc<AppState>>,
    h: HeaderMap,
    Path(a): Path<Uuid>,
) -> Result<Json<Value>> {
    let p = auth::authenticate(&s, &h).await?;
    let mut tx = s.pool.begin().await?;
    Ok(Json(contact_view(&mut tx, p.id, a).await?))
}
async fn block(
    State(s): State<Arc<AppState>>,
    h: HeaderMap,
    Path(a): Path<Uuid>,
    Json(v): Json<Value>,
) -> Result<Json<Value>> {
    let p = auth::authenticate(&s, &h).await?;
    if a == p.id {
        return Err(AppError::bad("不能屏蔽自己"));
    }
    let mut tx = s.pool.begin().await?;
    nodes::lock(&mut tx).await?;
    let current = contact_view(&mut tx, p.id, a).await?;
    if replay(&mut tx, p.id, &v, "block", a).await?.is_some() {
        return Ok(Json(current));
    }
    if v["expectedRevision"] != current["revision"] {
        return Err(AppError::conflict("联系人版本已变化"));
    }
    let blocked = v["blocked"]
        .as_bool()
        .ok_or_else(|| AppError::bad("缺少屏蔽状态"))?;
    sqlx::query("INSERT INTO cloud_contacts(owner,target,listed,blocked) VALUES($1,$2,false,$3) ON CONFLICT(owner,target) DO UPDATE SET blocked=$3,revision=cloud_contacts.revision+1").bind(p.id).bind(a).bind(blocked).execute(&mut *tx).await?;
    if blocked {
        let cid:Option<Uuid>=sqlx::query_scalar("SELECT id FROM cloud_conversations WHERE kind='direct' AND (first_id=$1 AND second_id=$2 OR first_id=$2 AND second_id=$1)").bind(p.id).bind(a).fetch_optional(&mut *tx).await?;
        if let Some(cid) = cid {
            nodes::conversation_changed(&mut tx, cid, "blocked").await?;
        }
    }
    if blocked {
        crate::participation::reconcile(&mut tx).await?;
    }
    for id in [p.id, a] {
        sqlx::query("INSERT INTO cloud_events(owner) VALUES($1)")
            .bind(id)
            .execute(&mut *tx)
            .await?;
    }
    save(&mut tx, p.id, &v, "block", a, None).await?;
    let out = contact_view(&mut tx, p.id, a).await?;
    tx.commit().await?;
    Ok(Json(out))
}
