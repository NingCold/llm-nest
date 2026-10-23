//! Authenticated import of explicitly identified local history; never creates execution work.
use crate::{AppError, AppState, Result, auth, chat, db as sqlx, nodes};
use axum::{
    Json, Router,
    extract::{State, rejection::JsonRejection},
    http::HeaderMap,
    routing::post,
};
use connector::migration::*;
use serde_json::json;
use sha2::{Digest, Sha256};
use sqlx::Row;
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};
use uuid::Uuid;

pub(crate) fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/migration/preview", post(preview))
        .route("/migration/import", post(import))
}
fn bad(s: &str) -> AppError {
    AppError::bad(s)
}
fn uuid(s: &str) -> Result<Uuid> {
    s.parse().map_err(|_| bad("迁移标识必须是 UUID"))
}
fn number(s: &str) -> Result<i64> {
    s.parse::<i64>()
        .ok()
        .filter(|n| *n > 0)
        .ok_or_else(|| bad("迁移修订和序号必须为正整数"))
}
fn digest(v: &impl serde::Serialize) -> String {
    format!("{:x}", Sha256::digest(serde_json::to_vec(v).unwrap()))
}
fn body<T>(v: std::result::Result<Json<T>, JsonRejection>) -> Result<T> {
    v.map(|Json(v)| v).map_err(|e| {
        if e.status() == axum::http::StatusCode::PAYLOAD_TOO_LARGE {
            AppError {
                status: e.status(),
                code: "migration_too_large",
                message: "当前单次迁移上限 128 MiB，未导入；需另行分批迁移".into(),
            }
        } else {
            bad("迁移 JSON 格式或字段无效")
        }
    })
}
fn validate(v: &MigrationSnapshot) -> Result<()> {
    if v.version != 1 {
        return Err(bad("不支持的迁移格式版本"));
    }
    uuid(&v.source_id)?;
    let mut actors = HashMap::new();
    for a in &v.actors {
        uuid(&a.id)?;
        number(&a.revision)?;
        if actors.insert(a.id.as_str(), a).is_some()
            || !matches!(a.kind.as_str(), "human" | "ai")
            || !matches!(a.status.as_str(), "active" | "disabled")
            || a.name.trim().is_empty()
            || a.name.chars().count() > 80
            || a.bio.len() > 4096
        {
            return Err(bad("迁移账号重复或资料无效"));
        }
    }
    if v.actors.iter().filter(|a| a.kind == "human").count() != 1
        || actors
            .get(v.source_human_id.as_str())
            .is_none_or(|a| a.kind != "human")
    {
        return Err(bad("迁移只允许唯一来源真人，映射到当前登录真人"));
    }
    let mut conversations = HashMap::new();
    let mut pairs = HashSet::new();
    for c in &v.conversations {
        uuid(&c.id)?;
        number(&c.revision)?;
        if conversations.insert(c.id.as_str(), c).is_some()
            || !matches!(c.kind.as_str(), "direct" | "group")
            || c.name.chars().count() > 100
        {
            return Err(bad("迁移窗口重复或类型无效"));
        }
        let mut seen = HashSet::new();
        for m in &c.members {
            if !actors.contains_key(m.actor_id.as_str())
                || !seen.insert(&m.actor_id)
                || !matches!(m.role.as_str(), "owner" | "admin" | "member")
            {
                return Err(bad("迁移成员无效或重复"));
            }
        }
        if !c
            .members
            .iter()
            .any(|m| m.actor_id == v.source_human_id && !m.left)
        {
            return Err(bad("不能导入来源真人已经退出的窗口"));
        }
        if c.kind == "direct" {
            if c.members.len() != 2 || c.members.iter().any(|m| m.left) {
                return Err(bad("私聊必须有两个当前成员"));
            }
            let mut ids = c
                .members
                .iter()
                .map(|m| m.actor_id.as_str())
                .collect::<Vec<_>>();
            ids.sort();
            if !pairs.insert(ids) {
                return Err(bad("来源包含重复账号私聊"));
            }
        } else if !(2..=100).contains(&c.members.len())
            || c.members
                .iter()
                .filter(|m| !m.left && m.role == "owner")
                .count()
                != 1
        {
            return Err(bad("群需要唯一当前群主及 2 至 100 个成员记录"));
        }
    }
    let mut messages = HashMap::new();
    let mut sequences = HashSet::new();
    for m in &v.messages {
        uuid(&m.id)?;
        number(&m.revision)?;
        let seq = number(&m.sequence)?;
        let c = conversations
            .get(m.conversation_id.as_str())
            .ok_or_else(|| bad("消息窗口不存在"))?;
        if messages.insert(m.id.as_str(), m).is_some()
            || !sequences.insert((&m.conversation_id, seq))
            || !c.members.iter().any(|x| x.actor_id == m.sender_id)
            || !matches!(
                m.status.as_str(),
                "committed" | "generating" | "interrupted" | "withdrawn"
            )
            || m.text.len() > 1024 * 1024
        {
            return Err(bad("迁移消息重复、发送者或状态无效"));
        }
        m.created_at
            .parse::<chrono::DateTime<chrono::Utc>>()
            .map_err(|_| bad("消息时间无效"))?;
        let mut indices = HashSet::new();
        for a in &m.attachments {
            if !indices.insert(a.source_index) || !matches!(a.kind.as_str(), "image" | "file") {
                return Err(bad("附件索引或类型无效"));
            }
        }
        if m.mentions
            .iter()
            .any(|a| !c.members.iter().any(|x| x.actor_id == *a))
        {
            return Err(bad("提及的账号不是来源窗口成员"));
        }
    }
    for m in &v.messages {
        if let Some(id) = &m.reply_to
            && messages
                .get(id.as_str())
                .is_none_or(|r| r.conversation_id != m.conversation_id)
        {
            return Err(bad("回复来源不存在或跨窗口"));
        }
        if let Some(r) = &m.reference {
            let target = conversations[m.conversation_id.as_str()];
            let source = conversations
                .get(r.conversation_id.as_str())
                .ok_or_else(|| bad("引用群窗口不存在"))?;
            if target.kind != "direct"
                || source.kind != "group"
                || !(1..=8).contains(&r.sources.len())
            {
                return Err(bad("群引用形状无效"));
            }
            let mut seen = HashSet::new();
            for s in &r.sources {
                number(&s.revision)?;
                if !seen.insert(&s.message_id)
                    || messages
                        .get(s.message_id.as_str())
                        .is_none_or(|m| m.conversation_id != r.conversation_id)
                {
                    return Err(bad("引用消息不存在、重复或跨窗口"));
                }
            }
        }
    }
    let mut contacts = HashSet::new();
    for c in &v.contacts {
        if c.owner_id == c.target_id
            || !actors.contains_key(c.owner_id.as_str())
            || !actors.contains_key(c.target_id.as_str())
            || !contacts.insert((&c.owner_id, &c.target_id))
        {
            return Err(bad("联系人关系无效或重复"));
        }
    }
    let mut states = HashSet::new();
    for l in &v.list_state {
        let c = conversations
            .get(l.conversation_id.as_str())
            .ok_or_else(|| bad("读态窗口不存在"))?;
        if !c
            .members
            .iter()
            .any(|m| m.actor_id == l.owner_id && !m.left)
            || !states.insert((&l.owner_id, &l.conversation_id))
        {
            return Err(bad("个人窗口状态无效或重复"));
        }
        for id in &l.read_message_ids {
            if messages
                .get(id.as_str())
                .is_none_or(|m| m.conversation_id != l.conversation_id || m.status == "generating")
            {
                return Err(bad("已读消息不存在或不属于窗口"));
            }
        }
    }
    Ok(())
}
async fn existing(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    owner: Uuid,
    v: &MigrationSnapshot,
    hash: &str,
) -> Result<Option<MigrationMapping>> {
    if let Some(r) = sqlx::query(
        "SELECT snapshot_digest,mapping FROM cloud_imports WHERE owner=$1 AND source_id=$2",
    )
    .bind(owner)
    .bind(uuid(&v.source_id)?)
    .fetch_optional(&mut **tx)
    .await?
    {
        if r.get::<String, _>(0) != hash {
            return Err(AppError::conflict(
                "该来源已导入不同快照；不能重复建立身份或历史",
            ));
        }
        return Ok(Some(
            serde_json::from_value(r.get(1)).map_err(|_| AppError::internal())?,
        ));
    }
    Ok(None)
}
fn confirmation(owner: Uuid, hash: &str) -> String {
    digest(&json!(["create-all-ai-v1", owner, hash]))
}
async fn preview(
    State(s): State<Arc<AppState>>,
    h: HeaderMap,
    v: std::result::Result<Json<MigrationSnapshot>, JsonRejection>,
) -> Result<Json<MigrationPreview>> {
    let p = auth::authenticate(&s, &h).await?;
    let v = body(v)?;
    validate(&v)?;
    let hash = digest(&v);
    let mut tx = s.pool.begin().await?;
    let done = existing(&mut tx, p.id, &v, &hash).await?.is_some();
    Ok(Json(MigrationPreview {snapshot_digest:hash.clone(),preview_digest:confirmation(p.id,&hash),target_human_id:p.id.to_string(),new_ai_ids:v.actors.iter().filter(|a|a.kind=="ai").map(|a|a.id.clone()).collect(),warnings:vec!["唯一来源真人映射到当前登录账号；每个 AI 都新建并授予当前真人初始管理员，不合并同名账号".into(),"归档和旧导入窗口只读；已退出成员不恢复；所有窗口暂停，旧运行、自动设置及未来约定不执行".into(),format!("{} 项旧参与设置或约定仅供预览，恢复必须另行确认",v.deferred.len())],already_imported:done}))
}
async fn import(
    State(s): State<Arc<AppState>>,
    h: HeaderMap,
    v: std::result::Result<Json<MigrationImport>, JsonRejection>,
) -> Result<Json<MigrationMapping>> {
    let p = auth::authenticate(&s, &h).await?;
    let request = body(v)?;
    validate(&request.snapshot)?;
    let v = &request.snapshot;
    let hash = digest(v);
    if request.preview_digest != confirmation(p.id, &hash) {
        return Err(AppError::conflict(
            "迁移预览与当前快照或登录账号不一致，请重新预览确认",
        ));
    }
    let mut tx = s.pool.begin().await?;
    nodes::lock(&mut tx).await?;
    if let Some(mapping) = existing(&mut tx, p.id, v, &hash).await? {
        return Ok(Json(mapping));
    }
    let batch = Uuid::new_v4();
    let stage = s.attachments.join(format!(".migration-{batch}"));
    tokio::fs::create_dir_all(&stage).await?;
    let mut published = Vec::new();
    let result = apply(&s, &mut tx, p.id, v, &hash, batch, &stage, &mut published).await;
    match result {
        Ok(mapping) => {
            // A failed COMMIT can have an unknown outcome. Keep published files until
            // checking the authoritative receipt, rather than deleting committed data.
            if let Err(error) = tx.commit().await {
                let mut verification = s.pool.begin().await?;
                // The original COMMIT may still be finishing on another connection.
                // Taking its transaction lock first makes absence authoritative.
                nodes::lock(&mut verification).await?;
                let committed: bool =
                    sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM cloud_imports WHERE id=$1)")
                        .bind(batch)
                        .fetch_one(&mut *verification)
                        .await?;
                verification.commit().await?;
                if !committed {
                    cleanup(&stage, &published).await?;
                    return Err(error.into());
                }
            }
            tokio::fs::remove_dir_all(&stage).await?;
            Ok(Json(mapping))
        }
        Err(error) => {
            tx.rollback().await?;
            cleanup(&stage, &published).await?;
            Err(error)
        }
    }
}
async fn cleanup(stage: &std::path::Path, published: &[std::path::PathBuf]) -> Result<()> {
    for path in published {
        tokio::fs::remove_dir_all(path).await?;
    }
    tokio::fs::remove_dir_all(stage).await?;
    Ok(())
}
#[allow(
    clippy::too_many_arguments,
    reason = "Keep the database transaction and staged attachment cleanup ownership explicit at the import boundary"
)]
async fn apply(
    s: &AppState,
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    owner: Uuid,
    v: &MigrationSnapshot,
    hash: &str,
    batch: Uuid,
    stage: &std::path::Path,
    published: &mut Vec<std::path::PathBuf>,
) -> Result<MigrationMapping> {
    let mut mapping = MigrationMapping {
        import_id: batch.to_string(),
        source_id: v.source_id.clone(),
        snapshot_digest: hash.into(),
        actors: vec![],
        conversations: vec![],
        messages: vec![],
    };
    let mut actors = HashMap::new();
    let mut conversations = HashMap::new();
    let mut messages = HashMap::new();
    for a in &v.actors {
        let id = if a.kind == "human" {
            owner
        } else {
            Uuid::new_v4()
        };
        actors.insert(a.id.as_str(), id);
        if a.kind == "ai" {
            sqlx::query("INSERT INTO accounts(id,name,bio,kind,status) VALUES($1,$2,$3,'ai',$4)")
                .bind(id)
                .bind(&a.name)
                .bind(&a.bio)
                .bind(&a.status)
                .execute(&mut **tx)
                .await?;
            sqlx::query(
                "INSERT INTO cloud_ai_admins(ai_id,administrator,initial) VALUES($1,$2,true)",
            )
            .bind(id)
            .bind(owner)
            .execute(&mut **tx)
            .await?;
            sqlx::query("INSERT INTO cloud_ai_bindings(ai_id) VALUES($1)")
                .bind(id)
                .execute(&mut **tx)
                .await?;
        }
        let revision: i64 = sqlx::query_scalar("SELECT profile_revision FROM accounts WHERE id=$1")
            .bind(id)
            .fetch_one(&mut **tx)
            .await?;
        mapping.actors.push(MigrationEntityMapping {
            source_id: a.id.clone(),
            target_id: id.to_string(),
            source_revision: a.revision.clone(),
            target_revision: revision.to_string(),
        });
    }
    for c in &v.conversations {
        let id = Uuid::new_v4();
        conversations.insert(c.id.as_str(), id);
        let (first, second) = if c.kind == "direct" {
            let mut pair = c
                .members
                .iter()
                .map(|m| actors[m.actor_id.as_str()])
                .collect::<Vec<_>>();
            pair.sort();
            (Some(pair[0]), Some(pair[1]))
        } else {
            (None, None)
        };
        if let (Some(a), Some(b)) = (first, second) {
            let conflict:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM cloud_conversations WHERE first_id=$1 AND second_id=$2)").bind(a).bind(b).fetch_one(&mut **tx).await?;
            if conflict {
                return Err(AppError::conflict("目标唯一私聊已存在，不能混入旧历史"));
            }
        }
        sqlx::query("INSERT INTO cloud_conversations(id,first_id,second_id,kind,name,read_only) VALUES($1,$2,$3,$4,$5,$6)").bind(id).bind(first).bind(second).bind(&c.kind).bind(&c.name).bind(c.archived||c.legacy).execute(&mut **tx).await?;
        for m in c.members.iter().filter(|m| !m.left) {
            let actor = actors[m.actor_id.as_str()];
            sqlx::query("INSERT INTO cloud_members(conversation,account_id,role) VALUES($1,$2,$3)")
                .bind(id)
                .bind(actor)
                .bind(&m.role)
                .execute(&mut **tx)
                .await?;
            sqlx::query("INSERT INTO cloud_list(owner,conversation) VALUES($1,$2)")
                .bind(actor)
                .bind(id)
                .execute(&mut **tx)
                .await?;
        }
        sqlx::query("INSERT INTO cloud_participation(conversation,paused,reason) VALUES($1,true,'migration_confirmation_required')").bind(id).execute(&mut **tx).await?;
        mapping.conversations.push(MigrationEntityMapping {
            source_id: c.id.clone(),
            target_id: id.to_string(),
            source_revision: c.revision.clone(),
            target_revision: "1".into(),
        });
    }
    for c in &v.contacts {
        sqlx::query("INSERT INTO cloud_contacts(owner,target,listed,blocked) VALUES($1,$2,$3,$4)")
            .bind(actors[c.owner_id.as_str()])
            .bind(actors[c.target_id.as_str()])
            .bind(c.listed)
            .bind(c.blocked)
            .execute(&mut **tx)
            .await?;
    }
    let mut ordered = v.messages.iter().collect::<Vec<_>>();
    ordered.sort_by_key(|m| (&m.conversation_id, m.sequence.parse::<i64>().unwrap()));
    for m in ordered {
        let id = Uuid::new_v4();
        messages.insert(m.id.as_str(), id);
        let mut parts = Vec::new();
        let mut attachment_mapping = Vec::new();
        if m.status != "withdrawn" {
            for kind in ["image", "file"] {
                for a in m.attachments.iter().filter(|a| a.kind == kind) {
                    attachment_mapping.push(MigrationAttachmentMapping {
                        source_index: a.source_index,
                        target_index: parts.len(),
                    });
                    parts.push((
                        kind,
                        chat::Upload {
                            name: a.name.clone(),
                            mime: a.mime.clone(),
                            data: a.data.clone(),
                        },
                    ));
                }
            }
        }
        let split = parts.partition_point(|(kind, _)| *kind == "image");
        let mut uploads = parts.into_iter().map(|(_, u)| u).collect::<Vec<_>>();
        let files = uploads.split_off(split);
        let attachments = chat::save_uploads(stage, id, &uploads, &files).await?;
        let status = match m.status.as_str() {
            "generating" => "interrupted",
            "withdrawn" => "committed",
            s => s,
        };
        let provenance = ImportProvenance {
            import_id: batch.to_string(),
            source_message_id: m.id.clone(),
            source_revision: m.revision.clone(),
        };
        let pos:i64=sqlx::query_scalar("INSERT INTO cloud_messages(id,conversation,sender,body,created_at,updated_at,withdrawn,status,attachments,sender_name,mentions,import_origin) VALUES($1,$2,$3,$4,$5,$5,$6,$7,$8,$9,$10,$11) RETURNING position").bind(id).bind(conversations[m.conversation_id.as_str()]).bind(actors[m.sender_id.as_str()]).bind(if m.status=="withdrawn"{""}else{&m.text}).bind(m.created_at.parse::<chrono::DateTime<chrono::Utc>>().unwrap()).bind(m.status=="withdrawn").bind(status).bind(attachments).bind(&m.sender_name).bind(m.mentions.iter().map(|a|actors[a.as_str()]).collect::<Vec<_>>()).bind(json!(provenance)).fetch_one(&mut **tx).await?;
        mapping.messages.push(MigrationMessageMapping {
            source_id: m.id.clone(),
            target_id: id.to_string(),
            source_revision: m.revision.clone(),
            target_revision: "1".into(),
            target_position: pos.to_string(),
            attachments: attachment_mapping,
        });
    }
    for m in &v.messages {
        let mid = messages[m.id.as_str()];
        if let Some(reply) = &m.reply_to {
            sqlx::query("UPDATE cloud_messages SET reply_to=$2 WHERE id=$1")
                .bind(mid)
                .bind(messages[reply.as_str()])
                .execute(&mut **tx)
                .await?;
        }
        if let Some(r) = &m.reference
            && m.status != "withdrawn"
        {
            let target = v
                .conversations
                .iter()
                .find(|c| c.id == m.conversation_id)
                .unwrap();
            let group = v
                .conversations
                .iter()
                .find(|c| c.id == r.conversation_id)
                .unwrap();
            let permitted = target.members.iter().all(|t| {
                group
                    .members
                    .iter()
                    .any(|g| g.actor_id == t.actor_id && !g.left)
            });
            let valid = permitted
                && !r.stale
                && r.sources.iter().all(|src| {
                    v.messages.iter().any(|s| {
                        s.id == src.message_id
                            && s.revision == src.revision
                            && s.status == "committed"
                    })
                });
            let sources = r
                .sources
                .iter()
                .map(|src| json!({"messageId":messages[src.message_id.as_str()],"revision":"1"}))
                .collect::<Vec<_>>();
            sqlx::query("UPDATE cloud_messages SET context_reference=$2 WHERE id=$1").bind(mid).bind(json!({"conversationId":conversations[r.conversation_id.as_str()],"sources":sources,"revision":"1","stale":!valid})).execute(&mut **tx).await?;
            if valid {
                for src in &r.sources {
                    sqlx::query("INSERT INTO cloud_reference_sources(message_id,conversation,source_id,source_revision) VALUES($1,$2,$3,1)").bind(mid).bind(conversations[r.conversation_id.as_str()]).bind(messages[src.message_id.as_str()]).execute(&mut **tx).await?;
                }
            }
        }
    }
    for l in &v.list_state {
        for id in &l.read_message_ids {
            sqlx::query("INSERT INTO cloud_import_reads(owner,message_id) VALUES($1,$2) ON CONFLICT DO NOTHING").bind(actors[l.owner_id.as_str()]).bind(messages[id.as_str()]).execute(&mut **tx).await?;
        }
        sqlx::query("UPDATE cloud_list SET pinned=$3,manual_unread=$4,read_position=$5 WHERE owner=$1 AND conversation=$2").bind(actors[l.owner_id.as_str()]).bind(conversations[l.conversation_id.as_str()]).bind(l.pinned).bind(l.manual_unread).bind(0i64).execute(&mut **tx).await?;
    }
    for c in &v.conversations {
        chat::notify(
            tx,
            conversations[c.id.as_str()],
            c.members
                .iter()
                .filter(|m| !m.left)
                .map(|m| actors[m.actor_id.as_str()])
                .collect(),
        )
        .await?;
    }
    sqlx::query("INSERT INTO cloud_events(owner) VALUES($1)")
        .bind(owner)
        .execute(&mut **tx)
        .await?;
    sqlx::query("INSERT INTO cloud_imports(id,owner,source_id,snapshot_digest,mapping) VALUES($1,$2,$3,$4,$5)").bind(batch).bind(owner).bind(uuid(&v.source_id)?).bind(hash).bind(json!(mapping)).execute(&mut **tx).await?;
    for m in &mapping.messages {
        if !m.attachments.is_empty() {
            let path = s.attachments.join(&m.target_id);
            tokio::fs::rename(stage.join(&m.target_id), &path).await?;
            published.push(path);
        }
    }
    Ok(mapping)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat::tests::Fixture;
    fn sample() -> MigrationSnapshot {
        let h = Uuid::new_v4().to_string();
        let ai = Uuid::new_v4().to_string();
        let c = Uuid::new_v4().to_string();
        let a = Uuid::new_v4().to_string();
        let b = Uuid::new_v4().to_string();
        serde_json::from_value(json!({"version":1,"sourceId":Uuid::new_v4(),"sourceHumanId":h,
            "actors":[{"id":h,"kind":"human","name":"local","bio":"","status":"active","revision":"2"},{"id":ai,"kind":"ai","name":"local AI","bio":"public","status":"disabled","revision":"5"}],
            "contacts":[{"ownerId":h,"targetId":ai,"listed":false,"blocked":true}],
            "conversations":[{"id":c,"kind":"direct","name":"old","revision":"4","archived":false,"legacy":false,"members":[{"actorId":h,"role":"member","left":false},{"actorId":ai,"role":"member","left":false}]}],
            "messages":[{"id":a,"conversationId":c,"senderId":h,"senderName":"old human","sequence":"7","revision":"3","status":"committed","createdAt":"2026-01-01T00:00:00Z","text":"history","attachments":[{"sourceIndex":2,"kind":"file","name":"empty.txt","mime":"text/plain","data":""}],"replyTo":null,"mentions":[],"reference":null},
            {"id":b,"conversationId":c,"senderId":ai,"senderName":"old AI","sequence":"9","revision":"2","status":"generating","createdAt":"2026-01-02T00:00:00Z","text":"partial","attachments":[],"replyTo":a,"mentions":[],"reference":null}],
            "listState":[{"ownerId":h,"conversationId":c,"pinned":true,"manualUnread":true,"readMessageIds":[a]}],"deferred":[{"kind":"intent","conversationId":c,"description":"confirm later"}]})).unwrap()
    }
    async fn run(f: &Fixture, v: MigrationSnapshot) -> MigrationMapping {
        let p = preview(
            State(f.state.clone()),
            f.users[0].1.clone(),
            Ok(Json(v.clone())),
        )
        .await
        .unwrap()
        .0;
        import(
            State(f.state.clone()),
            f.users[0].1.clone(),
            Ok(Json(MigrationImport {
                snapshot: v,
                preview_digest: p.preview_digest,
            })),
        )
        .await
        .unwrap()
        .0
    }
    #[test]
    fn rejects_other_humans_missing_senders_and_left_human() {
        let v = sample();
        validate(&v).unwrap();
        let mut other = v.clone();
        other.actors[1].kind = "human".into();
        assert!(validate(&other).is_err());
        let mut other = v.clone();
        other.messages[0].sender_id = Uuid::new_v4().to_string();
        assert!(validate(&other).is_err());
        let mut other = v;
        other.conversations[0].members[0].left = true;
        assert!(validate(&other).is_err());
    }
    #[tokio::test]
    async fn authenticated_idempotent_import_preserves_state_and_never_enqueues() {
        let Some(f) = Fixture::new().await else {
            return;
        };
        let v = sample();
        assert!(
            preview(
                State(f.state.clone()),
                HeaderMap::new(),
                Ok(Json(v.clone()))
            )
            .await
            .is_err()
        );
        let mapping = run(&f, v.clone()).await;
        assert_eq!(mapping.actors[0].target_id, f.users[0].0.to_string());
        assert_eq!(
            mapping.messages[0].attachments[0],
            MigrationAttachmentMapping {
                source_index: 2,
                target_index: 0
            }
        );
        let cid = uuid(&mapping.conversations[0].target_id).unwrap();
        let mid = uuid(&mapping.messages[0].target_id).unwrap();
        let row = sqlx::query("SELECT * FROM cloud_messages WHERE id=$1")
            .bind(mid)
            .fetch_one(&f.state.pool)
            .await
            .unwrap();
        assert_eq!(chat::message(&row)["importOrigin"]["sourceRevision"], "3");
        assert_eq!(
            crate::consciousness::node_message(&row)["importOrigin"]["importId"],
            mapping.import_id
        );
        let reply = sqlx::query("SELECT status,reply_to FROM cloud_messages WHERE id=$1")
            .bind(uuid(&mapping.messages[1].target_id).unwrap())
            .fetch_one(&f.state.pool)
            .await
            .unwrap();
        assert_eq!(reply.get::<String, _>(0), "interrupted");
        assert_eq!(reply.get::<Uuid, _>(1), mid);
        let state=sqlx::query("SELECT pinned,manual_unread,read_position FROM cloud_list WHERE owner=$1 AND conversation=$2").bind(f.users[0].0).bind(cid).fetch_one(&f.state.pool).await.unwrap();
        assert!(state.get::<bool, _>(0));
        assert!(state.get::<bool, _>(1));
        assert_eq!(state.get::<i64, _>(2), 0);
        let contact = sqlx::query("SELECT listed,blocked FROM cloud_contacts WHERE owner=$1")
            .bind(f.users[0].0)
            .fetch_one(&f.state.pool)
            .await
            .unwrap();
        assert!(!contact.get::<bool, _>(0));
        assert!(contact.get::<bool, _>(1));
        let ai = uuid(&mapping.actors[1].target_id).unwrap();
        let disabled: String = sqlx::query_scalar("SELECT status FROM accounts WHERE id=$1")
            .bind(ai)
            .fetch_one(&f.state.pool)
            .await
            .unwrap();
        assert_eq!(disabled, "disabled");
        let tasks:i64=sqlx::query_scalar("SELECT (SELECT count(*) FROM cloud_node_tasks)+(SELECT count(*) FROM cloud_contact_intents)").fetch_one(&f.state.pool).await.unwrap();
        assert_eq!(tasks, 0);
        let paused: bool = sqlx::query_scalar(
            "SELECT paused AND NOT enabled FROM cloud_participation WHERE conversation=$1",
        )
        .bind(cid)
        .fetch_one(&f.state.pool)
        .await
        .unwrap();
        assert!(paused);
        assert_eq!(run(&f, v.clone()).await, mapping);
        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM cloud_messages")
            .fetch_one(&f.state.pool)
            .await
            .unwrap();
        assert_eq!(count, 2);
        let mut changed = v;
        changed.messages[0].text = "changed".into();
        assert!(
            preview(
                State(f.state.clone()),
                f.users[0].1.clone(),
                Ok(Json(changed))
            )
            .await
            .is_err()
        );
        assert_eq!(
            tokio::fs::read(
                f.state
                    .attachments
                    .join(&mapping.messages[0].target_id)
                    .join("0")
            )
            .await
            .unwrap(),
            Vec::<u8>::new()
        );
        f.close().await;
    }
    #[tokio::test]
    async fn preview_is_bound_to_account_and_failed_attachments_leave_no_rows_or_files() {
        let Some(f) = Fixture::new().await else {
            return;
        };
        let mut v = sample();
        let p = preview(
            State(f.state.clone()),
            f.users[0].1.clone(),
            Ok(Json(v.clone())),
        )
        .await
        .unwrap()
        .0;
        let e = import(
            State(f.state.clone()),
            f.users[1].1.clone(),
            Ok(Json(MigrationImport {
                snapshot: v.clone(),
                preview_digest: p.preview_digest,
            })),
        )
        .await
        .unwrap_err();
        assert_eq!(e.status, axum::http::StatusCode::CONFLICT);
        v.messages[0].attachments[0].data = "invalid base64!".into();
        let p = preview(
            State(f.state.clone()),
            f.users[0].1.clone(),
            Ok(Json(v.clone())),
        )
        .await
        .unwrap()
        .0;
        assert!(
            import(
                State(f.state.clone()),
                f.users[0].1.clone(),
                Ok(Json(MigrationImport {
                    snapshot: v,
                    preview_digest: p.preview_digest
                }))
            )
            .await
            .is_err()
        );
        let count:i64=sqlx::query_scalar("SELECT (SELECT count(*) FROM cloud_imports)+(SELECT count(*) FROM cloud_messages)+(SELECT count(*) FROM accounts WHERE kind='ai')").fetch_one(&f.state.pool).await.unwrap();
        assert_eq!(count, 0);
        assert!(
            tokio::fs::read_dir(&f.state.attachments)
                .await
                .unwrap()
                .next_entry()
                .await
                .unwrap()
                .is_none()
        );
        f.close().await;
    }
    #[tokio::test]
    async fn group_left_members_references_and_archive_read_only() {
        let Some(f) = Fixture::new().await else {
            return;
        };
        let mut v = sample();
        let g = Uuid::new_v4().to_string();
        let src = Uuid::new_v4().to_string();
        let left = MigrationActor {
            id: Uuid::new_v4().to_string(),
            kind: "ai".into(),
            name: "left".into(),
            bio: "".into(),
            status: "active".into(),
            revision: "1".into(),
        };
        v.conversations.push(MigrationConversation {
            id: g.clone(),
            kind: "group".into(),
            name: "old group".into(),
            revision: "8".into(),
            archived: true,
            legacy: false,
            members: vec![
                MigrationMember {
                    actor_id: v.source_human_id.clone(),
                    role: "owner".into(),
                    left: false,
                },
                MigrationMember {
                    actor_id: v.actors[1].id.clone(),
                    role: "member".into(),
                    left: false,
                },
                MigrationMember {
                    actor_id: left.id.clone(),
                    role: "member".into(),
                    left: true,
                },
            ],
        });
        let mut m = v.messages[0].clone();
        m.id = src.clone();
        m.conversation_id = g.clone();
        m.sender_id = left.id.clone();
        m.attachments.clear();
        v.messages.push(m);
        v.actors.push(left);
        v.messages[0].reference = Some(MigrationReference {
            conversation_id: g.clone(),
            sources: vec![MigrationSource {
                message_id: src,
                revision: "3".into(),
            }],
            stale: false,
        });
        let mapping = run(&f, v).await;
        let cid = uuid(&mapping.conversations[1].target_id).unwrap();
        let left = uuid(&mapping.actors[2].target_id).unwrap();
        let count: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM cloud_members WHERE conversation=$1 AND account_id=$2",
        )
        .bind(cid)
        .bind(left)
        .fetch_one(&f.state.pool)
        .await
        .unwrap();
        assert_eq!(count, 0);
        let mut tx = f.state.pool.begin().await.unwrap();
        assert!(chat::writable(&mut tx, cid).await.is_err());
        let c = chat::conversation(&mut tx, cid, f.users[0].0)
            .await
            .unwrap();
        assert_eq!(c["readOnly"], true);
        tx.rollback().await.unwrap();
        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM cloud_reference_sources")
            .fetch_one(&f.state.pool)
            .await
            .unwrap();
        assert_eq!(count, 1);
        f.close().await;
    }

    #[tokio::test]
    async fn delivery_read_set_preserves_unread_before_read_in_message_order() {
        let Some(f) = Fixture::new().await else {
            return;
        };
        let mut v = sample();
        v.messages[1].status = "committed".into();
        let mut earlier = v.messages[1].clone();
        earlier.id = Uuid::new_v4().to_string();
        earlier.sequence = "8".into();
        // Sequence 8 completed after sequence 9, and was not read locally.
        v.messages.push(earlier);
        v.list_state[0]
            .read_message_ids
            .push(v.messages[1].id.clone());
        let mapping = run(&f, v).await;
        let cid = uuid(&mapping.conversations[0].target_id).unwrap();
        let mut tx = f.state.pool.begin().await.unwrap();
        assert_eq!(
            chat::conversation(&mut tx, cid, f.users[0].0)
                .await
                .unwrap()["unreadCount"],
            1
        );
        let last = mapping
            .messages
            .iter()
            .map(|m| m.target_position.parse::<i64>().unwrap())
            .max()
            .unwrap();
        sqlx::query("UPDATE cloud_list SET read_position=$3 WHERE owner=$1 AND conversation=$2")
            .bind(f.users[0].0)
            .bind(cid)
            .bind(last)
            .execute(&mut *tx)
            .await
            .unwrap();
        assert_eq!(
            chat::conversation(&mut tx, cid, f.users[0].0)
                .await
                .unwrap()["unreadCount"],
            0
        );
        tx.commit().await.unwrap();
        f.close().await;
    }

    #[tokio::test]
    async fn read_only_import_rejects_http_mutations_but_allows_personal_list_changes() {
        let Some(f) = Fixture::new().await else {
            return;
        };
        let mut v = sample();
        v.conversations[0].legacy = true;
        let mapping = run(&f, v).await;
        let cid = &mapping.conversations[0].target_id;
        let mid = &mapping.messages[0].target_id;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let app = crate::router(f.state.clone(), f.state.attachments.clone());
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let client = reqwest::Client::new();
        for (method, path, payload) in [
            (
                reqwest::Method::POST,
                format!("chat/conversations/{cid}/messages"),
                json!({"requestId":Uuid::new_v4(),"text":"new"}),
            ),
            (
                reqwest::Method::PATCH,
                format!("chat/messages/{mid}"),
                json!({"requestId":Uuid::new_v4(),"text":"edit","expectedRevision":"1"}),
            ),
            (
                reqwest::Method::POST,
                format!("chat/messages/{mid}/withdraw"),
                json!({"requestId":Uuid::new_v4(),"expectedRevision":"1"}),
            ),
            (
                reqwest::Method::PUT,
                format!("chat/conversations/{cid}/participation"),
                json!({"requestId":Uuid::new_v4()}),
            ),
            (
                reqwest::Method::POST,
                format!("chat/conversations/{cid}/contact-intents"),
                json!({"requestId":Uuid::new_v4()}),
            ),
        ] {
            let response = client
                .request(method, format!("http://{address}/api/{path}"))
                .header("x-llmn-client", "1")
                .header("cookie", f.users[0].1.get("cookie").unwrap())
                .json(&payload)
                .send()
                .await
                .unwrap();
            assert_eq!(response.status(), reqwest::StatusCode::CONFLICT, "{path}");
        }
        let response = client
            .put(format!(
                "http://{address}/api/chat/conversations/{cid}/list"
            ))
            .header("x-llmn-client", "1")
            .header("cookie", f.users[0].1.get("cookie").unwrap())
            .json(&json!({"requestId":Uuid::new_v4(),"pinned":false,"expectedRevision":"1"}))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), reqwest::StatusCode::OK);
        server.abort();
        let _ = server.await;
        f.close().await;
    }
}
