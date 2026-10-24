//! Scoped encrypted AI material. Authorization and CAS remain server-owned.
use crate::{
    AppError, AppState, Result, chat,
    crypto::{self, Actor, Tx},
    db as sqlx, nodes,
};
use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::HeaderMap,
    routing::{get, post, put},
};
use connector::crypto::{ApplyResource, SaveResource};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::Row;
use std::sync::Arc;
use uuid::Uuid;
pub(crate) fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/crypto/resource-participants", get(participants))
        .route("/crypto/resources", get(list))
        .route("/crypto/resources/{id}", put(save))
        .route("/crypto/resources/{id}/applied", post(applied))
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Scope {
    ai_id: Uuid,
    conversation_id: Option<Uuid>,
    kind: String,
}
fn key(s: &Scope) -> String {
    format!(
        "{}:{}:{}",
        s.ai_id,
        s.kind,
        s.conversation_id
            .map_or_else(|| "global".into(), |id| id.to_string())
    )
}
async fn permission(tx: &mut Tx<'_>, a: &Actor, s: &Scope) -> Result<()> {
    if !matches!(
        s.kind.as_str(),
        "profile" | "memory" | "summary" | "summarySettings"
    ) {
        return Err(AppError::bad("资料类型无效"));
    }
    let ai: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM accounts WHERE id=$1 AND kind='ai' AND status='active')",
    )
    .bind(s.ai_id)
    .fetch_one(&mut **tx)
    .await?;
    if !ai {
        return Err(AppError::not_found());
    }
    if let Some(n) = &a.node
        && n.ai != s.ai_id
    {
        return Err(AppError::forbidden());
    }
    if s.kind == "profile" {
        if s.conversation_id.is_some() {
            return Err(AppError::bad("角色资料不归窗口"));
        }
        if a.node.is_none() {
            nodes::admin(tx, s.ai_id, a.id).await?;
        }
    } else {
        let cid = s
            .conversation_id
            .ok_or_else(|| AppError::bad("缺少资料窗口"))?;
        let members = chat::member(tx, cid, a.id).await?;
        if !members.contains(&s.ai_id) {
            return Err(AppError::forbidden());
        }
        if a.node.is_none() && s.kind != "memory" {
            nodes::admin(tx, s.ai_id, a.id).await?;
        }
    }
    Ok(())
}
fn scope(r: &sqlx::postgres::PgRow) -> Scope {
    Scope {
        ai_id: r.get("ai_id"),
        conversation_id: r.get("conversation"),
        kind: r.get("kind"),
    }
}
async fn view(tx: &mut Tx<'_>, r: &sqlx::postgres::PgRow) -> Result<Value> {
    let applied=sqlx::query("SELECT a.* FROM cloud_crypto_applied a JOIN cloud_ai_bindings b ON b.ai_id=a.ai_id AND b.node_id=a.node_id AND b.version=a.binding_version WHERE a.ai_id=$1 AND a.scope=$2 AND a.item_id=$3").bind(r.get::<Uuid,_>("ai_id")).bind(r.get::<String,_>("scope")).bind(r.get::<String,_>("item_id")).fetch_optional(&mut **tx).await?.map(|a|json!({"bindingVersion":a.get::<i64,_>("binding_version").to_string(),"revision":a.get::<i64,_>("revision").to_string(),"status":a.get::<String,_>("status")}));
    Ok(
        json!({"id":r.get::<String,_>("item_id"),"aiId":r.get::<Uuid,_>("ai_id"),"conversationId":r.get::<Option<Uuid>,_>("conversation"),"kind":r.get::<String,_>("kind"),"revision":r.get::<i64,_>("revision").to_string(),"sealed":r.get::<Option<Value>,_>("sealed"),"sources":r.get::<Value,_>("sources"),"tombstone":r.get::<Option<String>,_>("tombstone"),"applied":applied}),
    )
}
async fn audience(tx: &mut Tx<'_>, q: &Scope) -> Result<Value> {
    let mut ids: Vec<Uuid> = if q.kind == "profile" {
        sqlx::query_scalar(
            "SELECT administrator FROM cloud_ai_admins WHERE ai_id=$1 UNION SELECT $1::uuid",
        )
        .bind(q.ai_id)
        .fetch_all(&mut **tx)
        .await?
    } else if q.kind == "memory" {
        sqlx::query_scalar("SELECT account_id FROM cloud_members WHERE conversation=$1")
            .bind(q.conversation_id)
            .fetch_all(&mut **tx)
            .await?
    } else {
        sqlx::query_scalar("SELECT m.account_id FROM cloud_members m WHERE m.conversation=$1 AND (m.account_id=$2 OR EXISTS(SELECT 1 FROM cloud_ai_admins a WHERE a.ai_id=$2 AND a.administrator=m.account_id))").bind(q.conversation_id).bind(q.ai_id).fetch_all(&mut **tx).await?
    };
    ids.sort();
    let mut accounts = vec![];
    let mut identities = vec![];
    for id in ids {
        let row=sqlx::query("SELECT a.status,b.version FROM accounts a LEFT JOIN cloud_ai_bindings b ON b.ai_id=a.id WHERE a.id=$1").bind(id).fetch_one(&mut **tx).await?;
        let status: String = row.get("status");
        let devices=sqlx::query("SELECT device_id,node_id,binding_version,device_keys->'keys' AS public_keys FROM cloud_crypto_devices d WHERE account_id=$1 AND NOT revoked AND device_keys IS NOT NULL AND (node_id IS NULL OR EXISTS(SELECT 1 FROM cloud_ai_bindings b WHERE b.ai_id=d.account_id AND b.node_id=d.node_id AND b.version=d.binding_version)) ORDER BY device_id").bind(id).fetch_all(&mut **tx).await?;
        let keys = devices
            .iter()
            .map(|r| {
                json!([
                    r.get::<String, _>("device_id"),
                    r.get::<Option<Uuid>, _>("node_id"),
                    r.get::<Option<i64>, _>("binding_version"),
                    r.get::<Value, _>("public_keys")
                ])
            })
            .collect::<Vec<_>>();
        identities.push(json!([
            id,
            status,
            row.get::<Option<i64>, _>("version"),
            keys
        ]));
        if status == "active" {
            accounts.push(id);
        }
    }
    let conversation_revision: Option<i64> = if let Some(cid) = q.conversation_id {
        Some(
            sqlx::query_scalar("SELECT revision FROM cloud_conversations WHERE id=$1")
                .bind(cid)
                .fetch_one(&mut **tx)
                .await?,
        )
    } else {
        None
    };
    let administrators_revision: Option<i64> = if q.kind != "memory" {
        Some(
            sqlx::query_scalar("SELECT administrators_revision FROM accounts WHERE id=$1")
                .bind(q.ai_id)
                .fetch_one(&mut **tx)
                .await?,
        )
    } else {
        None
    };
    let revision = format!(
        "{:x}",
        Sha256::digest(
            serde_json::to_vec(&json!([
                key(q),
                conversation_revision,
                administrators_revision,
                identities
            ]))
            .unwrap()
        )
    );
    Ok(json!({"accounts":accounts,"revision":revision}))
}
async fn participants(
    State(s): State<Arc<AppState>>,
    h: HeaderMap,
    Query(q): Query<Scope>,
) -> Result<Json<Value>> {
    let mut tx = s.pool.begin().await?;
    nodes::lock(&mut tx).await?;
    let a = crypto::actor(&s, &h, &mut tx).await?;
    permission(&mut tx, &a, &q).await?;
    Ok(Json(audience(&mut tx, &q).await?))
}
async fn list(
    State(s): State<Arc<AppState>>,
    h: HeaderMap,
    Query(q): Query<Scope>,
) -> Result<Json<Value>> {
    let mut tx = s.pool.begin().await?;
    let a = crypto::actor(&s, &h, &mut tx).await?;
    permission(&mut tx, &a, &q).await?;
    let rows=sqlx::query("SELECT * FROM cloud_crypto_resources WHERE ai_id=$1 AND scope=$2 ORDER BY updated_at,item_id").bind(q.ai_id).bind(key(&q)).fetch_all(&mut *tx).await?;
    let mut items = vec![];
    for r in rows {
        items.push(view(&mut tx, &r).await?);
    }
    Ok(Json(json!({"items":items})))
}
async fn sources(
    tx: &mut Tx<'_>,
    cid: Option<Uuid>,
    sources: &[connector::node::SourceRevision],
) -> Result<()> {
    if sources.len() > 512 {
        return Err(AppError::bad("资料来源超限"));
    }
    for source in sources {
        let cid = cid.ok_or_else(|| AppError::bad("全局资料不能引用窗口原文"))?;
        let rev: i64 = source
            .revision
            .parse()
            .map_err(|_| AppError::bad("来源版本无效"))?;
        let valid:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM cloud_messages WHERE id=$1 AND conversation=$2 AND revision=$3 AND NOT withdrawn AND status='committed')").bind(crypto::uid(&source.message_id)?).bind(cid).bind(rev).fetch_one(&mut **tx).await?;
        if !valid {
            return Err(AppError::conflict("资料来源已失效"));
        }
    }
    Ok(())
}
async fn save(
    State(s): State<Arc<AppState>>,
    h: HeaderMap,
    Path(id): Path<Uuid>,
    Json(v): Json<SaveResource>,
) -> Result<Json<Value>> {
    let rid = crypto::uid(&v.request_id)?;
    let scope = Scope {
        ai_id: crypto::uid(&v.ai_id)?,
        conversation_id: v.conversation_id.as_deref().map(crypto::uid).transpose()?,
        kind: v.kind.clone(),
    };
    let scope_key = key(&scope);
    let fp = format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&json!([id, v])).unwrap())
    );
    let mut tx = s.pool.begin().await?;
    nodes::lock(&mut tx).await?;
    let a = crypto::actor(&s, &h, &mut tx).await?;
    crypto::device(&mut tx, &a, &v.device_id).await?;
    permission(&mut tx, &a, &scope).await?;
    if let Some(r) =
        sqlx::query("SELECT * FROM cloud_crypto_resource_receipts WHERE actor=$1 AND request_id=$2")
            .bind(a.id)
            .bind(rid)
            .fetch_optional(&mut *tx)
            .await?
    {
        if r.get::<String, _>("fingerprint") != fp {
            return Err(AppError::conflict("资料命令ID已用于其他内容"));
        }
        let r = sqlx::query("SELECT * FROM cloud_crypto_resources WHERE item_id=$1")
            .bind(id.to_string())
            .fetch_one(&mut *tx)
            .await?;
        return Ok(Json(view(&mut tx, &r).await?));
    }
    if audience(&mut tx, &scope).await?["revision"] != v.expected_audience_revision {
        let mut error = AppError::conflict("资料接收设备或授权名单已变化，请重新确认");
        error.code = "audience_changed";
        return Err(error);
    }
    if let Some(cid) = scope.conversation_id {
        chat::writable(&mut tx, cid).await?;
    }
    let old = sqlx::query("SELECT * FROM cloud_crypto_resources WHERE item_id=$1")
        .bind(id.to_string())
        .fetch_optional(&mut *tx)
        .await?;
    let rev = old.as_ref().map_or(0, |r| r.get::<i64, _>("revision"));
    if v.expected_revision != rev.to_string()
        || old
            .as_ref()
            .is_some_and(|r| r.get::<String, _>("scope") != scope_key)
    {
        return Err(AppError::conflict("资料版本或归属已变化"));
    }
    if old
        .as_ref()
        .is_some_and(|r| r.get::<Option<String>, _>("tombstone").is_some())
        && v.tombstone.is_none()
        && scope.kind != "summarySettings"
    {
        return Err(AppError::conflict(
            "已遗忘或失效条目不能由旧记录恢复，请新建条目",
        ));
    }
    if v.tombstone
        .as_deref()
        .is_some_and(|s| !matches!(s, "forgotten" | "stale"))
        || v.tombstone.is_some() == v.sealed.is_some()
    {
        return Err(AppError::bad("资料密文或墓碑无效"));
    }
    if old.is_none()
        && matches!(scope.kind.as_str(), "profile" | "summarySettings")
        && sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM cloud_crypto_resources WHERE ai_id=$1 AND scope=$2)",
        )
        .bind(scope.ai_id)
        .bind(&scope_key)
        .fetch_one(&mut *tx)
        .await?
    {
        return Err(AppError::conflict("当前范围已有资料，请读取后编辑"));
    }
    if matches!(scope.kind.as_str(), "profile" | "summarySettings") && v.tombstone.is_some() {
        return Err(AppError::bad("配置资料只能保存新版本"));
    }
    // Forgetting clears content while retaining the authoritative provenance tombstone.
    let stored_sources = if v.tombstone.is_some() {
        if let Some(old) = &old {
            old.get::<Value, _>("sources")
        } else {
            if v.sources.len() > 512 {
                return Err(AppError::bad("资料来源超限"));
            }
            for source in &v.sources {
                crypto::uid(&source.message_id)?;
                if source.revision.parse::<u64>().ok().is_none_or(|r| r == 0) {
                    return Err(AppError::bad("来源版本无效"));
                }
            }
            json!(v.sources)
        }
    } else {
        sources(&mut tx, scope.conversation_id, &v.sources).await?;
        json!(v.sources)
    };
    if let Some(e) = &v.sealed {
        crypto::envelope(
            &mut tx,
            &a,
            &connector::crypto::resource_room(
                &scope.ai_id.to_string(),
                &scope.kind,
                scope.conversation_id.map(|id| id.to_string()).as_deref(),
            ),
            &format!("${id}.{}.final", rev + 1),
            e,
        )
        .await?;
    }
    let material = Uuid::new_v4();
    let r=sqlx::query("INSERT INTO cloud_crypto_resources(ai_id,scope,conversation,item_id,kind,revision,sealed,tombstone,sources,material_revision) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10) ON CONFLICT(item_id) DO UPDATE SET revision=$6,sealed=$7,tombstone=$8,sources=$9,material_revision=$10,updated_at=now() RETURNING *").bind(scope.ai_id).bind(&scope_key).bind(scope.conversation_id).bind(id.to_string()).bind(&scope.kind).bind(rev+1).bind(v.sealed.as_ref().map(|e|json!(e))).bind(&v.tombstone).bind(stored_sources).bind(material).fetch_one(&mut *tx).await?;
    sqlx::query("INSERT INTO cloud_crypto_resource_receipts VALUES($1,$2,$3,$4,$5,$6)")
        .bind(a.id)
        .bind(rid)
        .bind(fp)
        .bind(scope.ai_id)
        .bind(scope_key)
        .bind(id.to_string())
        .execute(&mut *tx)
        .await?;
    changed(&mut tx, scope.ai_id, scope.conversation_id).await?;
    let out = view(&mut tx, &r).await?;
    tx.commit().await?;
    Ok(Json(out))
}
pub(crate) async fn changed(tx: &mut Tx<'_>, ai: Uuid, cid: Option<Uuid>) -> Result<()> {
    crate::handover::changed(tx, ai).await?;
    if let Some(cid) = cid {
        let epoch = Uuid::new_v4();
        sqlx::query("INSERT INTO cloud_material_epochs VALUES($1,$2,$3) ON CONFLICT(ai_id,conversation) DO UPDATE SET revision=$3").bind(ai).bind(cid).bind(epoch).execute(&mut **tx).await?;
        let rows=sqlx::query("SELECT * FROM cloud_node_tasks WHERE ai_id=$1 AND conversation=$2 AND state IN ('queued','running','uncertain') AND material_scopes<>'[]'::jsonb").bind(ai).bind(cid).fetch_all(&mut **tx).await?;
        nodes::invalidate_rows(tx, rows, "material_changed").await?;
        let members = chat::member(tx, cid, ai).await?;
        chat::notify(tx, cid, members).await?;
    }
    sqlx::query("INSERT INTO cloud_node_events(ai_id,binding_version,kind) SELECT ai_id,version,'sourcesChanged' FROM cloud_ai_bindings WHERE ai_id=$1").bind(ai).execute(&mut **tx).await?;
    Ok(())
}
async fn applied(
    State(s): State<Arc<AppState>>,
    h: HeaderMap,
    Path(id): Path<Uuid>,
    Json(v): Json<ApplyResource>,
) -> Result<Json<Value>> {
    if !matches!(v.status.as_str(), "applied" | "rejected") {
        return Err(AppError::bad("应用状态无效"));
    }
    let mut tx = s.pool.begin().await?;
    nodes::lock(&mut tx).await?;
    let a = crypto::actor(&s, &h, &mut tx).await?;
    let n = a.node.as_ref().ok_or_else(AppError::forbidden)?;
    crypto::device(&mut tx, &a, &v.device_id).await?;
    let r = sqlx::query("SELECT * FROM cloud_crypto_resources WHERE item_id=$1")
        .bind(id.to_string())
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(AppError::not_found)?;
    let scope = scope(&r);
    permission(&mut tx, &a, &scope).await?;
    let revision: i64 = r.get("revision");
    if v.revision != revision.to_string() {
        return Err(AppError::conflict("不能确认旧资料版本"));
    }
    sqlx::query("INSERT INTO cloud_crypto_applied(ai_id,scope,item_id,revision,node_id,binding_version,status) VALUES($1,$2,$3,$4,$5,$6,$7) ON CONFLICT(ai_id,scope,item_id) DO UPDATE SET revision=$4,node_id=$5,binding_version=$6,status=$7,error_code=NULL,updated_at=now()").bind(n.ai).bind(key(&scope)).bind(id.to_string()).bind(revision).bind(n.node).bind(n.version).bind(v.status).execute(&mut *tx).await?;
    let out = view(&mut tx, &r).await?;
    tx.commit().await?;
    Ok(Json(out))
}
pub(crate) async fn ready(tx: &mut Tx<'_>, ai: Uuid, cid: Uuid) -> Result<()> {
    let pending:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM cloud_crypto_resources r JOIN cloud_ai_bindings b ON b.ai_id=r.ai_id WHERE r.ai_id=$1 AND (r.conversation IS NULL OR r.conversation=$2) AND NOT EXISTS(SELECT 1 FROM cloud_crypto_applied a WHERE a.ai_id=r.ai_id AND a.scope=r.scope AND a.item_id=r.item_id AND a.revision=r.revision AND a.node_id=b.node_id AND a.binding_version=b.version AND a.status='applied'))").bind(ai).bind(cid).fetch_one(&mut **tx).await?;
    if pending {
        let mut e = AppError::conflict("最新加密资料尚未在当前节点应用");
        e.code = "crypto_material_pending";
        return Err(e);
    }
    Ok(())
}
pub(crate) async fn revoke_access(tx: &mut Tx<'_>, cid: Uuid, removed: &[Uuid]) -> Result<()> {
    let rows=sqlx::query("UPDATE cloud_crypto_resources SET sealed=NULL,tombstone='stale',revision=revision+1,material_revision=$3,updated_at=now() WHERE conversation=$1 AND ai_id=ANY($2) AND tombstone IS NULL RETURNING ai_id").bind(cid).bind(removed).bind(Uuid::new_v4()).fetch_all(&mut **tx).await?;
    let mut accounts = rows
        .iter()
        .map(|r| r.get::<Uuid, _>("ai_id"))
        .collect::<Vec<_>>();
    accounts.sort();
    accounts.dedup();
    for ai in accounts {
        crate::handover::changed(tx, ai).await?;
        sqlx::query("INSERT INTO cloud_material_epochs VALUES($1,$2,$3) ON CONFLICT(ai_id,conversation) DO UPDATE SET revision=$3").bind(ai).bind(cid).bind(Uuid::new_v4()).execute(&mut **tx).await?;
    }
    Ok(())
}
pub(crate) async fn source_changed(tx: &mut Tx<'_>, mid: Uuid) -> Result<()> {
    let rows=sqlx::query("UPDATE cloud_crypto_resources SET sealed=NULL,tombstone='stale',revision=revision+1,material_revision=$2,updated_at=now() WHERE tombstone IS NULL AND EXISTS(SELECT 1 FROM jsonb_array_elements(sources) s WHERE s->>'messageId'=$1::text) RETURNING ai_id,conversation").bind(mid).bind(Uuid::new_v4()).fetch_all(&mut **tx).await?;
    for r in rows {
        changed(tx, r.get("ai_id"), r.get("conversation")).await?;
    }
    Ok(())
}
