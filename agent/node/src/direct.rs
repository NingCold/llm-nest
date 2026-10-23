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
    ensure!(
        bootstrap.messages.iter().all(|m| m.attachments.is_empty()),
        "direct_unsupported_history"
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
) -> Result<()> {
    let bootstrap = authorized(store, owner, cid)?;
    ensure!(
        !uuid::Uuid::parse_str(request)?.is_nil(),
        "invalid_request_id"
    );
    ensure!(
        !text.trim().is_empty() && text.len() <= 128 * 1024,
        "invalid_direct_text"
    );
    let fingerprint = format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&(
            owner,
            cid,
            text,
            input.as_ref().map(|m| (&m.id, &m.sealed))
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
        import_origin: None,
        sealed: None,
    };
    let mut human = input.unwrap_or_else(|| message(&bootstrap.human, text.into()));
    human.sender_name = bootstrap.human.name.clone();
    let mut answer = message(&bootstrap.ai, String::new());
    answer.reply_to = Some(human.id.clone());
    tx.execute("INSERT INTO direct_turns(request,cid,fingerprint,human,answer,state,payload) VALUES(?1,?2,?3,?4,?5,'queued',?6)",params![request,cid,fingerprint,serde_json::to_string(&human)?,serde_json::to_string(&answer)?,serde_json::to_string(&Execution::default())?])?;
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
    let make = |m: NodeMessage, status: String, run: Option<String>, usage, error| DirectMessage {
        id: m.id,
        sender_id: m.sender_id,
        text: m.text,
        sealed: m.sealed,
        status,
        created_at: m.created_at,
        reply_to: m.reply_to,
        run_id: run,
        usage,
        error,
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
            make(human, "committed".into(), None, None, None),
            make(
                answer,
                state,
                Some(request_id),
                execution.usage,
                execution.error,
            ),
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
