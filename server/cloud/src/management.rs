//! Human-authorized AI administration and scoped read-only account views.
use crate::{AppError, AppState, Result, auth, chat, db as sqlx, nodes};
use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::HeaderMap,
    response::Response,
    routing::{get, post, put},
};
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::{Postgres, Row, Transaction};
use std::sync::Arc;
use uuid::Uuid;
type Tx<'a> = Transaction<'a, Postgres>;
pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/ai/{id}/profile", put(profile))
        .route("/ai/{id}/administrators", get(administrators))
        .route("/ai/{id}/administrators/{target}", put(administrator))
        .route("/ai/{id}/view", get(account_view))
        .route("/ai/{id}/view/conversations/{cid}/messages", get(messages))
        .route(
            "/ai/{id}/view/conversations/{cid}/messages/{mid}/attachments/{index}",
            get(attachment),
        )
        .route(
            "/ai/{id}/management",
            post(crate::consciousness::manage_profile),
        )
}
fn revision(v: &Value, field: &str) -> Result<i64> {
    v[field]
        .as_str()
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| AppError::bad("版本无效"))
}
async fn changed(tx: &mut Tx<'_>, ai: Uuid) -> Result<()> {
    sqlx::query(
        "INSERT INTO cloud_events(owner) SELECT administrator FROM cloud_ai_admins WHERE ai_id=$1",
    )
    .bind(ai)
    .execute(&mut **tx)
    .await?;
    let ids: Vec<Uuid> =
        sqlx::query_scalar("SELECT conversation FROM cloud_members WHERE account_id=$1")
            .bind(ai)
            .fetch_all(&mut **tx)
            .await?;
    for cid in ids {
        let members = chat::member(tx, cid, ai).await?;
        chat::notify(tx, cid, members).await?;
    }
    Ok(())
}
async fn profile(
    State(s): State<Arc<AppState>>,
    h: HeaderMap,
    Path(ai): Path<Uuid>,
    Json(v): Json<Value>,
) -> Result<Json<Value>> {
    let p = auth::authenticate(&s, &h).await?;
    let name = v["name"]
        .as_str()
        .ok_or_else(|| AppError::bad("名称无效"))?
        .trim();
    let bio = v["bio"].as_str().ok_or_else(|| AppError::bad("简介无效"))?;
    if name.is_empty() || name.chars().count() > 80 || bio.len() > 4096 {
        return Err(AppError::bad("账号资料超出限制"));
    }
    let mut tx = s.pool.begin().await?;
    nodes::lock(&mut tx).await?;
    nodes::admin(&mut tx, ai, p.id).await?;
    if nodes::admin_receipt(&mut tx, p.id, &v, "profile", ai)
        .await?
        .is_some()
    {
        return Ok(Json(nodes::view(&mut tx, ai, p.id).await?));
    }
    let result=sqlx::query("UPDATE accounts SET name=$2,bio=$3,profile_revision=profile_revision+1 WHERE id=$1 AND profile_revision=$4").bind(ai).bind(name).bind(bio).bind(revision(&v,"expectedProfileRevision")?).execute(&mut *tx).await?;
    if result.rows_affected() == 0 {
        return Err(AppError::conflict("账号资料版本已变化"));
    }
    changed(&mut tx, ai).await?;
    let result = nodes::view(&mut tx, ai, p.id).await?;
    nodes::save_admin_receipt(&mut tx, p.id, &v, "profile", ai, &json!({})).await?;
    tx.commit().await?;
    Ok(Json(result))
}
async fn administrator_view(tx: &mut Tx<'_>, ai: Uuid) -> Result<Value> {
    let revision: i64 =
        sqlx::query_scalar("SELECT administrators_revision FROM accounts WHERE id=$1")
            .bind(ai)
            .fetch_one(&mut **tx)
            .await?;
    let rows=sqlx::query("SELECT administrator,initial FROM cloud_ai_admins WHERE ai_id=$1 ORDER BY initial DESC,administrator").bind(ai).fetch_all(&mut **tx).await?;
    let mut administrators = vec![];
    for row in rows {
        administrators.push(json!({"account":chat::account(tx,row.get("administrator")).await?,"role":if row.get::<bool,_>("initial"){"initial"}else{"admin"}}));
    }
    Ok(json!({"revision":revision.to_string(),"administrators":administrators}))
}
async fn administrators(
    State(s): State<Arc<AppState>>,
    h: HeaderMap,
    Path(ai): Path<Uuid>,
) -> Result<Json<Value>> {
    let p = auth::authenticate(&s, &h).await?;
    let mut tx = s.pool.begin().await?;
    nodes::admin(&mut tx, ai, p.id).await?;
    Ok(Json(administrator_view(&mut tx, ai).await?))
}
async fn administrator(
    State(s): State<Arc<AppState>>,
    h: HeaderMap,
    Path((ai, target)): Path<(Uuid, Uuid)>,
    Json(v): Json<Value>,
) -> Result<Json<Value>> {
    let p = auth::authenticate(&s, &h).await?;
    let mut tx = s.pool.begin().await?;
    nodes::lock(&mut tx).await?;
    let initial:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM cloud_ai_admins WHERE ai_id=$1 AND administrator=$2 AND initial)").bind(ai).bind(p.id).fetch_one(&mut *tx).await?;
    if !initial {
        return Err(AppError::forbidden());
    }
    let operation = format!("administrator:{target}");
    if nodes::admin_receipt(&mut tx, p.id, &v, &operation, ai)
        .await?
        .is_some()
    {
        return Ok(Json(administrator_view(&mut tx, ai).await?));
    }
    let enabled = v["enabled"]
        .as_bool()
        .ok_or_else(|| AppError::bad("管理员设置无效"))?;
    if chat::account(&mut tx, target).await?["kind"] != "human" {
        return Err(AppError::bad("管理员必须是真人账号"));
    }
    let target_initial:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM cloud_ai_admins WHERE ai_id=$1 AND administrator=$2 AND initial)").bind(ai).bind(target).fetch_one(&mut *tx).await?;
    if target_initial {
        return Err(AppError::bad("不能修改初始管理员"));
    }
    let result=sqlx::query("UPDATE accounts SET administrators_revision=administrators_revision+1 WHERE id=$1 AND administrators_revision=$2").bind(ai).bind(revision(&v,"expectedRevision")?).execute(&mut *tx).await?;
    if result.rows_affected() == 0 {
        return Err(AppError::conflict("管理员版本已变化"));
    }
    if enabled {
        sqlx::query(
            "INSERT INTO cloud_ai_admins(ai_id,administrator) VALUES($1,$2) ON CONFLICT DO NOTHING",
        )
        .bind(ai)
        .bind(target)
        .execute(&mut *tx)
        .await?;
    } else {
        sqlx::query("DELETE FROM cloud_ai_admins WHERE ai_id=$1 AND administrator=$2")
            .bind(ai)
            .bind(target)
            .execute(&mut *tx)
            .await?;
    }
    changed(&mut tx, ai).await?;
    sqlx::query("INSERT INTO cloud_events(owner) VALUES($1)")
        .bind(target)
        .execute(&mut *tx)
        .await?;
    let result = administrator_view(&mut tx, ai).await?;
    nodes::save_admin_receipt(&mut tx, p.id, &v, &operation, ai, &json!({})).await?;
    tx.commit().await?;
    Ok(Json(result))
}
async fn account_view(
    State(s): State<Arc<AppState>>,
    h: HeaderMap,
    Path(ai): Path<Uuid>,
) -> Result<Json<Value>> {
    let p = auth::authenticate(&s, &h).await?;
    let mut tx = s.pool.begin().await?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ")
        .execute(&mut *tx)
        .await?;
    nodes::admin(&mut tx, ai, p.id).await?;
    let ids: Vec<Uuid> = sqlx::query_scalar(
        "SELECT conversation FROM cloud_members WHERE account_id=$1 ORDER BY conversation",
    )
    .bind(ai)
    .fetch_all(&mut *tx)
    .await?;
    let mut conversations = vec![];
    for cid in ids {
        conversations.push(chat::conversation(&mut tx, cid, ai).await?);
    }
    Ok(Json(
        json!({"account":chat::account(&mut tx,ai).await?,"conversations":conversations}),
    ))
}
#[derive(Deserialize)]
struct Q {
    before: Option<i64>,
    limit: Option<i64>,
    revision: Option<i64>,
    inline: Option<bool>,
}
async fn messages(
    State(s): State<Arc<AppState>>,
    h: HeaderMap,
    Path((ai, cid)): Path<(Uuid, Uuid)>,
    Query(q): Query<Q>,
) -> Result<Json<Value>> {
    let p = auth::authenticate(&s, &h).await?;
    let mut tx = s.pool.begin().await?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ")
        .execute(&mut *tx)
        .await?;
    nodes::admin(&mut tx, ai, p.id).await?;
    chat::member(&mut tx, cid, ai).await?;
    chat::read_messages(&mut tx, cid, q.before, q.limit).await
}
async fn attachment(
    State(s): State<Arc<AppState>>,
    h: HeaderMap,
    Path((ai, cid, mid, index)): Path<(Uuid, Uuid, Uuid, usize)>,
    Query(q): Query<Q>,
) -> Result<Response> {
    let p = auth::authenticate(&s, &h).await?;
    let mut tx = s.pool.begin().await?;
    nodes::lock(&mut tx).await?;
    nodes::admin(&mut tx, ai, p.id).await?;
    chat::member(&mut tx, cid, ai).await?;
    let row = sqlx::query("SELECT * FROM cloud_messages WHERE id=$1 AND conversation=$2 FOR SHARE")
        .bind(mid)
        .bind(cid)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(AppError::not_found)?;
    if q.revision != Some(row.get("revision")) || row.get::<bool, _>("withdrawn") {
        return Err(AppError::conflict("附件版本已变化"));
    }
    let parts: Value = row.get("attachments");
    let part = parts.get(index).ok_or_else(AppError::not_found)?;
    let inline = q.inline.unwrap_or(false);
    if inline && part["kind"] != "image" {
        return Err(AppError::bad("普通文件不可内联"));
    }
    let bytes = tokio::fs::read(
        crate::history_migration::attachment_path(&mut tx, &s.attachments, mid, index).await?,
    )
    .await?;
    let name = if part["kind"] == "image" {
        format!(
            "{mid}-{index}.{}",
            chat::image_extension(part["mime"].as_str().unwrap())
        )
    } else {
        social::attachment::file_name(part["name"].as_str())
    };
    Ok(chat::binary(
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
