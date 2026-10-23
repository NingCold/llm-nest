//! Durable cloud participation and one-time contact authorization.
use crate::{AppError, AppState, Result, auth, chat, db as sqlx, nodes};
use axum::{
    Json, Router,
    extract::{Path, State},
    http::HeaderMap,
    routing::{get, post, put},
};
use chrono::{DateTime, Duration, Utc};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::{Postgres, Row, Transaction};
use std::sync::Arc;
use uuid::Uuid;
type Tx<'a> = Transaction<'a, Postgres>;
fn uid(v: &Value, k: &str) -> Result<Uuid> {
    v[k].as_str()
        .and_then(|v| v.parse().ok())
        .ok_or_else(|| AppError::bad("标识无效"))
}
fn time(v: &Value, k: &str) -> Result<DateTime<Utc>> {
    v[k].as_str()
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| AppError::bad("时间格式无效"))
}
pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route(
            "/chat/conversations/{id}/participation",
            get(read).put(policy),
        )
        .route(
            "/chat/conversations/{id}/participation/control",
            post(control),
        )
        .route("/chat/conversations/{id}/contact-settings", put(settings))
        .route("/chat/conversations/{id}/contact-intents", post(create))
        .route(
            "/chat/conversations/{id}/contact-intents/{intent}/cancel",
            post(cancel),
        )
}
pub(crate) async fn ensure(tx: &mut Tx<'_>, cid: Uuid) -> Result<()> {
    sqlx::query("INSERT INTO cloud_participation(conversation) VALUES($1) ON CONFLICT DO NOTHING")
        .bind(cid)
        .execute(&mut **tx)
        .await?;
    sqlx::query("UPDATE cloud_participation SET budget_day=greatest(budget_day,(now() AT TIME ZONE 'UTC')::date) WHERE conversation=$1 AND budget_day<(now() AT TIME ZONE 'UTC')::date").bind(cid).execute(&mut **tx).await?;
    Ok(())
}
async fn permission(tx: &mut Tx<'_>, cid: Uuid, owner: Uuid) -> Result<bool> {
    let r=sqlx::query("SELECT c.kind,m.role,a.kind AS account_kind FROM cloud_members m JOIN cloud_conversations c ON c.id=m.conversation JOIN accounts a ON a.id=m.account_id WHERE m.conversation=$1 AND m.account_id=$2").bind(cid).bind(owner).fetch_optional(&mut **tx).await?.ok_or_else(AppError::not_found)?;
    if r.get::<String, _>("account_kind") != "human" {
        return Err(AppError::forbidden());
    }
    Ok(r.get::<String, _>("kind") == "direct"
        || matches!(r.get::<String, _>("role").as_str(), "owner" | "admin"))
}
async fn changed(tx: &mut Tx<'_>, cid: Uuid) -> Result<()> {
    sqlx::query("INSERT INTO cloud_events(owner,conversation) SELECT account_id,conversation FROM cloud_members WHERE conversation=$1").bind(cid).execute(&mut **tx).await?;
    Ok(())
}
fn intent(r: &sqlx::postgres::PgRow) -> Value {
    json!({"id":r.get::<Uuid,_>("id"),"creatorId":r.get::<Uuid,_>("creator_id"),"aiId":r.get::<Uuid,_>("ai_id"),"content":r.get::<String,_>("content"),"dueAt":r.get::<DateTime<Utc>,_>("due_at"),"expiresAt":r.get::<DateTime<Utc>,_>("expires_at"),"utcOffsetMinutes":r.get::<i32,_>("utc_offset_minutes"),"status":r.get::<String,_>("status"),"revision":r.get::<i64,_>("revision").to_string(),"reason":r.get::<Option<String>,_>("reason"),"taskId":r.get::<Option<Uuid>,_>("task_id")})
}
async fn view(tx: &mut Tx<'_>, cid: Uuid, owner: Uuid) -> Result<Value> {
    let manage = permission(tx, cid, owner).await?;
    ensure(tx, cid).await?;
    let r=sqlx::query("SELECT p.*,coalesce(d.calls,0) AS daily_calls FROM cloud_participation p LEFT JOIN cloud_participation_daily d ON d.conversation=p.conversation AND d.day=p.budget_day WHERE p.conversation=$1").bind(cid).fetch_one(&mut **tx).await?;
    let rows=sqlx::query("SELECT * FROM cloud_contact_intents WHERE conversation=$1 AND (status IN ('pending','claimed','executing') OR id IN (SELECT id FROM cloud_contact_intents WHERE conversation=$1 AND status NOT IN ('pending','claimed','executing') ORDER BY created_at DESC LIMIT 50)) ORDER BY created_at DESC").bind(cid).fetch_all(&mut **tx).await?;
    Ok(
        json!({"revision":r.get::<i64,_>("revision").to_string(),"canManage":manage,"policy":r.get::<Value,_>("policy"),"state":{"paused":r.get::<bool,_>("paused"),"pausedAiIds":r.get::<Vec<Uuid>,_>("paused_ai"),"boutId":r.get::<Option<Uuid>,_>("bout_id"),"consecutiveAi":r.get::<i32,_>("consecutive_ai"),"boutCalls":r.get::<i32,_>("bout_calls"),"dailyCalls":r.get::<i32,_>("daily_calls"),"reason":r.get::<Option<String>,_>("reason")},"settings":{"revision":r.get::<i64,_>("settings_revision").to_string(),"enabled":r.get::<bool,_>("enabled"),"quietUntil":r.get::<Option<DateTime<Utc>>,_>("quiet_until"),"quietAction":r.get::<String,_>("quiet_action")},"intents":rows.iter().map(intent).collect::<Vec<_>>()}),
    )
}
async fn receipt(
    tx: &mut Tx<'_>,
    owner: Uuid,
    cid: Uuid,
    v: &Value,
    op: &str,
) -> Result<Option<Uuid>> {
    let fp = format!(
        "{:x}",
        Sha256::digest(json!([op, cid, v]).to_string().as_bytes())
    );
    let rid = uid(v, "requestId")?;
    if let Some(r) = sqlx::query(
        "SELECT fingerprint,target FROM cloud_participation_receipts WHERE owner=$1 AND request=$2",
    )
    .bind(owner)
    .bind(rid)
    .fetch_optional(&mut **tx)
    .await?
    {
        if r.get::<String, _>("fingerprint") != fp {
            return Err(AppError::conflict("请求标识已用于其他命令"));
        }
        return Ok(Some(r.get("target")));
    }
    Ok(None)
}
async fn save(
    tx: &mut Tx<'_>,
    owner: Uuid,
    cid: Uuid,
    v: &Value,
    op: &str,
    target: Uuid,
) -> Result<()> {
    let fp = format!(
        "{:x}",
        Sha256::digest(json!([op, cid, v]).to_string().as_bytes())
    );
    sqlx::query("INSERT INTO cloud_participation_receipts(owner,request,fingerprint,target) VALUES($1,$2,$3,$4)").bind(owner).bind(uid(v,"requestId")?).bind(fp).bind(target).execute(&mut **tx).await?;
    Ok(())
}
async fn read(
    State(s): State<Arc<AppState>>,
    h: HeaderMap,
    Path(cid): Path<Uuid>,
) -> Result<Json<Value>> {
    let p = auth::authenticate(&s, &h).await?;
    let mut tx = s.pool.begin().await?;
    nodes::lock(&mut tx).await?;
    let out = view(&mut tx, cid, p.id).await?;
    tx.commit().await?;
    Ok(Json(out))
}
fn revision(v: &Value, r: i64) -> Result<()> {
    if v["expectedRevision"].as_str() != Some(r.to_string().as_str()) {
        return Err(AppError::conflict("版本已变化，请刷新后重试"));
    }
    Ok(())
}
fn validate_policy(v: &Value) -> Result<()> {
    if !matches!(v["mode"].as_str(), Some("mentionedOnly" | "limitedAuto")) {
        return Err(AppError::bad("接话模式无效"));
    }
    for (k, min, max) in [
        ("directMergeMs", 0, 2000),
        ("maxConsecutiveAi", 1, 6),
        ("maxBoutCalls", 1, 48),
        ("maxDailyCalls", 1, 200),
        ("idleSeconds", 30, 3600),
    ] {
        if !v[k].as_i64().is_some_and(|n| (min..=max).contains(&n)) {
            return Err(AppError::bad("接话规则超出允许范围"));
        }
    }
    Ok(())
}
async fn policy(
    State(s): State<Arc<AppState>>,
    h: HeaderMap,
    Path(cid): Path<Uuid>,
    Json(v): Json<Value>,
) -> Result<Json<Value>> {
    mutate(s, h, cid, v, "policy").await
}
async fn control(
    State(s): State<Arc<AppState>>,
    h: HeaderMap,
    Path(cid): Path<Uuid>,
    Json(v): Json<Value>,
) -> Result<Json<Value>> {
    mutate(s, h, cid, v, "control").await
}
async fn settings(
    State(s): State<Arc<AppState>>,
    h: HeaderMap,
    Path(cid): Path<Uuid>,
    Json(v): Json<Value>,
) -> Result<Json<Value>> {
    mutate(s, h, cid, v, "settings").await
}
async fn mutate(
    s: Arc<AppState>,
    h: HeaderMap,
    cid: Uuid,
    v: Value,
    op: &str,
) -> Result<Json<Value>> {
    let p = auth::authenticate(&s, &h).await?;
    let mut tx = s.pool.begin().await?;
    nodes::lock(&mut tx).await?;
    if !permission(&mut tx, cid, p.id).await? {
        return Err(AppError::forbidden());
    }
    chat::writable(&mut tx, cid).await?;
    ensure(&mut tx, cid).await?;
    if receipt(&mut tx, p.id, cid, &v, op).await?.is_none() {
        let r = sqlx::query("SELECT * FROM cloud_participation WHERE conversation=$1")
            .bind(cid)
            .fetch_one(&mut *tx)
            .await?;
        revision(
            &v,
            r.get(if op == "settings" {
                "settings_revision"
            } else {
                "revision"
            }),
        )?;
        if op == "policy" {
            validate_policy(&v["policy"])?;
            sqlx::query("UPDATE cloud_participation SET policy=$2,revision=revision+1,reason='policy_changed' WHERE conversation=$1").bind(cid).bind(&v["policy"]).execute(&mut *tx).await?;
            let rows=sqlx::query("SELECT * FROM cloud_node_tasks WHERE conversation=$1 AND automatic AND state IN ('queued','running','uncertain')").bind(cid).fetch_all(&mut *tx).await?;
            nodes::invalidate_rows(&mut tx, rows, "policy_changed").await?;
        } else if op == "settings" {
            let enabled = v["enabled"]
                .as_bool()
                .ok_or_else(|| AppError::bad("主动联系开关无效"))?;
            let quiet = if v["quietUntil"].is_null() {
                None
            } else {
                Some(time(&v, "quietUntil")?)
            };
            let action = v["quietAction"]
                .as_str()
                .filter(|s| matches!(*s, "defer" | "skip"))
                .ok_or_else(|| AppError::bad("免打扰处理方式无效"))?;
            sqlx::query("UPDATE cloud_participation SET enabled=$2,quiet_until=$3,quiet_action=$4,settings_revision=settings_revision+1 WHERE conversation=$1").bind(cid).bind(enabled).bind(quiet).bind(action).execute(&mut *tx).await?;
            if !enabled {
                cancel_intents(&mut tx, cid, None, "authorization_closed").await?;
            } else if quiet.is_some_and(|t| t > Utc::now()) {
                let rows=sqlx::query("SELECT * FROM cloud_node_tasks WHERE conversation=$1 AND contact_intent_id IS NOT NULL AND state='running'").bind(cid).fetch_all(&mut *tx).await?;
                nodes::invalidate_rows(&mut tx, rows, "quiet").await?;
            }
        } else {
            let action = v["action"]
                .as_str()
                .filter(|a| matches!(*a, "pause" | "resume" | "end"))
                .ok_or_else(|| AppError::bad("控制操作无效"))?;
            let ai = if v["aiId"].is_null() {
                None
            } else {
                Some(uid(&v, "aiId")?)
            };
            if ai.is_some() && action == "end" {
                return Err(AppError::bad("结束操作仅支持整个窗口"));
            }
            if let Some(ai) = ai {
                let valid:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM cloud_members m JOIN accounts a ON a.id=m.account_id WHERE conversation=$1 AND account_id=$2 AND a.kind='ai')").bind(cid).bind(ai).fetch_one(&mut *tx).await?;
                if !valid {
                    return Err(AppError::bad("目标 AI 不是当前成员"));
                }
            }
            if action == "resume" {
                sqlx::query("UPDATE cloud_contact_intents SET status='skipped',reason='paused',revision=revision+1 WHERE conversation=$1 AND status='pending' AND due_at<=now() AND ($2::uuid IS NULL OR ai_id=$2)").bind(cid).bind(ai).execute(&mut *tx).await?;
            }
            if action == "end" {
                sqlx::query("UPDATE cloud_contact_intents SET status='cancelled',reason='user_ended',revision=revision+1 WHERE conversation=$1 AND status='pending' AND due_at<=now()").bind(cid).execute(&mut *tx).await?;
                sqlx::query("UPDATE cloud_participation SET bout_id=NULL,bout_calls=0,consecutive_ai=0,reason='user_ended',revision=revision+1 WHERE conversation=$1").bind(cid).execute(&mut *tx).await?;
            } else if let Some(ai) = ai {
                sqlx::query("UPDATE cloud_participation SET paused_ai=CASE WHEN $3 THEN array_append(array_remove(paused_ai,$2),$2) ELSE array_remove(paused_ai,$2) END,revision=revision+1 WHERE conversation=$1").bind(cid).bind(ai).bind(action=="pause").execute(&mut *tx).await?;
            } else {
                sqlx::query("UPDATE cloud_participation SET paused=$2,revision=revision+1 WHERE conversation=$1").bind(cid).bind(action=="pause").execute(&mut *tx).await?;
            }
            if action != "resume" {
                let rows=sqlx::query("SELECT * FROM cloud_node_tasks WHERE conversation=$1 AND ($2::uuid IS NULL OR ai_id=$2) AND state IN ('queued','running','uncertain')").bind(cid).bind(ai).fetch_all(&mut *tx).await?;
                nodes::invalidate_rows(&mut tx, rows, "cancelled").await?;
            }
        }
        if op == "settings" {
            sqlx::query("UPDATE cloud_node_tasks SET ready_notified=false,not_before=greatest(now(),coalesce((SELECT quiet_until FROM cloud_participation WHERE conversation=$1),now())) WHERE conversation=$1 AND contact_intent_id IS NOT NULL AND state='queued'").bind(cid).execute(&mut *tx).await?;
        } else if op == "control" {
            sqlx::query("UPDATE cloud_node_tasks SET policy_revision=(SELECT revision FROM cloud_participation WHERE conversation=$1) WHERE conversation=$1 AND automatic AND state='queued'").bind(cid).execute(&mut *tx).await?;
        }
        save(&mut tx, p.id, cid, &v, op, cid).await?;
        changed(&mut tx, cid).await?;
    }
    let out = view(&mut tx, cid, p.id).await?;
    tx.commit().await?;
    Ok(Json(out))
}
pub(crate) async fn contact_allowed(
    tx: &mut Tx<'_>,
    cid: Uuid,
    creator: Uuid,
    ai: Uuid,
) -> Result<bool> {
    Ok(sqlx::query_scalar("SELECT (SELECT count(*) FROM cloud_members WHERE conversation=$1 AND account_id IN ($2,$3))=2 AND EXISTS(SELECT 1 FROM accounts WHERE id=$3 AND kind='ai' AND status='active') AND NOT EXISTS(SELECT 1 FROM cloud_contacts WHERE blocked AND (owner=$2 AND target=$3 OR owner=$3 AND target=$2))").bind(cid).bind(creator).bind(ai).fetch_one(&mut **tx).await?)
}
async fn create(
    State(s): State<Arc<AppState>>,
    h: HeaderMap,
    Path(cid): Path<Uuid>,
    Json(v): Json<Value>,
) -> Result<Json<Value>> {
    let p = auth::authenticate(&s, &h).await?;
    let mut tx = s.pool.begin().await?;
    nodes::lock(&mut tx).await?;
    permission(&mut tx, cid, p.id).await?;
    chat::writable(&mut tx, cid).await?;
    crate::crypto::plaintext(&mut tx, cid).await?;
    ensure(&mut tx, cid).await?;
    let iid = if let Some(iid) = receipt(&mut tx, p.id, cid, &v, "create").await? {
        iid
    } else {
        let ai = uid(&v, "aiId")?;
        let content = v["content"]
            .as_str()
            .filter(|s| !s.trim().is_empty() && s.chars().count() <= 4000)
            .ok_or_else(|| AppError::bad("约定内容须为1至4000字"))?;
        let due = time(&v, "dueAt")?;
        let expires = time(&v, "expiresAt")?;
        let offset = v["utcOffsetMinutes"]
            .as_i64()
            .filter(|n| (-840..=840).contains(n))
            .ok_or_else(|| AppError::bad("时区偏移无效"))?;
        let now = Utc::now();
        if due <= now
            || due > now + Duration::days(366)
            || expires <= due
            || expires > due + Duration::days(7)
        {
            return Err(AppError::bad("约定时间范围无效"));
        }
        let enabled: bool =
            sqlx::query_scalar("SELECT enabled FROM cloud_participation WHERE conversation=$1")
                .bind(cid)
                .fetch_one(&mut *tx)
                .await?;
        if !enabled || !contact_allowed(&mut tx, cid, p.id, ai).await? {
            return Err(AppError::forbidden());
        }
        let count:i64=sqlx::query_scalar("SELECT count(*) FROM cloud_contact_intents WHERE conversation=$1 AND status IN ('pending','claimed','executing')").bind(cid).fetch_one(&mut *tx).await?;
        if count >= 64 {
            return Err(AppError::conflict("当前窗口未完成约定已达上限"));
        }
        let iid = Uuid::new_v4();
        sqlx::query("INSERT INTO cloud_contact_intents(id,conversation,creator_id,ai_id,content,due_at,expires_at,utc_offset_minutes) VALUES($1,$2,$3,$4,$5,$6,$7,$8)").bind(iid).bind(cid).bind(p.id).bind(ai).bind(content).bind(due).bind(expires).bind(offset as i32).execute(&mut *tx).await?;
        save(&mut tx, p.id, cid, &v, "create", iid).await?;
        changed(&mut tx, cid).await?;
        iid
    };
    let row = sqlx::query("SELECT * FROM cloud_contact_intents WHERE id=$1")
        .bind(iid)
        .fetch_one(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(Json(intent(&row)))
}
async fn cancel(
    State(s): State<Arc<AppState>>,
    h: HeaderMap,
    Path((cid, iid)): Path<(Uuid, Uuid)>,
    Json(v): Json<Value>,
) -> Result<Json<Value>> {
    let p = auth::authenticate(&s, &h).await?;
    let mut tx = s.pool.begin().await?;
    nodes::lock(&mut tx).await?;
    let manage = permission(&mut tx, cid, p.id).await?;
    chat::writable(&mut tx, cid).await?;
    let row = sqlx::query("SELECT * FROM cloud_contact_intents WHERE id=$1 AND conversation=$2")
        .bind(iid)
        .bind(cid)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(AppError::not_found)?;
    if !manage && row.get::<Uuid, _>("creator_id") != p.id {
        return Err(AppError::forbidden());
    }
    let op = format!("cancel:{iid}");
    if receipt(&mut tx, p.id, cid, &v, &op).await?.is_none() {
        revision(&v, row.get("revision"))?;
        cancel_intents(&mut tx, cid, Some(iid), "cancelled").await?;
        save(&mut tx, p.id, cid, &v, &op, iid).await?;
        changed(&mut tx, cid).await?;
    }
    let row = sqlx::query("SELECT * FROM cloud_contact_intents WHERE id=$1")
        .bind(iid)
        .fetch_one(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(Json(intent(&row)))
}
pub(crate) async fn cancel_intents(
    tx: &mut Tx<'_>,
    cid: Uuid,
    iid: Option<Uuid>,
    reason: &str,
) -> Result<()> {
    let rows=sqlx::query("SELECT t.* FROM cloud_node_tasks t JOIN cloud_contact_intents i ON i.id=t.contact_intent_id WHERE i.conversation=$1 AND ($2::uuid IS NULL OR i.id=$2) AND t.state IN ('queued','running','uncertain')").bind(cid).bind(iid).fetch_all(&mut **tx).await?;
    nodes::invalidate_rows(tx, rows, reason).await?;
    sqlx::query("UPDATE cloud_contact_intents SET status='cancelled',reason=$3,revision=revision+1 WHERE conversation=$1 AND ($2::uuid IS NULL OR id=$2) AND status IN ('pending','claimed','executing')").bind(cid).bind(iid).bind(reason).execute(&mut **tx).await?;
    Ok(())
}
/// Start and result both recheck the independent authorization of a contact.
pub(crate) async fn validate_task(tx: &mut Tx<'_>, task: &sqlx::postgres::PgRow) -> Result<()> {
    let cid = task.get::<Uuid, _>("conversation");
    ensure(tx, cid).await?;
    let p = sqlx::query("SELECT * FROM cloud_participation WHERE conversation=$1")
        .bind(cid)
        .fetch_one(&mut **tx)
        .await?;
    if p.get::<bool, _>("paused")
        || p.get::<Vec<Uuid>, _>("paused_ai")
            .contains(&task.get("ai_id"))
    {
        return Err(AppError::conflict("窗口或目标 AI 已暂停"));
    }
    if let Some(iid) = task.get::<Option<Uuid>, _>("contact_intent_id") {
        let i = sqlx::query("SELECT * FROM cloud_contact_intents WHERE id=$1")
            .bind(iid)
            .fetch_one(&mut **tx)
            .await?;
        if task.get::<String, _>("state") == "queued"
            && p.get::<Option<DateTime<Utc>>, _>("quiet_until")
                .is_some_and(|t| t > Utc::now())
        {
            let mut error = AppError::conflict("约定等待免打扰结束");
            error.code = "task_not_ready";
            return Err(error);
        }
        if !p.get::<bool, _>("enabled")
            || !matches!(
                i.get::<String, _>("status").as_str(),
                "claimed" | "executing"
            ) && !(task.get::<String, _>("state") == "uncertain"
                && i.get::<String, _>("status") == "uncertain")
            || i.get::<DateTime<Utc>, _>("expires_at") <= Utc::now()
            || p.get::<Option<DateTime<Utc>>, _>("quiet_until")
                .is_some_and(|t| t > Utc::now())
            || !contact_allowed(tx, cid, i.get("creator_id"), i.get("ai_id")).await?
        {
            return Err(AppError::conflict("约定授权已变化"));
        }
    }
    Ok(())
}
pub(crate) async fn reserve(tx: &mut Tx<'_>, task: &sqlx::postgres::PgRow) -> Result<()> {
    validate_task(tx, task).await?;
    if !task.get::<bool, _>("automatic") {
        return Ok(());
    }
    let cid = task.get::<Uuid, _>("conversation");
    let p = sqlx::query("SELECT * FROM cloud_participation WHERE conversation=$1")
        .bind(cid)
        .fetch_one(&mut **tx)
        .await?;
    let policy: Value = p.get("policy");
    let contact = task.get::<Option<Uuid>, _>("contact_intent_id");
    if contact.is_none()
        && (policy["mode"] != "limitedAuto"
            || task.get::<Option<i64>, _>("policy_revision") != Some(p.get("revision")))
    {
        return Err(AppError::conflict("自动接话规则已变化"));
    }
    let day:chrono::NaiveDate=sqlx::query_scalar("UPDATE cloud_participation SET budget_day=greatest(budget_day,(now() AT TIME ZONE 'UTC')::date),bout_id=coalesce(bout_id,$2) WHERE conversation=$1 RETURNING budget_day").bind(cid).bind(Uuid::new_v4()).fetch_one(&mut **tx).await?;
    sqlx::query("INSERT INTO cloud_participation_daily(conversation,day) VALUES($1,$2) ON CONFLICT DO NOTHING").bind(cid).bind(day).execute(&mut **tx).await?;
    let calls: i32 = sqlx::query_scalar(
        "SELECT calls FROM cloud_participation_daily WHERE conversation=$1 AND day=$2",
    )
    .bind(cid)
    .bind(day)
    .fetch_one(&mut **tx)
    .await?;
    if p.get::<i32, _>("consecutive_ai") >= policy["maxConsecutiveAi"].as_i64().unwrap() as i32
        || p.get::<i32, _>("bout_calls") >= policy["maxBoutCalls"].as_i64().unwrap() as i32
        || calls >= policy["maxDailyCalls"].as_i64().unwrap() as i32
    {
        return Err(AppError::conflict("自动调用额度已用尽"));
    }
    sqlx::query(
        "UPDATE cloud_participation_daily SET calls=calls+1 WHERE conversation=$1 AND day=$2",
    )
    .bind(cid)
    .bind(day)
    .execute(&mut **tx)
    .await?;
    sqlx::query("UPDATE cloud_participation SET bout_calls=bout_calls+1 WHERE conversation=$1")
        .bind(cid)
        .execute(&mut **tx)
        .await?;
    if let Some(iid) = contact {
        sqlx::query(
            "UPDATE cloud_contact_intents SET status='executing',revision=revision+1 WHERE id=$1",
        )
        .bind(iid)
        .execute(&mut **tx)
        .await?;
    }
    changed(tx, cid).await?;
    Ok(())
}
/// All task state changes also drive the contact state in the same transaction.
pub(crate) async fn task_changed(tx: &mut Tx<'_>, r: &sqlx::postgres::PgRow) -> Result<()> {
    if let Some(iid) = r.get::<Option<Uuid>, _>("contact_intent_id") {
        let state = r.get::<String, _>("state");
        let status = match state.as_str() {
            "completed" => "delivered",
            "failed" => "failed",
            "cancelled" => "cancelled",
            "expired" => "expired",
            "uncertain" | "interrupted" => "uncertain",
            _ => return Ok(()),
        };
        sqlx::query("UPDATE cloud_contact_intents SET status=CASE WHEN expires_at<=now() AND $2='uncertain' THEN 'expired' ELSE $2 END,reason=$3,revision=revision+1 WHERE id=$1 AND status IN ('pending','claimed','executing','uncertain')").bind(iid).bind(status).bind(r.get::<Option<String>,_>("error_code")).execute(&mut **tx).await?;
    }
    Ok(())
}
pub(crate) async fn completed(tx: &mut Tx<'_>, r: &sqlx::postgres::PgRow) -> Result<()> {
    let cid = r.get::<Uuid, _>("conversation");
    ensure(tx, cid).await?;
    sqlx::query("UPDATE cloud_participation SET consecutive_ai=consecutive_ai+1,last_ai=$2,last_activity=greatest(last_activity,now()) WHERE conversation=$1").bind(cid).bind(r.get::<Uuid,_>("ai_id")).execute(&mut **tx).await?;
    if r.get::<Option<Uuid>, _>("contact_intent_id").is_none() {
        automatic(tx, cid, r.get("message_id"), Utc::now()).await?;
    }
    Ok(())
}
pub(crate) async fn automatic(
    tx: &mut Tx<'_>,
    cid: Uuid,
    mid: Uuid,
    now: DateTime<Utc>,
) -> Result<()> {
    let p=sqlx::query("SELECT p.*,c.kind,coalesce(d.calls,0) AS calls FROM cloud_participation p JOIN cloud_conversations c ON c.id=p.conversation LEFT JOIN cloud_participation_daily d ON d.conversation=p.conversation AND d.day=p.budget_day WHERE p.conversation=$1").bind(cid).fetch_one(&mut **tx).await?;
    let policy: Value = p.get("policy");
    if p.get::<String, _>("kind") != "group"
        || p.get::<Option<Uuid>, _>("bout_id").is_none()
        || p.get::<bool, _>("paused")
        || policy["mode"] != "limitedAuto"
    {
        return Ok(());
    }
    let reason = if p.get::<i32, _>("consecutive_ai")
        >= policy["maxConsecutiveAi"].as_i64().unwrap() as i32
        || p.get::<i32, _>("bout_calls") >= policy["maxBoutCalls"].as_i64().unwrap() as i32
    {
        Some("turn_limit")
    } else if p.get::<i32, _>("calls") >= policy["maxDailyCalls"].as_i64().unwrap() as i32 {
        Some("daily_limit")
    } else {
        None
    };
    if let Some(reason) = reason {
        sqlx::query("UPDATE cloud_participation SET reason=$2 WHERE conversation=$1")
            .bind(cid)
            .bind(reason)
            .execute(&mut **tx)
            .await?;
        return Ok(());
    }
    let busy:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM cloud_node_tasks WHERE conversation=$1 AND (state='queued' AND automatic AND contact_intent_id IS NULL OR state IN ('queued','running') AND (contact_intent_id IS NULL OR state='running' OR not_before<=now()) AND EXISTS(SELECT 1 FROM cloud_messages WHERE id=$2 AND sender IN (SELECT id FROM accounts WHERE kind='ai'))))").bind(cid).bind(mid).fetch_one(&mut **tx).await?;
    if busy {
        return Ok(());
    }
    let running_ai: Option<Uuid> = sqlx::query_scalar(
        "SELECT ai_id FROM cloud_node_tasks WHERE conversation=$1 AND state='running'",
    )
    .bind(cid)
    .fetch_optional(&mut **tx)
    .await?;
    let cursor_ai = running_ai.or(p.get::<Option<Uuid>, _>("last_ai"));
    let candidate=sqlx::query("SELECT a.id,b.version,c.revision FROM cloud_members m JOIN accounts a ON a.id=m.account_id JOIN cloud_ai_bindings b ON b.ai_id=a.id JOIN cloud_conversations c ON c.id=m.conversation WHERE m.conversation=$1 AND a.kind='ai' AND a.status='active' AND b.handover_state='active' AND NOT (a.id=ANY($2)) AND ($3::uuid IS NULL OR a.id<>$3) ORDER BY CASE WHEN a.id>$3 THEN 0 ELSE 1 END,a.id LIMIT 1").bind(cid).bind(p.get::<Vec<Uuid>,_>("paused_ai")).bind(cursor_ai).fetch_optional(&mut **tx).await?;
    if let Some(a) = candidate {
        let tid = Uuid::new_v4();
        let inserted=sqlx::query("INSERT INTO cloud_node_tasks(id,ai_id,conversation,input_id,input_revision,binding_version,expires_at,conversation_revision,automatic,not_before,policy_revision,input_ids,trigger) SELECT $1,$2,$3,$4,revision,$5,$6,$7,true,$8,$9,ARRAY[$4],jsonb_build_object('type','message','messageId',$4::text,'inputIds',ARRAY[$4]) FROM cloud_messages WHERE id=$4 ON CONFLICT(ai_id,input_id) DO NOTHING").bind(tid).bind(a.get::<Uuid,_>("id")).bind(cid).bind(mid).bind(a.get::<i64,_>("version")).bind(now+Duration::minutes(5)).bind(a.get::<i64,_>("revision")).bind(now+Duration::milliseconds(800)).bind(p.get::<i64,_>("revision")).execute(&mut **tx).await?;
        if inserted.rows_affected() == 1 {
            nodes::publish(tx, tid).await?;
        }
    } else {
        sqlx::query("UPDATE cloud_participation SET reason='no_candidate' WHERE conversation=$1")
            .bind(cid)
            .execute(&mut **tx)
            .await?;
    }
    Ok(())
}
/// Timestamp injection keeps due/expiry tests deterministic without long sleeps.
pub async fn tick(pool: &sqlx::PgPool, now: DateTime<Utc>) -> Result<()> {
    let mut tx = pool.begin().await?;
    nodes::lock(&mut tx).await?;
    sqlx::query("UPDATE cloud_participation SET budget_day=$1 WHERE budget_day<$1")
        .bind(now.date_naive())
        .execute(&mut *tx)
        .await?;
    let idle=sqlx::query("UPDATE cloud_participation p SET bout_id=NULL,bout_calls=0,consecutive_ai=0,reason='idle' WHERE bout_id IS NOT NULL AND last_activity+((policy->>'idleSeconds')::integer*interval '1 second')<=$1 AND NOT EXISTS(SELECT 1 FROM cloud_node_tasks WHERE conversation=p.conversation AND state IN ('queued','running')) RETURNING conversation").bind(now).fetch_all(&mut *tx).await?;
    for c in idle {
        changed(&mut tx, c.get("conversation")).await?;
    }
    let expired=sqlx::query("UPDATE cloud_node_tasks SET state='expired',revision=revision+1,error_code='expired',finished_at=$1 WHERE state IN ('queued','running') AND expires_at<=$1 RETURNING *").bind(now).fetch_all(&mut *tx).await?;
    for r in expired {
        if r.get::<Option<Uuid>, _>("message_id").is_some() {
            nodes::finish_message(
                &mut tx,
                &r,
                &r.get::<String, _>("delta_text"),
                "interrupted",
            )
            .await?;
        }
        nodes::publish(&mut tx, r.get("id")).await?;
    }
    let invalid=sqlx::query("SELECT i.* FROM cloud_contact_intents i JOIN cloud_participation p ON p.conversation=i.conversation WHERE (i.status='pending' AND i.due_at<=$1 OR i.status IN ('claimed','executing') AND (i.expires_at<=$1 OR NOT p.enabled OR p.paused OR i.ai_id=ANY(p.paused_ai) OR i.status='claimed' AND p.quiet_until>$1)) ORDER BY i.due_at,i.id").bind(now).fetch_all(&mut *tx).await?;
    for i in invalid {
        let cid = i.get::<Uuid, _>("conversation");
        let iid = i.get::<Uuid, _>("id");
        let p = sqlx::query("SELECT * FROM cloud_participation WHERE conversation=$1")
            .bind(cid)
            .fetch_one(&mut *tx)
            .await?;
        let active = contact_allowed(&mut tx, cid, i.get("creator_id"), i.get("ai_id")).await?;
        let status = i.get::<String, _>("status");
        let expires = i.get::<DateTime<Utc>, _>("expires_at");
        if !active || !p.get::<bool, _>("enabled") || expires <= now {
            cancel_intents(
                &mut tx,
                cid,
                Some(iid),
                if expires <= now {
                    "expired"
                } else {
                    "authorization_changed"
                },
            )
            .await?;
            if expires <= now {
                sqlx::query("UPDATE cloud_contact_intents SET status='expired',reason='expired',revision=revision+1 WHERE id=$1").bind(iid).execute(&mut *tx).await?;
            }
            changed(&mut tx, cid).await?;
            continue;
        }
        let due = i.get::<DateTime<Utc>, _>("due_at");
        let draining: bool = sqlx::query_scalar(
            "SELECT handover_state<>'active' FROM cloud_ai_bindings WHERE ai_id=$1",
        )
        .bind(i.get::<Uuid, _>("ai_id"))
        .fetch_one(&mut *tx)
        .await?;
        let paused = draining
            || p.get::<bool, _>("paused")
            || p.get::<Vec<Uuid>, _>("paused_ai").contains(&i.get("ai_id"));
        let quiet = p
            .get::<Option<DateTime<Utc>>, _>("quiet_until")
            .is_some_and(|t| t > now);
        if status == "pending" && due <= now {
            if paused || quiet && p.get::<String, _>("quiet_action") == "skip" {
                sqlx::query("UPDATE cloud_contact_intents SET status='skipped',reason=$2,revision=revision+1 WHERE id=$1").bind(iid).bind(if paused{"paused"}else{"quiet"}).execute(&mut *tx).await?;
                changed(&mut tx, cid).await?;
                continue;
            }
            if quiet {
                continue;
            }
            let a=sqlx::query("SELECT b.version,c.revision FROM cloud_ai_bindings b CROSS JOIN cloud_conversations c WHERE b.ai_id=$1 AND c.id=$2").bind(i.get::<Uuid,_>("ai_id")).bind(cid).fetch_one(&mut *tx).await?;
            let tid = Uuid::new_v4();
            sqlx::query("INSERT INTO cloud_node_tasks(id,ai_id,conversation,contact_intent_id,binding_version,expires_at,conversation_revision,automatic,trigger) VALUES($1,$2,$3,$4,$5,$6,$7,true,$8)").bind(tid).bind(i.get::<Uuid,_>("ai_id")).bind(cid).bind(iid).bind(a.get::<i64,_>("version")).bind(expires).bind(a.get::<i64,_>("revision")).bind(json!({"type":"contact","intentId":iid,"revision":(i.get::<i64,_>("revision")+1).to_string(),"creatorId":i.get::<Uuid,_>("creator_id"),"content":i.get::<String,_>("content"),"dueAt":due})).execute(&mut *tx).await?;
            sqlx::query("UPDATE cloud_contact_intents SET status='claimed',task_id=$2,revision=revision+1 WHERE id=$1").bind(iid).bind(tid).execute(&mut *tx).await?;
            nodes::publish(&mut tx, tid).await?;
        } else if status == "claimed" {
            if paused {
                cancel_intents(&mut tx, cid, Some(iid), "paused").await?;
            } else if quiet {
                if p.get::<String, _>("quiet_action") == "skip" {
                    cancel_intents(&mut tx, cid, Some(iid), "quiet").await?;
                    sqlx::query("UPDATE cloud_contact_intents SET status='skipped' WHERE id=$1")
                        .bind(iid)
                        .execute(&mut *tx)
                        .await?;
                } else {
                    sqlx::query("UPDATE cloud_node_tasks SET ready_notified=false,not_before=$2 WHERE contact_intent_id=$1 AND state='queued'").bind(iid).bind(p.get::<Option<DateTime<Utc>>,_>("quiet_until")).execute(&mut *tx).await?;
                }
            }
        }
    }
    let denied=sqlx::query("SELECT t.*,CASE WHEN p.consecutive_ai>=(p.policy->>'maxConsecutiveAi')::integer OR p.bout_calls>=(p.policy->>'maxBoutCalls')::integer THEN 'turn_limit' ELSE 'daily_limit' END AS quota_reason FROM cloud_node_tasks t JOIN cloud_participation p ON p.conversation=t.conversation LEFT JOIN cloud_participation_daily d ON d.conversation=p.conversation AND d.day=p.budget_day WHERE t.state='queued' AND t.automatic AND t.not_before<=$1 AND (p.consecutive_ai>=(p.policy->>'maxConsecutiveAi')::integer OR p.bout_calls>=(p.policy->>'maxBoutCalls')::integer OR coalesce(d.calls,0)>=(p.policy->>'maxDailyCalls')::integer)").bind(now).fetch_all(&mut *tx).await?;
    for r in denied {
        let tid: Uuid = r.get("id");
        sqlx::query("UPDATE cloud_node_tasks SET state='failed',error_code='automatic_quota',revision=revision+1,finished_at=$2 WHERE id=$1").bind(tid).bind(now).execute(&mut *tx).await?;
        sqlx::query("UPDATE cloud_participation SET reason=$2 WHERE conversation=$1")
            .bind(r.get::<Uuid, _>("conversation"))
            .bind(r.get::<String, _>("quota_reason"))
            .execute(&mut *tx)
            .await?;
        nodes::publish(&mut tx, tid).await?;
    }
    let ready=sqlx::query("UPDATE cloud_node_tasks SET ready_notified=true WHERE state='queued' AND NOT ready_notified AND not_before<=$1 RETURNING id").bind(now).fetch_all(&mut *tx).await?;
    for r in ready {
        nodes::publish(&mut tx, r.get("id")).await?;
    }
    tx.commit().await?;
    Ok(())
}
pub(crate) async fn reconcile(tx: &mut Tx<'_>) -> Result<()> {
    let rows = sqlx::query(
        "SELECT * FROM cloud_contact_intents i WHERE status IN ('pending','claimed','executing') OR status='uncertain' AND EXISTS(SELECT 1 FROM cloud_node_tasks t WHERE t.contact_intent_id=i.id AND t.state='uncertain')",
    )
    .fetch_all(&mut **tx)
    .await?;
    for i in rows {
        let cid = i.get("conversation");
        if !contact_allowed(tx, cid, i.get("creator_id"), i.get("ai_id")).await? {
            cancel_intents(tx, cid, Some(i.get("id")), "authorization_changed").await?;
            changed(tx, cid).await?;
        }
    }
    Ok(())
}
