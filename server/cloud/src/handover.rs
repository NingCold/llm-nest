//! Explicit draining before a normal encrypted node handover.
use crate::{AppError, AppState, Result, auth, crypto, db as sqlx, nodes};
use axum::{
    Json, Router,
    extract::{Path, State},
    http::HeaderMap,
    routing::post,
};
use serde_json::{Value, json};
use sqlx::Row;
use std::sync::Arc;
use uuid::Uuid;
type Tx<'a> = sqlx::Transaction<'a, sqlx::Postgres>;
pub(crate) fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/ai/{id}/node/handover", post(begin))
        .route("/ai/{id}/node/handover/cancel", post(cancel))
        .route("/node/handover-ready", post(ready))
}
pub(crate) async fn encrypted(tx: &mut Tx<'_>, ai: Uuid) -> Result<bool> {
    Ok(sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM cloud_members m JOIN cloud_conversations c ON c.id=m.conversation WHERE m.account_id=$1 AND c.crypto_enabled) OR EXISTS(SELECT 1 FROM cloud_crypto_resources WHERE ai_id=$1)").bind(ai).fetch_one(&mut **tx).await?)
}
pub(crate) async fn active(tx: &mut Tx<'_>, ai: Uuid) -> Result<()> {
    let state: String =
        sqlx::query_scalar("SELECT handover_state FROM cloud_ai_bindings WHERE ai_id=$1")
            .bind(ai)
            .fetch_one(&mut **tx)
            .await?;
    if state != "active" {
        let mut e = AppError::conflict("节点正在移交，不接受新运行");
        e.code = "node_draining";
        return Err(e);
    }
    Ok(())
}
pub(crate) async fn changed(tx: &mut Tx<'_>, ai: Uuid) -> Result<()> {
    sqlx::query("UPDATE cloud_ai_bindings SET handover_state='draining' WHERE ai_id=$1 AND handover_state='ready'").bind(ai).execute(&mut **tx).await?;
    Ok(())
}
async fn begin(
    State(s): State<Arc<AppState>>,
    h: HeaderMap,
    Path(ai): Path<Uuid>,
    Json(v): Json<Value>,
) -> Result<Json<Value>> {
    control(s, h, ai, v, false).await
}
async fn cancel(
    State(s): State<Arc<AppState>>,
    h: HeaderMap,
    Path(ai): Path<Uuid>,
    Json(v): Json<Value>,
) -> Result<Json<Value>> {
    control(s, h, ai, v, true).await
}
async fn control(
    s: Arc<AppState>,
    h: HeaderMap,
    ai: Uuid,
    v: Value,
    cancel: bool,
) -> Result<Json<Value>> {
    let p = auth::authenticate(&s, &h).await?;
    let mut tx = s.pool.begin().await?;
    nodes::lock(&mut tx).await?;
    nodes::admin(&mut tx, ai, p.id).await?;
    let op = if cancel {
        "handover_cancel"
    } else {
        "handover"
    };
    if nodes::admin_receipt(&mut tx, p.id, &v, op, ai)
        .await?
        .is_some()
    {
        return Ok(Json(nodes::view(&mut tx, ai, p.id).await?));
    }
    let r =
        sqlx::query("SELECT version,node_id,handover_state FROM cloud_ai_bindings WHERE ai_id=$1")
            .bind(ai)
            .fetch_one(&mut *tx)
            .await?;
    if r.get::<i64, _>("version").to_string() != crypto::field(&v, "expectedBindingVersion")?
        || r.get::<Option<Uuid>, _>("node_id").is_none()
    {
        return Err(AppError::conflict("运行节点已变化"));
    }
    if !cancel || r.get::<String, _>("handover_state") != "active" {
        let rows = sqlx::query("SELECT * FROM cloud_node_tasks WHERE ai_id=$1 AND state='queued'")
            .bind(ai)
            .fetch_all(&mut *tx)
            .await?;
        nodes::invalidate_rows(&mut tx, rows, "node_handover").await?;
    }
    sqlx::query("UPDATE cloud_ai_bindings SET handover_state=$2 WHERE ai_id=$1")
        .bind(ai)
        .bind(if cancel { "active" } else { "draining" })
        .execute(&mut *tx)
        .await?;
    let out = nodes::view(&mut tx, ai, p.id).await?;
    nodes::save_admin_receipt(&mut tx, p.id, &v, op, ai, &json!({})).await?;
    tx.commit().await?;
    Ok(Json(out))
}
async fn ready(
    State(s): State<Arc<AppState>>,
    h: HeaderMap,
    Json(v): Json<Value>,
) -> Result<Json<Value>> {
    let mut tx = s.pool.begin().await?;
    nodes::lock(&mut tx).await?;
    let a = crypto::actor(&s, &h, &mut tx).await?;
    let n = a.node.as_ref().ok_or_else(AppError::forbidden)?;
    crypto::device(&mut tx, &a, crypto::field(&v, "deviceId")?).await?;
    if n.version.to_string() != crypto::field(&v, "expectedBindingVersion")? {
        return Err(AppError::conflict("节点绑定已变化"));
    }
    let state: String =
        sqlx::query_scalar("SELECT handover_state FROM cloud_ai_bindings WHERE ai_id=$1")
            .bind(n.ai)
            .fetch_one(&mut *tx)
            .await?;
    if state == "active" {
        return Err(AppError::conflict("尚未开始节点移交"));
    }
    let unfinished:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM cloud_node_tasks WHERE ai_id=$1 AND state IN ('queued','running','uncertain')) OR EXISTS(SELECT 1 FROM cloud_summary_calls WHERE ai_id=$1 AND state IN ('running','uncertain'))").bind(n.ai).fetch_one(&mut *tx).await?;
    if unfinished {
        return Err(AppError::conflict("节点仍有未结束调用，不能声明移交完成"));
    }
    let pending:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM cloud_crypto_resources r WHERE r.ai_id=$1 AND NOT EXISTS(SELECT 1 FROM cloud_crypto_applied a WHERE a.ai_id=r.ai_id AND a.scope=r.scope AND a.item_id=r.item_id AND a.revision=r.revision AND a.node_id=$2 AND a.binding_version=$3 AND a.status='applied'))").bind(n.ai).bind(n.node).bind(n.version).fetch_one(&mut *tx).await?;
    if pending {
        return Err(AppError::conflict("当前资料尚未全部应用"));
    }
    if encrypted(&mut tx, n.ai).await? {
        let latest:Option<Uuid>=sqlx::query_scalar("SELECT version FROM cloud_crypto_backups WHERE account_id=$1 ORDER BY created_at DESC LIMIT 1").bind(n.ai).fetch_optional(&mut *tx).await?;
        if latest.map(|v| v.to_string()).as_deref()
            != v.get("backupVersion").and_then(Value::as_str)
            || latest.is_none()
        {
            return Err(AppError::conflict("历史密钥尚未确认备份到当前版本"));
        }
    }
    sqlx::query("UPDATE cloud_ai_bindings SET handover_state='ready' WHERE ai_id=$1")
        .bind(n.ai)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(Json(json!({"handoverState":"ready"})))
}
