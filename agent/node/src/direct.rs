//! Durable same-device conversations; accepting history never starts a cloud model task.
use crate::store::Store;
use anyhow::{Result, ensure};
use connector::{direct::*, node::*};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{collections::HashSet, sync::Mutex};

#[derive(Default)]
pub struct Windows(Mutex<HashSet<String>>);
pub struct WindowGuard<'a>(&'a Windows, String);
impl Windows {
    pub fn enter(&self, cid: &str) -> Option<WindowGuard<'_>> {
        self.0
            .lock()
            .unwrap()
            .insert(cid.into())
            .then(|| WindowGuard(self, cid.into()))
    }
}
impl Drop for WindowGuard<'_> {
    fn drop(&mut self) {
        self.0.0.lock().unwrap().remove(&self.1);
    }
}

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct Execution {
    pub sources: Vec<SourceRevision>,
    pub derived: Vec<ScopedSource>,
    pub scopes: Vec<MaterialScope>,
    pub profile: String,
    pub usage: Option<NodeUsage>,
    pub error: Option<String>,
}

pub fn initialize(store: &Store) -> Result<()> {
    store.db.execute_batch("CREATE TABLE IF NOT EXISTS direct_windows(cid TEXT PRIMARY KEY,owner TEXT NOT NULL,ai TEXT NOT NULL,grant TEXT NOT NULL,bootstrap TEXT NOT NULL,state TEXT NOT NULL DEFAULT 'synced',error TEXT,sync_json TEXT);
CREATE TABLE IF NOT EXISTS direct_attachments(cid TEXT NOT NULL,message TEXT NOT NULL,revision TEXT NOT NULL,idx INTEGER NOT NULL,data BLOB NOT NULL,PRIMARY KEY(cid,message,revision,idx));
CREATE TABLE IF NOT EXISTS direct_references(cid TEXT NOT NULL,fingerprint TEXT NOT NULL,json TEXT NOT NULL,PRIMARY KEY(cid,fingerprint));
CREATE TABLE IF NOT EXISTS direct_turns(request TEXT PRIMARY KEY,cid TEXT NOT NULL,fingerprint TEXT NOT NULL,human TEXT NOT NULL,answer TEXT NOT NULL,state TEXT NOT NULL,payload TEXT NOT NULL,synced INTEGER NOT NULL DEFAULT 0);
UPDATE direct_turns SET state='interrupted',payload=json_set(payload,'$.error','node_restarted') WHERE state='running';")?;
    Ok(())
}
pub fn binding(store: &Store, cid: &str) -> Result<(String, String, String, DirectBootstrap)> {
    let (owner, ai, grant, json): (String, String, String, String) = store.db.query_row(
        "SELECT owner,ai,grant,bootstrap FROM direct_windows WHERE cid=?1",
        [cid],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
    )?;
    Ok((owner, ai, grant, serde_json::from_str(&json)?))
}
fn authorized(store: &Store, owner: &str, cid: &str) -> Result<DirectBootstrap> {
    let (actual, _, _, bootstrap) = binding(store, cid)?;
    ensure!(actual == owner, "direct_owner_mismatch");
    Ok(bootstrap)
}
pub fn cache(store: &Store, owner: &str, grant: &str, bootstrap: &DirectBootstrap) -> Result<()> {
    ensure!(bootstrap.human.id == owner, "direct_owner_mismatch");
    ensure!(
        bootstrap.conversation.kind == "direct",
        "direct_private_chat_required"
    );
    let pending: bool = store.db.query_row(
        "SELECT EXISTS(SELECT 1 FROM direct_turns WHERE cid=?1 AND synced=0)",
        [&bootstrap.conversation.id],
        |r| r.get(0),
    )?;
    ensure!(!pending, "direct_unsynced_history");
    store.db.execute("INSERT INTO direct_windows(cid,owner,ai,grant,bootstrap) VALUES(?1,?2,?3,?4,?5) ON CONFLICT(cid) DO UPDATE SET owner=excluded.owner,ai=excluded.ai,grant=excluded.grant,bootstrap=excluded.bootstrap,state='synced',error=NULL,sync_json=NULL",params![bootstrap.conversation.id,owner,bootstrap.ai.id,grant,serde_json::to_string(bootstrap)?])?;
    Ok(())
}
pub fn windows(store: &Store) -> Result<Vec<(String, String, String)>> {
    Ok(store
        .db
        .prepare("SELECT cid,owner,ai FROM direct_windows WHERE state!='conflict'")?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
        .collect::<rusqlite::Result<_>>()?)
}
pub fn pending(store: &Store) -> Result<Vec<(String, String, String)>> {
    Ok(store.db.prepare("SELECT t.request,t.cid,w.owner FROM direct_turns t JOIN direct_windows w ON w.cid=t.cid WHERE t.state='queued' AND w.state!='conflict' ORDER BY t.rowid")?.query_map([],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?)))?.collect::<rusqlite::Result<_>>()?)
}
pub fn turn(store: &Store, request: &str) -> Result<(NodeMessage, NodeMessage, Execution)> {
    let (h, a, p): (String, String, String) = store.db.query_row(
        "SELECT human,answer,payload FROM direct_turns WHERE request=?1",
        [request],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?;
    Ok((
        serde_json::from_str(&h)?,
        serde_json::from_str(&a)?,
        serde_json::from_str(&p)?,
    ))
}
pub fn send_message(
    store: &Store,
    owner: &str,
    cid: &str,
    request: &str,
    text: &str,
    input: Option<NodeMessage>,
    encrypted_attachments: &[connector::crypto::EncryptedAttachment],
) -> Result<()> {
    let bootstrap = authorized(store, owner, cid)?;
    ensure!(
        !uuid::Uuid::parse_str(request)?.is_nil(),
        "invalid_request_id"
    );
    ensure!(
        (!text.trim().is_empty() || input.as_ref().is_some_and(|m| !m.attachments.is_empty()))
            && text.len() <= 128 * 1024,
        "invalid_direct_text"
    );
    let fingerprint = format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&(
            owner,
            cid,
            text,
            input.as_ref().map(|m| (
                &m.id,
                &m.sealed,
                &m.reply_to,
                &m.context_reference,
                &m.attachments
            )),
            encrypted_attachments
        ))?)
    );
    let tx = store.db.unchecked_transaction()?;
    let previous: Option<String> = tx
        .query_row(
            "SELECT fingerprint FROM direct_turns WHERE request=?1",
            [request],
            |r| r.get(0),
        )
        .optional()?;
    if let Some(previous) = previous {
        ensure!(previous == fingerprint, "direct_request_reused");
        return Ok(());
    }
    let busy: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM direct_turns WHERE cid=?1 AND state IN ('queued','running'))",
        [cid],
        |r| r.get(0),
    )?;
    ensure!(!busy, "direct_conversation_busy");
    let state: String = tx.query_row(
        "SELECT state FROM direct_windows WHERE cid=?1",
        [cid],
        |r| r.get(0),
    )?;
    ensure!(state != "conflict", "direct_sync_conflict");
    let message = |sender: &NodeAccount, text: String| NodeMessage {
        id: crate::id(),
        position: "0".into(),
        sender_id: sender.id.clone(),
        sender_name: sender.name.clone(),
        reply_to: None,
        mentions: vec![],
        text,
        revision: "1".into(),
        created_at: chrono::Utc::now().to_rfc3339(),
        attachments: vec![],
        audience_revision: None,
        context_reference: None,
        import_origin: None,
        sealed: None,
        history_migration: None,
    };
    let mut human = input.unwrap_or_else(|| message(&bootstrap.human, text.into()));
    human.sender_name = bootstrap.human.name.clone();
    let mut answer = message(&bootstrap.ai, String::new());
    answer.reply_to = Some(human.id.clone());
    if bootstrap.conversation.encryption.is_some() {
        answer.audience_revision = Some(bootstrap.conversation.revision.clone());
    }
    tx.execute("INSERT INTO direct_turns(request,cid,fingerprint,human,answer,state,payload) VALUES(?1,?2,?3,?4,?5,'queued',?6)",params![request,cid,fingerprint,serde_json::to_string(&human)?,serde_json::to_string(&answer)?,serde_json::to_string(&Execution::default())?])?;
    use base64::Engine;
    for attachment in encrypted_attachments {
        let bytes = base64::engine::general_purpose::STANDARD.decode(&attachment.data)?;
        tx.execute("INSERT INTO direct_attachments VALUES(?1,?2,?3,?4,?5) ON CONFLICT(cid,message,revision,idx) DO NOTHING",params![cid,human.id,human.revision,attachment.index,bytes])?;
    }
    tx.execute(
        "UPDATE direct_windows SET state='pending',error=NULL WHERE cid=?1",
        [cid],
    )?;
    tx.commit()?;
    Ok(())
}
pub fn active(store: &Store, request: &str) -> Result<bool> {
    Ok(store.db.query_row(
        "SELECT state IN ('queued','running') FROM direct_turns WHERE request=?1",
        [request],
        |r| r.get(0),
    )?)
}
pub fn begin(store: &Store, request: &str, execution: &Execution) -> Result<()> {
    ensure!(store.db.execute("UPDATE direct_turns SET state='running',payload=?2 WHERE request=?1 AND state='queued'",params![request,serde_json::to_string(execution)?])?==1,"direct_turn_cancelled");
    Ok(())
}
pub fn save(
    store: &Store,
    request: &str,
    text: &str,
    state: &str,
    execution: &Execution,
    sealed: Option<connector::crypto::SealedEnvelope>,
) -> Result<()> {
    let (_, mut answer, _) = turn(store, request)?;
    answer.text = text.into();
    if sealed.is_some() {
        answer.sealed = sealed;
    }
    // A stop or permission invalidation wins over a late provider chunk.
    store.db.execute("UPDATE direct_turns SET answer=?2,state=?3,payload=?4 WHERE request=?1 AND state IN ('queued','running')",params![request,serde_json::to_string(&answer)?,state,serde_json::to_string(execution)?])?;
    Ok(())
}
pub fn stop(store: &Store, owner: &str, cid: &str) -> Result<()> {
    authorized(store, owner, cid)?;
    store.db.execute("UPDATE direct_turns SET state='interrupted',payload=json_set(payload,'$.error','user_stopped') WHERE cid=?1 AND state IN ('queued','running')",[cid])?;
    Ok(())
}
pub(crate) fn conflict(store: &Store, cid: &str, message: &str) -> Result<()> {
    let tx = store.db.unchecked_transaction()?;
    tx.execute(
        "UPDATE direct_windows SET state='conflict',error=?2 WHERE cid=?1",
        params![cid, message],
    )?;
    tx.execute("UPDATE direct_turns SET state='interrupted',payload=json_set(payload,'$.error','direct_sync_conflict') WHERE cid=?1 AND state IN ('queued','running')",[cid])?;
    tx.commit()?;
    Ok(())
}
pub fn view(store: &Store, owner: &str, cid: &str) -> Result<DirectView> {
    let bootstrap = authorized(store, owner, cid)?;
    let mut position = bootstrap
        .base
        .as_ref()
        .map(|h| h.position.parse::<u64>())
        .transpose()?
        .unwrap_or(0);
    let mut messages = bootstrap
        .messages
        .into_iter()
        .map(|message| DirectMessageView {
            status: bootstrap
                .message_statuses
                .get(&message.id)
                .cloned()
                .unwrap_or_else(|| "committed".into()),
            message,
            local: false,
            synced: true,
        })
        .collect::<Vec<_>>();
    let mut running = false;
    let mut statement = store.db.prepare(
        "SELECT human,answer,state FROM direct_turns WHERE cid=?1 AND synced=0 ORDER BY rowid",
    )?;
    for row in statement.query_map([cid], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
        ))
    })? {
        let (human, answer, state) = row?;
        let active = state == "queued" || state == "running";
        running |= active;
        for (json, status) in [
            (human, "committed".to_owned()),
            (answer, if active { "generating".into() } else { state }),
        ] {
            let mut message: NodeMessage = serde_json::from_str(&json)?;
            position += 1;
            message.position = position.to_string();

            messages.push(DirectMessageView {
                message,
                status,
                local: true,
                synced: false,
            });
        }
    }
    let (sync_state, error) = store.db.query_row(
        "SELECT state,error FROM direct_windows WHERE cid=?1",
        [cid],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    let has_more = bootstrap.has_more || messages.len() > 64;
    if messages.len() > 64 {
        messages.drain(..messages.len() - 64);
    }
    Ok(DirectView {
        ai: bootstrap.ai,
        human: bootstrap.human,
        conversation: bootstrap.conversation,
        messages,
        key_messages: crate::crypto::local_key_messages(store, owner, cid)?,
        has_more,
        sync_state,
        error,
        running,
    })
}

pub(crate) fn unfinished(store: &Store, cid: &str) -> Result<bool> {
    Ok(store.db.query_row(
        "SELECT EXISTS(SELECT 1 FROM direct_turns WHERE cid=?1 AND synced=0)",
        [cid],
        |r| r.get(0),
    )?)
}
fn unsynced_ids(store: &Store, cid: &str) -> Result<HashSet<String>> {
    let mut ids = HashSet::new();
    for r in store
        .db
        .prepare("SELECT human,answer FROM direct_turns WHERE cid=?1 AND synced=0")?
        .query_map([cid], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })?
    {
        let (human, answer) = r?;
        ids.insert(serde_json::from_str::<NodeMessage>(&human)?.id);
        ids.insert(serde_json::from_str::<NodeMessage>(&answer)?.id);
    }
    Ok(ids)
}
async fn validate_pending(store: &Store, client: &NodeClient, cid: &str, ai: &str) -> Result<()> {
    let local = unsynced_ids(store, cid)?;
    let payloads: Vec<String> = store
        .db
        .prepare("SELECT payload FROM direct_turns WHERE cid=?1 AND synced=0")?
        .query_map([cid], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    for payload in payloads {
        let execution: Execution = serde_json::from_str(&payload)?;
        ensure!(
            store.scopes_current(ai, &execution.scopes)?,
            "direct_material_changed"
        );
        let mut sources = execution.derived;
        sources.extend(
            execution
                .sources
                .into_iter()
                .filter(|s| !local.contains(&s.message_id))
                .map(|s| ScopedSource {
                    conversation_id: cid.into(),
                    message_id: s.message_id,
                    revision: s.revision,
                }),
        );
        if !sources.is_empty()
            && !client
                .validate(&ValidateSources {
                    sources,
                    conversations: vec![],
                })
                .await?
                .valid
        {
            anyhow::bail!("direct_source_changed");
        }
    }
    Ok(())
}

pub async fn sync(store: &Store, client: &NodeClient, cid: &str) -> Result<()> {
    let result = sync_inner(store, client, cid).await;
    if let Err(error) = &result {
        let busy = matches!(error.downcast_ref::<NodeError>(), Some(NodeError::Api { status: 409, body }) if serde_json::from_str::<serde_json::Value>(body).is_ok_and(|value| value["code"] == "conversation_busy"));
        if busy {
            store.db.execute(
                "UPDATE direct_windows SET error='云端正在回复，完成后自动刷新本机历史' WHERE cid=?1",
                [cid],
            )?;
            return Ok(());
        }
        let transient = error.downcast_ref::<NodeError>().is_some_and(|e| match e {
            NodeError::Api { status, .. } => *status >= 500 || *status == 429,
            _ => true,
        });
        if transient {
            store.db.execute(
                "UPDATE direct_windows SET error='暂时无法同步，聊天记录已保存在本机' WHERE cid=?1",
                [cid],
            )?;
        } else {
            conflict(
                store,
                cid,
                "云端权限、聊天或记忆已变化；本机记录已保留，尚未同步",
            )?;
        }
    }
    result
}
async fn sync_inner(store: &Store, client: &NodeClient, cid: &str) -> Result<()> {
    let (owner, ai, grant, bootstrap) = binding(store, cid)?;
    let frozen: Option<String> = store.db.query_row(
        "SELECT sync_json FROM direct_windows WHERE cid=?1",
        [cid],
        |r| r.get(0),
    )?;
    // A lost acceptance reply is retried verbatim before inspecting the now-changed cloud tail.
    if let Some(json) = frozen {
        let request: DirectSyncRequest = serde_json::from_str(&json)?;
        let mut receipt = client.direct_sync(&request).await?;
        crate::crypto::decrypt_receipt(client, store, &ai, cid, &mut receipt).await?;
        accept(store, cid, &request, receipt)?;
        return Ok(());
    }
    crate::consciousness::sync(client, store, &ai).await?;
    let mut current = client
        .direct_prepare(&DirectPrepare {
            grant: grant.clone(),
        })
        .await?;
    crate::crypto::decrypt_bootstrap(client, store, &mut current).await?;
    cache_bootstrap_downloads(client, store, &grant, &current).await?;
    if !unfinished(store, cid)? {
        return cache(store, &owner, &grant, &current);
    }
    ensure!(
        current.base == bootstrap.base
            && current.conversation.revision == bootstrap.conversation.revision
            && current.binding_version == bootstrap.binding_version,
        "direct_cloud_history_changed"
    );
    validate_pending(store, client, cid, &ai).await?;
    let candidate:Option<(String,String)>=store.db.query_row("SELECT request,state FROM direct_turns WHERE cid=?1 AND synced=0 ORDER BY rowid LIMIT 1",[cid],|r|Ok((r.get(0)?,r.get(1)?))).optional()?;
    let Some((request_id, state)) = candidate else {
        return Ok(());
    };
    if state == "queued" || state == "running" {
        return Ok(());
    }
    let (mut human, mut answer, execution) = turn(store, &request_id)?;
    if bootstrap.conversation.encryption.is_some() {
        crate::crypto::seal_direct_message(client, store, &ai, cid, &mut human, None).await?;
        crate::crypto::seal_direct_message(client, store, &ai, cid, &mut answer, Some(&request_id))
            .await?;
    }
    ensure!(
        store.scopes_current(&ai, &execution.scopes)?,
        "direct_material_changed"
    );
    let sources = execution
        .sources
        .into_iter()
        .filter(|s| s.message_id != human.id && s.message_id != answer.id)
        .collect();
    let make = |m: NodeMessage,
                status: String,
                run: Option<String>,
                usage,
                error|
     -> Result<DirectMessage> {
        Ok(DirectMessage {
            id: m.id.clone(),
            sender_id: m.sender_id.clone(),
            text: m.text.clone(),
            encrypted_attachments: if m.sealed.is_some() {
                attachment_payloads(store, cid, &m)?
            } else {
                vec![]
            },
            attachments: wire_attachments(&m),
            audience_revision: m.audience_revision,
            context_reference: m.context_reference,
            mentions: m.mentions,
            sealed: m.sealed,
            status,
            created_at: m.created_at,
            reply_to: m.reply_to,
            run_id: run,
            usage,
            error,
        })
    };
    let request = DirectSyncRequest {
        request_id: crate::id(),
        grant,
        conversation_revision: bootstrap.conversation.revision,
        base: bootstrap.base,
        sources,
        derived_sources: execution.derived,
        material_scopes: execution.scopes,
        configuration_revision: if execution.profile.is_empty() {
            "not-started".into()
        } else {
            execution.profile
        },
        messages: vec![
            make(human, "committed".into(), None, None, None)?,
            make(
                answer,
                state,
                Some(request_id),
                execution.usage,
                execution.error,
            )?,
        ],
    };
    store.db.execute(
        "UPDATE direct_windows SET sync_json=?2 WHERE cid=?1",
        params![cid, serde_json::to_string(&request)?],
    )?;
    let mut receipt = client.direct_sync(&request).await?;
    crate::crypto::decrypt_receipt(client, store, &ai, cid, &mut receipt).await?;
    accept(store, cid, &request, receipt)
}
fn accept(
    store: &Store,
    cid: &str,
    request: &DirectSyncRequest,
    receipt: DirectSyncReceipt,
) -> Result<()> {
    ensure!(
        receipt.messages.len() == 2
            && receipt
                .messages
                .iter()
                .zip(&request.messages)
                .all(|(a, b)| a.message.id == b.id),
        "direct_receipt_mismatch"
    );
    let (_, _, _, mut bootstrap) = binding(store, cid)?;
    let mut received = receipt.messages.into_iter();
    let human = received.next().unwrap();
    let answer = received.next().unwrap();
    // Never retain a stale body from a receipt retry after withdrawal.
    for item in [&human, &answer] {
        bootstrap.messages.retain(|m| m.id != item.message.id);
        bootstrap.message_statuses.remove(&item.message.id);
        if item.status != "withdrawn" {
            bootstrap
                .message_statuses
                .insert(item.message.id.clone(), item.status.clone());
            bootstrap.messages.push(item.message.clone());
        }
    }
    if bootstrap.messages.len() > 64 {
        bootstrap.has_more = true;
        bootstrap.messages.drain(..bootstrap.messages.len() - 64);
    }
    bootstrap
        .message_statuses
        .retain(|id, _| bootstrap.messages.iter().any(|m| m.id == *id));
    bootstrap.base = receipt.head;
    let tx = store.db.unchecked_transaction()?;
    tx.execute(
        "UPDATE direct_turns SET synced=1,human=?2,answer=?3,state=?4 WHERE request=?1",
        params![
            request.messages[1].run_id,
            serde_json::to_string(&human.message)?,
            serde_json::to_string(&answer.message)?,
            answer.status
        ],
    )?;
    let pending: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM direct_turns WHERE cid=?1 AND synced=0)",
        [cid],
        |r| r.get(0),
    )?;
    tx.execute(
        "UPDATE direct_windows SET bootstrap=?2,sync_json=NULL,state=?3,error=NULL WHERE cid=?1",
        params![
            cid,
            serde_json::to_string(&bootstrap)?,
            if pending { "pending" } else { "synced" }
        ],
    )?;
    tx.commit()?;
    Ok(())
}

pub fn wire_attachments(message: &NodeMessage) -> Vec<NodeAttachment> {
    message
        .attachments
        .iter()
        .map(|a| {
            if message.sealed.is_some() {
                NodeAttachment {
                    index: a.index,
                    kind: "encrypted".into(),
                    mime: "application/octet-stream".into(),
                    byte_length: a.byte_length,
                    name: None,
                    media_encryption_info: None,
                }
            } else {
                a.clone()
            }
        })
        .collect()
}
pub fn cache_attachment(
    store: &Store,
    cid: &str,
    message: &str,
    revision: &str,
    index: usize,
    data: &[u8],
) -> Result<()> {
    store.db.execute("INSERT INTO direct_attachments VALUES(?1,?2,?3,?4,?5) ON CONFLICT(cid,message,revision,idx) DO UPDATE SET data=excluded.data",params![cid,message,revision,index,data])?;
    Ok(())
}
pub fn attachment(
    store: &Store,
    cid: &str,
    message: &str,
    revision: &str,
    index: usize,
) -> Result<Vec<u8>> {
    Ok(store.db.query_row("SELECT data FROM direct_attachments WHERE cid=?1 AND message=?2 AND revision=?3 AND idx=?4",params![cid,message,revision,index],|r|r.get(0))?)
}
fn attachment_payloads(
    store: &Store,
    cid: &str,
    message: &NodeMessage,
) -> Result<Vec<connector::crypto::EncryptedAttachment>> {
    use base64::Engine;
    message
        .attachments
        .iter()
        .map(|a| {
            Ok(connector::crypto::EncryptedAttachment {
                index: a.index,
                data: base64::engine::general_purpose::STANDARD.encode(attachment(
                    store,
                    cid,
                    &message.id,
                    &message.revision,
                    a.index,
                )?),
            })
        })
        .collect()
}
fn reference_fingerprint(selection: &ReferenceSelection) -> Result<String> {
    Ok(serde_json::to_string(selection)?)
}
pub fn cache_reference(
    store: &Store,
    cid: &str,
    selection: &ReferenceSelection,
    reference: &NodeReference,
) -> Result<()> {
    store.db.execute("INSERT INTO direct_references VALUES(?1,?2,?3) ON CONFLICT(cid,fingerprint) DO UPDATE SET json=excluded.json",params![cid,reference_fingerprint(selection)?,serde_json::to_string(reference)?])?;
    Ok(())
}
pub fn reference(
    store: &Store,
    cid: &str,
    selection: &ReferenceSelection,
) -> Result<NodeReference> {
    let state: String = store.db.query_row(
        "SELECT state FROM direct_windows WHERE cid=?1",
        [cid],
        |r| r.get(0),
    )?;
    ensure!(state != "conflict", "direct_sync_conflict");
    let json: String = store.db.query_row(
        "SELECT json FROM direct_references WHERE cid=?1 AND fingerprint=?2",
        params![cid, reference_fingerprint(selection)?],
        |r| r.get(0),
    )?;
    Ok(serde_json::from_str(&json)?)
}
pub fn selected_reference(
    store: &Store,
    cid: &str,
    human: &NodeMessage,
) -> Result<Option<NodeReference>> {
    fn from_message(
        store: &Store,
        cid: &str,
        message: &NodeMessage,
        required: bool,
    ) -> Result<Option<NodeReference>> {
        let Some(r) = &message.context_reference else {
            return Ok(None);
        };
        ensure!(!r.stale, "direct_reference_stale");
        let mut reference = reference(
            store,
            cid,
            &ReferenceSelection {
                conversation_id: r.conversation_id.clone(),
                sources: r.sources.clone(),
            },
        )?;
        reference.message_id = message.id.clone();
        reference.revision = r.revision.clone();
        reference.required = required;
        Ok(Some(reference))
    }
    if human.context_reference.is_some() {
        return from_message(store, cid, human, true);
    }
    let (owner, _, _, _) = binding(store, cid)?;
    let messages = view(store, &owner, cid)?.messages;
    if let Some(reply) = &human.reply_to
        && let Some(message) = messages.iter().find(|m| m.message.id == *reply)
        && message.message.context_reference.is_some()
    {
        return from_message(store, cid, &message.message, true);
    }
    for message in messages
        .iter()
        .rev()
        .filter(|m| m.status == "committed")
        .take(12)
    {
        if message
            .message
            .context_reference
            .as_ref()
            .is_some_and(|r| !r.stale)
        {
            return from_message(store, cid, &message.message, false);
        }
    }
    Ok(None)
}
pub async fn cache_downloads(
    client: &NodeClient,
    store: &Store,
    ai: &str,
    grant: &str,
    cid: &str,
    messages: &[NodeMessage],
) -> Result<()> {
    for m in messages {
        for a in &m.attachments {
            let cached: bool = store.db.query_row("SELECT EXISTS(SELECT 1 FROM direct_attachments WHERE cid=?1 AND message=?2 AND revision=?3 AND idx=?4)",params![cid,m.id,m.revision,a.index],|r|r.get(0))?;
            if cached {
                continue;
            }
            let bytes = client
                .direct_attachment(
                    ai,
                    &DirectAttachmentRequest {
                        grant: grant.into(),
                        conversation_id: cid.into(),
                        message_id: m.id.clone(),
                        index: a.index,
                        revision: m.revision.clone(),
                    },
                )
                .await?;
            crate::crypto::validate_attachment_bytes(a, &bytes)?;
            cache_attachment(store, cid, &m.id, &m.revision, a.index, &bytes)?;
        }
    }
    Ok(())
}
pub async fn cache_bootstrap_downloads(
    client: &NodeClient,
    store: &Store,
    grant: &str,
    bootstrap: &DirectBootstrap,
) -> Result<()> {
    cache_downloads(
        client,
        store,
        &bootstrap.ai.id,
        grant,
        &bootstrap.conversation.id,
        &bootstrap.messages,
    )
    .await?;
    for reference in &bootstrap.references {
        cache_downloads(
            client,
            store,
            &bootstrap.ai.id,
            grant,
            &reference.conversation.id,
            &reference.messages,
        )
        .await?;
        cache_reference(
            store,
            &bootstrap.conversation.id,
            &ReferenceSelection {
                conversation_id: reference.conversation.id.clone(),
                sources: reference.sources.clone(),
            },
            reference,
        )?;
    }
    Ok(())
}

pub fn discard_reference(store: &Store, cid: &str, selection: &ReferenceSelection) -> Result<()> {
    store.db.execute(
        "DELETE FROM direct_references WHERE cid=?1 AND fingerprint=?2",
        params![cid, reference_fingerprint(selection)?],
    )?;
    Ok(())
}
/// The connected invalidation stream is authoritative for all locally prepared references.
pub fn invalidate_references(
    store: &Store,
    ai: &str,
    change: &ConsciousnessChange,
) -> Result<bool> {
    let local: bool = store.db.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='direct_references')",
        [],
        |r| r.get(0),
    )?;
    if !local {
        return Ok(false);
    }
    let rows: Vec<(String,String,String)> = store.db.prepare("SELECT r.cid,r.fingerprint,r.json FROM direct_references r JOIN direct_windows w ON w.cid=r.cid WHERE w.ai=?1")?.query_map([ai],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?)))?.collect::<rusqlite::Result<_>>()?;
    let mut affected = false;
    for (cid, fingerprint, json) in rows {
        let reference: NodeReference = serde_json::from_str(&json)?;
        if reference.conversation.id == change.conversation_id
            && (change.message_id.is_none()
                || reference
                    .sources
                    .iter()
                    .any(|s| Some(&s.message_id) == change.message_id.as_ref()))
        {
            store.db.execute(
                "DELETE FROM direct_references WHERE cid=?1 AND fingerprint=?2",
                params![cid, fingerprint],
            )?;
            affected = true;
        }
    }
    Ok(affected)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn fixture() -> (Store, DirectBootstrap) {
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        initialize(&store).unwrap();
        let bootstrap: DirectBootstrap=serde_json::from_value(json!({"ai":{"id":crate::id(),"name":"ai","bio":"","kind":"ai"},"human":{"id":crate::id(),"name":"human","bio":"","kind":"human"},"conversation":{"id":crate::id(),"kind":"direct","name":"chat","revision":"1","members":[]},"bindingVersion":"1","base":null,"messages":[],"hasMore":false})).unwrap();
        cache(&store, &bootstrap.human.id, "grant", &bootstrap).unwrap();
        (store, bootstrap)
    }
    #[tokio::test]
    async fn cloud_busy_waits_then_refreshes_cache_but_real_conflicts_stay_blocked() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let (store, mut previous) = fixture();
        let cid = previous.conversation.id.clone();
        for _ in 0..2 {
            let request = crate::id();
            send_message(
                &store,
                &previous.human.id,
                &cid,
                &request,
                "old local input",
                None,
                &[],
            )
            .unwrap();
            save(
                &store,
                &request,
                "old local answer",
                "committed",
                &Execution::default(),
                None,
            )
            .unwrap();
            let (human, answer, _) = turn(&store, &request).unwrap();
            previous.messages.extend([human, answer]);
            store
                .db
                .execute(
                    "UPDATE direct_turns SET synced=1 WHERE request=?1",
                    [&request],
                )
                .unwrap();
        }
        for (index, message) in previous.messages.iter_mut().enumerate() {
            message.position = (index + 1).to_string();
        }
        previous.base = Some(DirectHead {
            message_id: previous.messages.last().unwrap().id.clone(),
            revision: "1".into(),
            position: "4".into(),
        });
        cache(&store, &previous.human.id, "grant", &previous).unwrap();
        let old_turns: Vec<(String, String, String)> = store
            .db
            .prepare("SELECT request,human,answer FROM direct_turns ORDER BY rowid")
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        let mut current = previous.clone();
        for sender in [&current.human, &current.ai] {
            let mut message = previous.messages[0].clone();
            message.id = crate::id();
            message.sender_id = sender.id.clone();
            message.position = (current.messages.len() + 1).to_string();
            message.text = "new cloud history".into();
            current.messages.push(message);
        }
        current.base = Some(DirectHead {
            message_id: current.messages.last().unwrap().id.clone(),
            revision: "1".into(),
            position: "6".into(),
        });
        let mut changed = current.clone();
        changed.conversation.revision = "2".into();
        let changes = json!({"cursor":"0","hasMore":false,"changes":[]});
        let mut responses = vec![];
        for (status, body) in [
            (409, json!({"code":"conversation_busy"})),
            (200, serde_json::to_value(&current).unwrap()),
            (403, json!({"code":"forbidden"})),
            (409, json!({"code":"direct_conflict"})),
            (200, serde_json::to_value(&changed).unwrap()),
        ] {
            responses.push((200, changes.clone()));
            responses.push((status, body));
        }
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let api = NodeClient::new(
            &format!("http://{}", listener.local_addr().unwrap()),
            "fixture",
            None,
        )
        .unwrap();
        let server = tokio::spawn(async move {
            for (index, (status, body)) in responses.into_iter().enumerate() {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut bytes = vec![0; 16384];
                let size = socket.read(&mut bytes).await.unwrap();
                let request = String::from_utf8_lossy(&bytes[..size]);
                assert!(request.contains(if index % 2 == 0 {
                    "/node/consciousness/changes"
                } else {
                    "/node/direct/prepare"
                }));
                let body = body.to_string();
                socket.write_all(format!("HTTP/1.1 {status} Response\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).as_bytes()).await.unwrap();
            }
        });
        sync(&store, &api, &cid).await.unwrap();
        let waiting = view(&store, &previous.human.id, &cid).unwrap();
        assert_eq!(waiting.sync_state, "synced");
        assert!(waiting.error.unwrap().contains("云端正在回复"));
        for (ready, _, _) in windows(&store).unwrap() {
            sync(&store, &api, &ready).await.unwrap();
        }
        let refreshed = view(&store, &previous.human.id, &cid).unwrap();
        assert_eq!(refreshed.messages.len(), 6);
        assert!(refreshed.error.is_none());
        assert_eq!(refreshed.sync_state, "synced");
        let retained: Vec<(String, String, String)> = store
            .db
            .prepare("SELECT request,human,answer FROM direct_turns ORDER BY rowid")
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        assert_eq!(retained, old_turns);
        assert_eq!(
            store
                .db
                .query_row(
                    "SELECT (SELECT count(*) FROM model_reservations)+(SELECT count(*) FROM tasks)",
                    [],
                    |r| r.get::<_, i64>(0)
                )
                .unwrap(),
            0
        );
        for _ in 0..2 {
            assert!(sync(&store, &api, &cid).await.is_err());
            assert_eq!(
                view(&store, &previous.human.id, &cid).unwrap().sync_state,
                "conflict"
            );
            assert!(windows(&store).unwrap().is_empty());
            cache(&store, &previous.human.id, "grant", &current).unwrap();
        }
        let request = crate::id();
        send_message(
            &store,
            &previous.human.id,
            &cid,
            &request,
            "unsynced input",
            None,
            &[],
        )
        .unwrap();
        assert!(sync(&store, &api, &cid).await.is_err());
        assert!(unfinished(&store, &cid).unwrap());
        assert_eq!(
            view(&store, &previous.human.id, &cid).unwrap().sync_state,
            "conflict"
        );
        assert!(windows(&store).unwrap().is_empty());
        assert_eq!(
            store
                .db
                .query_row("SELECT count(*) FROM direct_turns", [], |r| r
                    .get::<_, i64>(0))
                .unwrap(),
            3
        );
        server.await.unwrap();
    }
    #[test]
    fn attachment_failure_rolls_back_turn_and_successful_retry_is_idempotent() {
        let (store, b) = fixture();
        let cid = &b.conversation.id;
        let request = crate::id();
        let message: NodeMessage=serde_json::from_value(json!({"id":crate::id(),"position":"0","senderId":b.human.id,"senderName":"human","replyTo":null,"mentions":[],"text":"file","revision":"1","createdAt":"2026-10-23T12:00:00Z","attachments":[{"index":0,"kind":"file","mime":"text/plain","byteLength":3,"name":"a.txt"}]})).unwrap();
        let payload = vec![connector::crypto::EncryptedAttachment {
            index: 0,
            data: "YWJj".into(),
        }];
        store.db.execute_batch("CREATE TEMP TRIGGER fail_blob BEFORE INSERT ON direct_attachments BEGIN SELECT RAISE(ABORT,'simulated disk failure'); END;").unwrap();
        assert!(
            send_message(
                &store,
                &b.human.id,
                cid,
                &request,
                "file",
                Some(message.clone()),
                &payload
            )
            .is_err()
        );
        assert_eq!(
            store
                .db
                .query_row("SELECT COUNT(*) FROM direct_turns", [], |r| r
                    .get::<_, usize>(0))
                .unwrap(),
            0
        );
        assert_eq!(
            store
                .db
                .query_row(
                    "SELECT state FROM direct_windows WHERE cid=?1",
                    [cid],
                    |r| r.get::<_, String>(0)
                )
                .unwrap(),
            "synced"
        );
        store.db.execute_batch("DROP TRIGGER fail_blob").unwrap();
        for _ in 0..2 {
            send_message(
                &store,
                &b.human.id,
                cid,
                &request,
                "file",
                Some(message.clone()),
                &payload,
            )
            .unwrap();
        }
        assert_eq!(
            store
                .db
                .query_row("SELECT COUNT(*) FROM direct_turns", [], |r| r
                    .get::<_, usize>(0))
                .unwrap(),
            1
        );
        assert_eq!(
            attachment(&store, cid, &message.id, "1", 0).unwrap(),
            b"abc"
        );
        let changed = vec![connector::crypto::EncryptedAttachment {
            index: 0,
            data: "eHl6".into(),
        }];
        assert!(
            send_message(
                &store,
                &b.human.id,
                cid,
                &request,
                "file",
                Some(message.clone()),
                &changed
            )
            .is_err()
        );
        assert_eq!(
            attachment(&store, cid, &message.id, "1", 0).unwrap(),
            b"abc"
        );
    }
    #[tokio::test]
    async fn busy_preserves_frozen_pending_sync_and_conflict_never_replays_it() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let (store, bootstrap) = fixture();
        let cid = &bootstrap.conversation.id;
        let request = crate::id();
        send_message(
            &store,
            &bootstrap.human.id,
            cid,
            &request,
            "pending input",
            None,
            &[],
        )
        .unwrap();
        save(
            &store,
            &request,
            "saved answer",
            "committed",
            &Execution::default(),
            None,
        )
        .unwrap();
        let (human, answer, _) = turn(&store, &request).unwrap();
        let wire = |message: NodeMessage, run: Option<&str>| json!({"id":message.id,"senderId":message.sender_id,"text":message.text,"status":"committed","createdAt":message.created_at,"replyTo":message.reply_to,"runId":run,"usage":null,"error":null});
        let frozen=json!({"requestId":crate::id(),"grant":"grant","conversationRevision":"1","base":null,"sources":[],"derivedSources":[],"materialScopes":[],"configurationRevision":"profile-1","messages":[wire(human,None),wire(answer,Some(&request))]}).to_string();
        store
            .db
            .execute(
                "UPDATE direct_windows SET sync_json=?2 WHERE cid=?1",
                params![cid, frozen],
            )
            .unwrap();
        let saved: (String, String, String, String, i64) = store
            .db
            .query_row(
                "SELECT state,human,answer,payload,synced FROM direct_turns WHERE request=?1",
                [&request],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )
            .unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let api = NodeClient::new(
            &format!("http://{}", listener.local_addr().unwrap()),
            "fixture",
            None,
        )
        .unwrap();
        let expected: serde_json::Value = serde_json::from_str(&frozen).unwrap();
        let server = tokio::spawn(async move {
            for code in ["conversation_busy", "direct_conflict"] {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut bytes = Vec::new();
                loop {
                    let mut chunk = [0; 4096];
                    let size = socket.read(&mut chunk).await.unwrap();
                    assert!(size > 0);
                    bytes.extend_from_slice(&chunk[..size]);
                    if let Some(offset) = bytes.windows(4).position(|v| v == b"\r\n\r\n") {
                        let header = String::from_utf8_lossy(&bytes[..offset]);
                        assert!(header.contains("/node/direct/sync"));
                        let length: usize = header
                            .lines()
                            .find_map(|line| {
                                line.to_lowercase()
                                    .strip_prefix("content-length: ")
                                    .map(str::to_owned)
                            })
                            .unwrap()
                            .parse()
                            .unwrap();
                        if bytes.len() >= offset + 4 + length {
                            assert_eq!(
                                serde_json::from_slice::<serde_json::Value>(
                                    &bytes[offset + 4..offset + 4 + length]
                                )
                                .unwrap(),
                                expected
                            );
                            break;
                        }
                    }
                }
                let body = json!({"code":code}).to_string();
                socket.write_all(format!("HTTP/1.1 409 Conflict\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).as_bytes()).await.unwrap();
            }
        });
        sync(&store, &api, cid).await.unwrap();
        assert_eq!(
            view(&store, &bootstrap.human.id, cid).unwrap().sync_state,
            "pending"
        );
        assert!(sync(&store, &api, cid).await.is_err());
        assert_eq!(
            view(&store, &bootstrap.human.id, cid).unwrap().sync_state,
            "conflict"
        );
        assert!(windows(&store).unwrap().is_empty());
        let retained: (String, String, String, String, i64) = store
            .db
            .query_row(
                "SELECT state,human,answer,payload,synced FROM direct_turns WHERE request=?1",
                [&request],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )
            .unwrap();
        assert_eq!(retained, saved);
        assert_eq!(
            store
                .db
                .query_row(
                    "SELECT sync_json FROM direct_windows WHERE cid=?1",
                    [cid],
                    |r| r.get::<_, String>(0)
                )
                .unwrap(),
            frozen
        );
        server.await.unwrap();
    }
    #[test]
    fn known_source_invalidations_remove_only_affected_offline_references() {
        let (store, b) = fixture();
        let source = crate::id();
        let mid = crate::id();
        let selection = ReferenceSelection {
            conversation_id: source.clone(),
            sources: vec![SourceRevision {
                message_id: mid.clone(),
                revision: "1".into(),
            }],
        };
        let reference = NodeReference {
            message_id: crate::id(),
            revision: "1".into(),
            required: true,
            conversation: NodeConversation {
                id: source.clone(),
                kind: "group".into(),
                name: "group".into(),
                revision: "1".into(),
                members: vec![],
                encryption: None,
            },
            messages: vec![],
            sources: selection.sources.clone(),
        };
        cache_reference(&store, &b.conversation.id, &selection, &reference).unwrap();
        let before = store.material_scope(&b.ai.id, &source).unwrap();
        store
            .invalidate(
                &b.ai.id,
                &ConsciousnessChanges {
                    cursor: "1".into(),
                    has_more: false,
                    changes: vec![ConsciousnessChange {
                        id: crate::id(),
                        conversation_id: source.clone(),
                        message_id: Some(crate::id()),
                        reason: "message_changed".into(),
                    }],
                },
            )
            .unwrap();
        assert!(super::reference(&store, &b.conversation.id, &selection).is_ok());
        store
            .invalidate(
                &b.ai.id,
                &ConsciousnessChanges {
                    cursor: "2".into(),
                    has_more: false,
                    changes: vec![ConsciousnessChange {
                        id: crate::id(),
                        conversation_id: source.clone(),
                        message_id: Some(mid),
                        reason: "message_changed".into(),
                    }],
                },
            )
            .unwrap();
        assert!(super::reference(&store, &b.conversation.id, &selection).is_err());
        assert_ne!(
            before.revision,
            store.material_scope(&b.ai.id, &source).unwrap().revision
        );
    }
}
