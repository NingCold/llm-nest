//! Endpoint-only crypto state and durable relay outbox.
use crate::store::{Binding, Config, Store};
use anyhow::{Result, ensure};
use base64::Engine;
use connector::{crypto::*, node::*};
use llmn_crypto::{Device, requests::Request};
use rand::RngCore;
use rusqlite::{OptionalExtension, params};
use serde_json::json;
use std::rc::Rc;
pub struct Session {
    pub device: Device,
    pub ai: String,
    router: std::cell::RefCell<Option<ai_client::ModelRouter>>,
}
fn initialize(store: &Store) -> Result<()> {
    store.db.execute_batch("CREATE TABLE IF NOT EXISTS crypto_devices(ai TEXT PRIMARY KEY,device TEXT NOT NULL,local_key BLOB NOT NULL,cursor TEXT NOT NULL DEFAULT '0',ready INTEGER NOT NULL DEFAULT 0);
 CREATE TABLE IF NOT EXISTS crypto_local_keys(ai TEXT NOT NULL,cid TEXT NOT NULL,request TEXT NOT NULL,id TEXT NOT NULL,json TEXT NOT NULL,PRIMARY KEY(ai,id));
 CREATE TABLE IF NOT EXISTS crypto_outbox(ai TEXT NOT NULL,id TEXT NOT NULL,json TEXT NOT NULL,PRIMARY KEY(ai,id));
 CREATE TABLE IF NOT EXISTS crypto_frozen(ai TEXT NOT NULL,id TEXT NOT NULL,json TEXT NOT NULL,PRIMARY KEY(ai,id));
 CREATE TABLE IF NOT EXISTS crypto_resource_conflicts(id TEXT PRIMARY KEY,json TEXT NOT NULL,reason TEXT NOT NULL);
 CREATE TABLE IF NOT EXISTS crypto_room_audience(room TEXT PRIMARY KEY,fingerprint TEXT NOT NULL);
 CREATE TABLE IF NOT EXISTS crypto_recovery_ready(ai TEXT PRIMARY KEY,version TEXT NOT NULL);
 CREATE TABLE IF NOT EXISTS crypto_handover(ai TEXT PRIMARY KEY,state TEXT NOT NULL);
 CREATE TABLE IF NOT EXISTS crypto_admins(ai TEXT NOT NULL,account TEXT NOT NULL,device TEXT NOT NULL,fingerprint TEXT,PRIMARY KEY(ai,account,device));
 CREATE TABLE IF NOT EXISTS crypto_control_outbox(ai TEXT NOT NULL,kind TEXT NOT NULL,json TEXT NOT NULL,file TEXT,PRIMARY KEY(ai,kind));
 CREATE TABLE IF NOT EXISTS crypto_resource_outbox(id TEXT PRIMARY KEY,json TEXT NOT NULL);
 CREATE TABLE IF NOT EXISTS crypto_window_activation(cid TEXT PRIMARY KEY,position TEXT NOT NULL);
 CREATE TABLE IF NOT EXISTS crypto_windows(cid TEXT PRIMARY KEY,ai TEXT NOT NULL,encrypted INTEGER NOT NULL);
 CREATE TABLE IF NOT EXISTS crypto_resources(id TEXT PRIMARY KEY,revision TEXT NOT NULL,status TEXT NOT NULL);")?;
    Ok(())
}
async fn session(store: &Store, ai: &str) -> Result<Rc<Session>> {
    if let Some(s) = store.crypto.borrow().get(ai).cloned() {
        return Ok(s);
    }
    initialize(store)?;
    let stored: Option<(String, Vec<u8>)> = store
        .db
        .query_row(
            "SELECT device,local_key FROM crypto_devices WHERE ai=?1",
            [ai],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    let (id, key) = if let Some(stored) = stored {
        stored
    } else {
        let id = crate::id();
        let mut key = vec![0; 32];
        rand::thread_rng().fill_bytes(&mut key);
        store.db.execute(
            "INSERT INTO crypto_devices(ai,device,local_key) VALUES(?1,?2,?3)",
            params![ai, id, key],
        )?;
        (id, key)
    };
    let database: String = store.db.query_row(
        "SELECT file FROM pragma_database_list WHERE name='main'",
        [],
        |r| r.get(0),
    )?;
    ensure!(
        !database.is_empty(),
        "crypto requires a persistent node directory"
    );
    let directory = std::path::Path::new(&database)
        .parent()
        .unwrap()
        .join("crypto")
        .join(ai);
    let device = Device::open(&directory, ai, &id, &key).await?;
    let session = Rc::new(Session {
        device,
        ai: ai.into(),
        router: Default::default(),
    });
    store.crypto.borrow_mut().insert(ai.into(), session.clone());
    Ok(session)
}
async fn relay(api: &NodeClient, store: &Store, s: &Session, request: Request) -> Result<()> {
    let json = serde_json::to_string(&request)?;
    store.db.execute(
        "INSERT INTO crypto_outbox VALUES(?1,?2,?3) ON CONFLICT(ai,id) DO NOTHING",
        params![s.ai, request.request_id, json],
    )?;
    let frozen: String = store.db.query_row(
        "SELECT json FROM crypto_outbox WHERE ai=?1 AND id=?2",
        params![s.ai, request.request_id],
        |r| r.get(0),
    )?;
    let request: Request = serde_json::from_str(&frozen)?;
    let response = api
        .crypto_request(&CryptoRequest {
            device_id: request.device_id.clone(),
            request_id: request.request_id.clone(),
            kind: request.kind.clone(),
            body: request.body.clone(),
        })
        .await?;
    s.device.acknowledge(&request, response).await?;
    store.db.execute(
        "DELETE FROM crypto_outbox WHERE ai=?1 AND id=?2",
        params![s.ai, request.request_id],
    )?;
    Ok(())
}
pub async fn sync(
    api: &NodeClient,
    store: &Store,
    config: &Config,
    binding: &Binding,
) -> Result<()> {
    initialize(store)?;
    let enrolled: bool = store.db.query_row(
        "SELECT EXISTS(SELECT 1 FROM crypto_devices WHERE ai=?1)",
        [&binding.ai],
        |r| r.get(0),
    )?;
    if !enrolled {
        return Ok(());
    }
    let s = session(store, &binding.ai).await?;
    s.router
        .replace(Some(ai_client::ModelRouter::new(&config.providers)?));
    let controls: Vec<String> = store
        .db
        .prepare("SELECT json FROM crypto_control_outbox WHERE ai=?1 AND kind='backup'")?
        .query_map([&binding.ai], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    for json in controls {
        api.crypto_save_backup(&serde_json::from_str(&json)?)
            .await?;
        store.db.execute(
            "DELETE FROM crypto_control_outbox WHERE ai=?1 AND kind='backup'",
            [&binding.ai],
        )?;
    }
    let keys = s
        .device
        .machine()
        .backup_machine()
        .get_backup_keys()
        .await?;
    if let (Some(key), Some(version)) = (keys.decryption_key, keys.backup_version) {
        let metadata = api.crypto_backup().await?;
        ensure!(
            s.device
                .machine()
                .backup_machine()
                .verify_backup(serde_json::from_value(metadata.clone())?, false)
                .await?
                .trusted(),
            "backup metadata signature is not trusted"
        );
        ensure!(
            metadata["version"] == version
                && metadata["auth_data"]["public_key"] == key.megolm_v1_public_key().to_base64(),
            "backup identity mismatch"
        );
        let public = key.megolm_v1_public_key();
        public.set_version(version);
        s.device
            .machine()
            .backup_machine()
            .enable_backup_v1(public)
            .await?;
    }
    api.crypto_register(s.device.machine().device_id().as_str(), "LLM Nest Agent")
        .await?;
    let queued: Vec<String> = store
        .db
        .prepare("SELECT json FROM crypto_outbox WHERE ai=?1 ORDER BY rowid")?
        .query_map([&binding.ai], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    for request in queued {
        relay(api, store, &s, serde_json::from_str(&request)?).await?;
    }
    for request in s.device.outgoing().await? {
        relay(api, store, &s, request).await?;
    }
    let cursor: String = store.db.query_row(
        "SELECT cursor FROM crypto_devices WHERE ai=?1",
        [&binding.ai],
        |r| r.get(0),
    )?;
    let response = api
        .crypto_sync(s.device.machine().device_id().as_str(), &cursor)
        .await?;
    let handover = response["llmn_handover_state"].as_str().unwrap_or("active");
    store.db.execute("INSERT INTO crypto_handover VALUES(?1,?2) ON CONFLICT(ai) DO UPDATE SET state=excluded.state",params![binding.ai,handover])?;
    let cursor = s.device.receive_sync(response).await?;
    store.db.execute(
        "UPDATE crypto_devices SET cursor=?2 WHERE ai=?1",
        params![binding.ai, cursor],
    )?;
    apply_resources(api, store, Some(config), &s, None).await?;
    let conversations = api.crypto_conversations().await?;
    let mut windows = Vec::with_capacity(conversations.len());
    for conversation in conversations {
        register_window(store, &conversation, &binding.ai)?;
        windows.push(conversation.id);
    }
    for cid in &windows {
        apply_resources(api, store, Some(config), &s, Some(cid)).await?;
    }
    // Close existing confirmations before acknowledging a completed resource upload.
    // Remote invalidations have already been applied; this never calls a provider.
    crate::consciousness::recover_summaries(api, store, &binding.ai).await?;
    for cid in windows {
        for mut record in store.summaries(&binding.ai, &cid)? {
            if record.item.status == "pending_sync" {
                let conflict: bool = store.db.query_row(
                    "SELECT EXISTS(SELECT 1 FROM crypto_resource_conflicts WHERE id=?1)",
                    [&record.item.id],
                    |r| r.get(0),
                )?;
                if conflict {
                    record.item.status = "sync_conflict".into();
                    record.finished = true;
                } else {
                    if let Err(error) =
                        publish_summary(api, store, &binding.ai, &cid, &record).await
                    {
                        if !store.db.query_row(
                            "SELECT EXISTS(SELECT 1 FROM crypto_resource_conflicts WHERE id=?1)",
                            [&record.item.id],
                            |r| r.get::<_, bool>(0),
                        )? {
                            return Err(error);
                        }
                        record.item.status = "sync_conflict".into();
                        record.finished = true;
                    } else {
                        record.item.status = "ready".into();
                        record.finished = true;
                    }
                }
                store.db.execute(
                    "UPDATE conscious_summaries SET json=?4 WHERE ai=?1 AND cid=?2 AND id=?3",
                    params![
                        binding.ai,
                        cid,
                        record.item.id,
                        serde_json::to_string(&record)?
                    ],
                )?;
            }
        }
    }
    let pending: Vec<(String, String)> = store
        .db
        .prepare("SELECT id,json FROM crypto_resource_outbox WHERE json_extract(json,'$.aiId')=?1")?
        .query_map([&binding.ai], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    for (id, json) in pending {
        match api
            .crypto_save_resource(&id, &serde_json::from_str(&json)?)
            .await
        {
            Ok(_) => {}
            Err(NodeError::Api {
                status: 400 | 403 | 409 | 410,
                ..
            }) => {
                store.db.execute("INSERT INTO crypto_resource_conflicts VALUES(?1,?2,'resource_conflict') ON CONFLICT(id) DO NOTHING",params![id,json])?;
            }
            Err(error) => return Err(error.into()),
        }
        store
            .db
            .execute("DELETE FROM crypto_resource_outbox WHERE id=?1", [id])?;
    }
    let profile_missing = api
        .crypto_resources(&s.ai, None, "profile")
        .await?
        .items
        .is_empty();
    let profile_conflict:bool=store.db.query_row("SELECT EXISTS(SELECT 1 FROM crypto_resource_conflicts WHERE json_extract(json,'$.aiId')=?1 AND json_extract(json,'$.kind')='profile')",[&s.ai],|r|r.get(0))?;
    if profile_missing && !profile_conflict {
        let id = crate::id();
        let resource = publish_resource(
            api,
            store,
            &s,
            None,
            "profile",
            &id,
            "0",
            serde_json::to_value(store.profile(&s.ai)?)?,
            vec![],
        )
        .await?;
        apply_resource(api, store, Some(config), &s, &resource).await?;
        store.db.execute("INSERT INTO crypto_resources VALUES(?1,?2,'applied') ON CONFLICT(id) DO UPDATE SET revision=excluded.revision,status='applied'",params![id,resource.revision])?;
        api.crypto_apply_resource(
            &id,
            &ApplyResource {
                device_id: s.device.machine().device_id().to_string(),
                revision: resource.revision,
                status: "applied".into(),
            },
        )
        .await?;
        store
            .db
            .execute("UPDATE crypto_devices SET ready=1 WHERE ai=?1", [&s.ai])?;
    }
    if let Some((id, request)) = s.device.machine().backup_machine().backup().await? {
        relay(
            api,
            store,
            &s,
            Request {
                device_id: s.device.machine().device_id().to_string(),
                request_id: id.to_string(),
                kind: "keysBackup".into(),
                body: json!({"version":request.version,"rooms":request.rooms}),
            },
        )
        .await?;
    }
    share_admin_history(api, store, &s).await?;
    if draining(store, &binding.ai)? {
        let local: bool = store.db.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='direct_turns')",
            [],
            |r| r.get(0),
        )?;
        if local {
            store.db.execute("UPDATE direct_turns SET state='interrupted',payload=json_set(payload,'$.error','node_handover') WHERE state='queued' AND cid IN (SELECT cid FROM direct_windows WHERE ai=?1)",[&binding.ai])?;
            for (cid, _, ai) in crate::direct::windows(store)? {
                if ai == binding.ai && crate::direct::unfinished(store, &cid)? {
                    crate::direct::sync(store, api, &cid).await?;
                }
            }
        }
        let pending:bool=store.db.query_row("SELECT EXISTS(SELECT 1 FROM tasks WHERE ai=?1 AND state IN ('intent','started','final')) OR EXISTS(SELECT 1 FROM crypto_outbox WHERE ai=?1) OR EXISTS(SELECT 1 FROM crypto_resource_outbox WHERE json_extract(json,'$.aiId')=?1) OR EXISTS(SELECT 1 FROM crypto_control_outbox WHERE ai=?1)",[&binding.ai],|r|r.get(0))?;
        let pending_local: bool = if local {
            store.db.query_row("SELECT EXISTS(SELECT 1 FROM direct_turns WHERE synced=0 AND cid IN (SELECT cid FROM direct_windows WHERE ai=?1))",[&binding.ai],|r|r.get(0))?
        } else {
            false
        };
        let pending_summaries:bool=store.db.query_row("SELECT EXISTS(SELECT 1 FROM conscious_summaries WHERE ai=?1 AND json_extract(json,'$.item.status') IN ('queued','running','pending_sync'))",[&binding.ai],|r|r.get(0))?;
        if !pending && !pending_local && !pending_summaries {
            let version = s
                .device
                .machine()
                .backup_machine()
                .get_backup_keys()
                .await?
                .backup_version
                .ok_or_else(|| anyhow::anyhow!("handover requires current backup"))?;
            api.crypto_handover_ready(
                s.device.machine().device_id().as_str(),
                &binding.version,
                &version,
            )
            .await?;
        }
    }
    Ok(())
}
pub fn ensure_ready(store: &Store, ai: &str) -> Result<()> {
    let ready = capable(store, ai)?;
    ensure!(
        ready,
        "encrypted device requires trusted authorization and current resource application"
    );
    Ok(())
}
async fn decrypt_message(
    api: &NodeClient,
    s: &Session,
    cid: &str,
    message: &mut NodeMessage,
) -> Result<()> {
    let Some(envelope) = &message.sealed else {
        return Ok(());
    };
    ensure!(
        envelope.version == 1
            && envelope.room_id == room_id(cid)
            && envelope.sender == user_id(&message.sender_id)
            && envelope.content["device_id"] == envelope.device_id,
        "sealed route mismatch"
    );
    let prefix = format!("${}.{}.", message.id, message.revision);
    let suffix = envelope
        .event_id
        .strip_prefix(&prefix)
        .ok_or_else(|| anyhow::anyhow!("sealed event id mismatch"))?;
    let sequence = if suffix == "final" {
        None
    } else {
        Some(suffix.parse::<u64>()?)
    };
    let event = s
        .device
        .open_envelope(&serde_json::to_value(envelope)?)
        .await?;
    let p = &event["content"];
    ensure!(
        p["version"] == 1
            && p["server"] == api.origin()
            && p["kind"]
                == if sequence.is_some() {
                    "delta"
                } else {
                    "message"
                }
            && p["sequence"] == json!(sequence)
            && p["conversationId"] == cid
            && p["messageId"] == message.id
            && p["senderId"] == message.sender_id
            && p["revision"] == message.revision
            && p["replyTo"] == json!(message.reply_to)
            && p["mentions"] == json!(message.mentions),
        "sealed content binding mismatch"
    );
    message.text = p["text"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("invalid sealed text"))?
        .into();
    Ok(())
}

async fn query_sender_identity(
    api: &NodeClient,
    store: &Store,
    s: &Session,
    envelope: &SealedEnvelope,
) -> Result<()> {
    let sender = llmn_crypto::ruma::UserId::parse(&envelope.sender)?;
    ensure!(
        sender.server_name().as_str() == "llmn",
        "invalid sender domain"
    );
    uuid::Uuid::parse_str(sender.localpart())?;
    if s.device
        .machine()
        .get_identity(&sender, None)
        .await?
        .is_none()
    {
        s.device
            .machine()
            .update_tracked_users([sender.as_ref()])
            .await?;
        for request in s.device.outgoing().await? {
            relay(api, store, s, request).await?;
        }
    }
    // Querying only supplies public keys. verify_envelope still requires the SDK
    // to verify their chain against the restored, previously signed identity.
    Ok(())
}
pub async fn decrypt_materials(
    api: &NodeClient,
    store: &Store,
    material: &mut TaskMaterials,
) -> Result<()> {
    initialize(store)?;
    register_window(store, &material.conversation, &material.ai.id)?;
    if material.conversation.encryption.is_none() {
        return Ok(());
    }
    ensure_ready(store, &material.ai.id)?;
    let s = session(store, &material.ai.id).await?;
    apply_resources(api, store, None, &s, Some(&material.conversation.id)).await?;
    prepare_room(
        api,
        store,
        &s,
        &room_id(&material.conversation.id),
        &material.conversation.members,
        true,
        Some(&material.conversation.revision),
    )
    .await?;
    for message in &mut material.messages {
        if message.position.parse::<u64>()?
            > material
                .conversation
                .encryption
                .as_ref()
                .unwrap()
                .activation_position
                .parse::<u64>()?
        {
            ensure!(
                message.sealed.is_some(),
                "encrypted window contains unsealed message"
            );
        }
        decrypt_message(api, &s, &material.conversation.id, message).await?;
    }
    if let Some(reference) = &mut material.reference {
        for message in &mut reference.messages {
            decrypt_message(api, &s, &reference.conversation.id, message).await?;
        }
    }
    Ok(())
}

/// Summary selection and provider input must use authenticated plaintext too.
pub async fn decrypt_history(
    api: &NodeClient,
    store: &Store,
    ai: &str,
    history: &mut NodeHistory,
) -> Result<()> {
    initialize(store)?;
    register_window(store, &history.conversation, ai)?;
    let Some(encryption) = &history.conversation.encryption else {
        return Ok(());
    };
    ensure_ready(store, ai)?;
    let activation = encryption.activation_position.parse::<u64>()?;
    let s = session(store, ai).await?;
    for message in &mut history.messages {
        if message.position.parse::<u64>()? > activation {
            ensure!(
                message.sealed.is_some(),
                "encrypted window contains unsealed message"
            );
        }
        if let Some(envelope) = &message.sealed {
            query_sender_identity(api, store, &s, envelope).await?;
        }
        decrypt_message(api, &s, &history.conversation.id, message).await?;
    }
    Ok(())
}
#[allow(clippy::too_many_arguments)]
async fn freeze(
    api: &NodeClient,
    store: &Store,
    ai: &str,
    cid: &str,
    id: &str,
    revision: &str,
    kind: &str,
    text: &str,
    run: Option<&str>,
    sequence: Option<u64>,
    reply: Option<&str>,
    mentions: &[String],
) -> Result<SealedEnvelope> {
    let key = format!(
        "{id}.{revision}.{}",
        sequence
            .map(|n| n.to_string())
            .unwrap_or_else(|| "final".into())
    );
    let prior: Option<String> = store
        .db
        .query_row(
            "SELECT json FROM crypto_frozen WHERE ai=?1 AND id=?2",
            params![ai, key],
            |r| r.get(0),
        )
        .optional()?;
    if let Some(prior) = prior {
        return Ok(serde_json::from_str(&prior)?);
    }
    let s = session(store, ai).await?;
    let p = json!({"version":1,"server":api.origin(),"kind":kind,"conversationId":cid,"messageId":id,"senderId":ai,"revision":revision,"text":text,"replyTo":reply,"mentions":mentions,"runId":run,"sequence":sequence});
    let envelope = SealedEnvelope {
        version: 1,
        room_id: room_id(cid),
        sender: user_id(ai),
        device_id: s.device.machine().device_id().to_string(),
        event_id: format!("${key}"),
        content: s.device.seal(cid, "com.llmn.content.v1", &p).await?,
        device_keys: serde_json::Value::Null,
        signatures: serde_json::Value::Null,
    };
    let envelope = sign(s.as_ref(), envelope).await?;
    store.db.execute(
        "INSERT INTO crypto_frozen VALUES(?1,?2,?3)",
        params![ai, key, serde_json::to_string(&envelope)?],
    )?;
    Ok(envelope)
}
pub async fn seal_result(
    api: &NodeClient,
    store: &Store,
    cid: &str,
    revision: &str,
    result: &mut ResultRequest,
) -> Result<()> {
    initialize(store)?;
    let ai: Option<String> = store
        .db
        .query_row(
            "SELECT ai FROM crypto_windows WHERE cid=?1 AND encrypted=1",
            [cid],
            |r| r.get(0),
        )
        .optional()?;
    let Some(ai) = ai else {
        return Ok(());
    };
    result.sealed = Some(
        freeze(
            api,
            store,
            &ai,
            cid,
            &result.message_id,
            revision,
            "message",
            &result.text,
            Some(&result.run_id),
            None,
            None,
            &[],
        )
        .await?,
    );
    result.text.clear();
    Ok(())
}
pub async fn seal_delta(
    api: &NodeClient,
    store: &Store,
    cid: &str,
    revision: &str,
    frame: &mut ClientFrame,
) -> Result<()> {
    initialize(store)?;
    let ai: Option<String> = store
        .db
        .query_row(
            "SELECT ai FROM crypto_windows WHERE cid=?1 AND encrypted=1",
            [cid],
            |r| r.get(0),
        )
        .optional()?;
    let Some(ai) = ai else {
        return Ok(());
    };
    let key = match frame {
        ClientFrame::Delta {
            run_id, sequence, ..
        } => format!("frame:{run_id}:{sequence}"),
        _ => return Ok(()),
    };
    let prior: Option<String> = store
        .db
        .query_row(
            "SELECT json FROM crypto_frozen WHERE ai=?1 AND id=?2",
            params![ai, key],
            |r| r.get(0),
        )
        .optional()?;
    if let Some(prior) = prior {
        *frame = serde_json::from_str(&prior)?;
        return Ok(());
    }
    if let ClientFrame::Delta {
        run_id,
        message_id,
        sequence,
        text,
        sealed,
        ..
    } = frame
    {
        *sealed = Some(
            freeze(
                api,
                store,
                &ai,
                cid,
                message_id,
                revision,
                "delta",
                text,
                Some(run_id),
                Some(*sequence),
                None,
                &[],
            )
            .await?,
        );
        text.clear();
    }
    store.db.execute(
        "INSERT INTO crypto_frozen VALUES(?1,?2,?3)",
        params![ai, key, serde_json::to_string(frame)?],
    )?;
    Ok(())
}
async fn apply_resources(
    api: &NodeClient,
    store: &Store,
    config: Option<&Config>,
    s: &Session,
    cid: Option<&str>,
) -> Result<()> {
    let kinds = if cid.is_some() {
        vec!["memory", "summary", "summarySettings"]
    } else {
        vec!["profile"]
    };
    for kind in kinds {
        let mut resources = api.crypto_resources(&s.ai, cid, kind).await?.items;
        resources.sort_by_key(|r| r.tombstone.is_none());
        for resource in resources {
            let previous: Option<(String, String)> = store
                .db
                .query_row(
                    "SELECT revision,status FROM crypto_resources WHERE id=?1",
                    [&resource.id],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()?;
            if previous.as_ref().is_some_and(|(rev, state)| {
                rev == &resource.revision
                    && matches!(state.as_str(), "applied" | "forgotten" | "stale")
            }) {
                let binding: String = store.db.query_row(
                    "SELECT version FROM bindings WHERE ai=?1",
                    [&s.ai],
                    |r| r.get(0),
                )?;
                if !resource.applied.as_ref().is_some_and(|ack| {
                    ack.binding_version == binding
                        && ack.revision == resource.revision
                        && ack.status == "applied"
                }) {
                    api.crypto_apply_resource(
                        &resource.id,
                        &ApplyResource {
                            device_id: s.device.machine().device_id().to_string(),
                            revision: resource.revision.clone(),
                            status: "applied".into(),
                        },
                    )
                    .await?;
                }
                continue;
            }
            if let Some((revision, status)) = &previous {
                ensure!(
                    resource.revision.parse::<u64>()? >= revision.parse::<u64>()?,
                    "resource revision rollback"
                );
                ensure!(
                    !(status == "forgotten" || status == "stale") || resource.tombstone.is_some(),
                    "resource tombstone cannot be resurrected"
                );
            }
            let result = apply_resource(api, store, config, s, &resource).await;
            let status = if result.is_ok() {
                "applied"
            } else {
                "rejected"
            };
            store.db.execute("INSERT INTO crypto_resources VALUES(?1,?2,?3) ON CONFLICT(id) DO UPDATE SET revision=excluded.revision,status=excluded.status",params![resource.id,resource.revision,if result.is_ok(){resource.tombstone.as_deref().unwrap_or(status)}else{status}])?;
            if kind == "profile" {
                store.db.execute(
                    "UPDATE crypto_devices SET ready=?2 WHERE ai=?1",
                    params![s.ai, result.is_ok()],
                )?;
            }
            api.crypto_apply_resource(
                &resource.id,
                &ApplyResource {
                    device_id: s.device.machine().device_id().to_string(),
                    revision: resource.revision.clone(),
                    status: status.into(),
                },
            )
            .await?;
            result?;
        }
    }
    Ok(())
}
async fn apply_resource(
    api: &NodeClient,
    store: &Store,
    config: Option<&Config>,
    s: &Session,
    r: &Resource,
) -> Result<()> {
    ensure!(r.ai_id == s.ai, "resource actor mismatch");
    if let Some(tombstone) = &r.tombstone {
        if let Some(cid) = &r.conversation_id {
            let table = match r.kind.as_str() {
                "memory" => "conscious_memory",
                "summary" => "conscious_summaries",
                _ => anyhow::bail!("invalid resource tombstone"),
            };
            let sql = if r.kind == "memory" {
                format!(
                    "UPDATE {table} SET json=json_set(json,'$.text','','$.status',?4) WHERE ai=?1 AND cid=?2 AND id=?3"
                )
            } else {
                format!(
                    "UPDATE {table} SET json=json_set(json,'$.item.text','','$.item.status',?4) WHERE ai=?1 AND cid=?2 AND id=?3"
                )
            };
            store
                .db
                .execute(&sql, params![s.ai, cid, r.id, tombstone])?;
            store.db.execute(
                "UPDATE conscious_scopes SET epoch=?3 WHERE ai=?1 AND cid=?2",
                params![s.ai, cid, crate::id()],
            )?;
        }
        return Ok(());
    }
    let envelope = r
        .sealed
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("resource is not sealed"))?;
    ensure!(
        envelope.room_id == resource_room(&s.ai, &r.kind, r.conversation_id.as_deref()),
        "resource room mismatch"
    );
    query_sender_identity(api, store, s, envelope).await?;
    let decrypted = s
        .device
        .open_envelope(&serde_json::to_value(envelope)?)
        .await?;
    let p = &decrypted["content"];
    let scope = format!(
        "{}.{}.{}",
        s.ai,
        r.kind,
        r.conversation_id.as_deref().unwrap_or("global")
    );
    ensure!(
        p["server"] == api.origin()
            && p["kind"] == "resource"
            && p["scope"] == scope
            && p["id"] == r.id
            && p["revision"] == r.revision,
        "resource payload mismatch"
    );
    ensure!(
        p["version"] == 1
            && p["messageId"] == r.id
            && p["conversationId"] == json!(r.conversation_id)
            && p["senderId"]
                .as_str()
                .is_some_and(|id| user_id(id) == envelope.sender)
            && p["expectedRevision"]
                .as_str()
                .and_then(|n| n.parse::<u64>().ok())
                .is_some_and(|n| (n + 1).to_string() == r.revision)
            && p["expectedAudienceRevision"]
                .as_str()
                .is_some_and(|value| !value.is_empty())
            && envelope.event_id == format!("${}.{}.final", r.id, r.revision),
        "resource authenticated metadata mismatch"
    );
    let data = p["data"].clone();
    match r.kind.as_str() {
        "profile" => {
            let mut profile: crate::store::Profile = serde_json::from_value(data)?;
            profile.revision = r.revision.clone();
            crate::management::validate(
                config.ok_or_else(|| anyhow::anyhow!("profile requires configured validation"))?,
                &profile,
            )?;
            store.db.execute(
                "UPDATE profiles SET json=?2 WHERE ai=?1",
                params![s.ai, serde_json::to_string(&profile)?],
            )?;
        }
        "memory" => {
            let mut item: MemoryItem = serde_json::from_value(data)?;
            ensure!(item.id == r.id, "memory id mismatch");
            item.revision = r.revision.clone();
            store.db.execute("INSERT INTO conscious_memory VALUES(?1,?2,?3,?4) ON CONFLICT(ai,cid,id) DO UPDATE SET json=excluded.json",params![s.ai,r.conversation_id,r.id,serde_json::to_string(&item)?])?;
        }
        "summarySettings" => {
            let mut settings: SummarySettings = serde_json::from_value(data)?;
            settings.revision = r.revision.clone();
            ensure!(
                !settings.enabled || settings.binding.is_some(),
                "summary_model_required"
            );
            if let Some(binding) = &settings.binding {
                let router = s.router.borrow();
                crate::consciousness::summary_options(
                    router
                        .as_ref()
                        .ok_or_else(|| anyhow::anyhow!("model configuration is not loaded"))?,
                    binding,
                )?;
            }
            store.db.execute("INSERT INTO conscious_settings VALUES(?1,?2,?3) ON CONFLICT(ai,cid) DO UPDATE SET json=excluded.json",params![s.ai,r.conversation_id,serde_json::to_string(&settings)?])?;
        }
        "summary" => {
            let item: SummaryItem = serde_json::from_value(data["item"].clone())?;
            ensure!(item.id == r.id, "summary id mismatch");
            let cid = r
                .conversation_id
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("summary scope missing"))?;
            let record = crate::consciousness::SummaryRecord {
                item,
                binding: serde_json::from_value(data["provenance"]["binding"].clone())?,
                settings_revision: data["provenance"]["settingsRevision"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("summary settings provenance missing"))?
                    .into(),
                conversation_revision: data["provenance"]["conversationRevision"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("summary conversation provenance missing"))?
                    .into(),
                epoch: store.material_scope(&s.ai, cid)?.revision,
                requested_by: data["provenance"]["requestedBy"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("summary requester missing"))?
                    .into(),
                start_request: String::new(),
                intent: false,
                confirmation: None,
                finished: true,
            };
            store.db.execute("INSERT INTO conscious_summaries VALUES(?1,?2,?3,?4) ON CONFLICT(ai,cid,id) DO UPDATE SET json=excluded.json",params![s.ai,r.conversation_id,r.id,serde_json::to_string(&record)?])?;
        }
        _ => anyhow::bail!("unsupported resource kind"),
    }
    if let Some(cid) = &r.conversation_id {
        store.material_scope(&s.ai, cid)?;
        store.db.execute(
            "UPDATE conscious_scopes SET epoch=?3 WHERE ai=?1 AND cid=?2",
            params![s.ai, cid, crate::id()],
        )?;
    }
    Ok(())
}

pub fn capable(store: &Store, ai: &str) -> Result<bool> {
    initialize(store)?;
    Ok(store
        .db
        .query_row("SELECT ready AND EXISTS(SELECT 1 FROM crypto_recovery_ready WHERE ai=?1) FROM crypto_devices WHERE ai=?1", [ai], |r| {
            r.get(0)
        })
        .optional()?
        .unwrap_or(false))
}
async fn prepare_room(
    api: &NodeClient,
    store: &Store,
    s: &Session,
    room: &str,
    members: &[NodeAccount],
    require_all: bool,
    audience_revision: Option<&str>,
) -> Result<()> {
    use llmn_crypto::matrix_sdk_crypto::{CollectStrategy, EncryptionSettings};
    let users = members
        .iter()
        .map(|m| llmn_crypto::ruma::UserId::parse(user_id(&m.id)))
        .collect::<Result<Vec<_>, _>>()?;
    s.device
        .machine()
        .update_tracked_users(users.iter().map(|u| u.as_ref()))
        .await?;
    for request in s.device.outgoing().await? {
        relay(api, store, s, request).await?;
    }
    for user in &users {
        // The SDK deliberately excludes this machine from is_any_verified().
        if user == s.device.machine().user_id() {
            continue;
        }
        if !require_all && user.as_str() != user_id(&s.ai) {
            continue;
        }
        ensure!(
            s.device
                .machine()
                .get_user_devices(user, None)
                .await?
                .is_any_verified(),
            "recipient has no trusted device"
        );
    }
    if let Some((id, r)) = s
        .device
        .machine()
        .get_missing_sessions(users.iter().map(|u| u.as_ref()))
        .await?
    {
        relay(api,store,s,Request {device_id:s.device.machine().device_id().to_string(),request_id:id.to_string(),kind:"keysClaim".into(),body:json!({"one_time_keys":r.one_time_keys,"timeout":r.timeout.map(|d|d.as_millis() as u64)})}).await?;
    }
    let mut devices = Vec::new();
    for user in &users {
        for device in s
            .device
            .machine()
            .get_user_devices(user, None)
            .await?
            .devices()
        {
            if device.is_verified() && !device.is_deleted() {
                devices.push(format!(
                    "{}:{}:{:?}:{:?}",
                    user,
                    device.device_id(),
                    device.ed25519_key(),
                    device.curve25519_key()
                ));
            }
        }
    }
    devices.sort();
    let fingerprint = serde_json::to_string(&(audience_revision, &users, devices))?;
    let previous: Option<String> = store
        .db
        .query_row(
            "SELECT fingerprint FROM crypto_room_audience WHERE room=?1",
            [room],
            |r| r.get(0),
        )
        .optional()?;
    if require_all || previous.as_ref().is_some_and(|old| old != &fingerprint) {
        s.device
            .machine()
            .discard_room_key(&llmn_crypto::ruma::RoomId::parse(room)?)
            .await?;
    }
    store.db.execute("INSERT INTO crypto_room_audience VALUES(?1,?2) ON CONFLICT(room) DO UPDATE SET fingerprint=excluded.fingerprint",params![room,fingerprint])?;
    let settings = EncryptionSettings {
        sharing_strategy: CollectStrategy::OnlyTrustedDevices,
        ..Default::default()
    };
    let room = llmn_crypto::ruma::RoomId::parse(room)?;
    for request in s
        .device
        .machine()
        .share_room_key(&room, users.iter().map(|u| u.as_ref()), settings)
        .await?
    {
        relay(
            api,
            store,
            s,
            Request {
                device_id: s.device.machine().device_id().to_string(),
                request_id: request.txn_id.to_string(),
                kind: "toDevice".into(),
                body: serde_json::to_value(&*request)?,
            },
        )
        .await?;
    }
    Ok(())
}
/// Explicit first identity creation. Refuses to replace an existing cloud identity.
pub async fn enroll(
    api: &NodeClient,
    store: &Store,
    config: &Config,
    binding: &Binding,
) -> Result<serde_json::Value> {
    let s = session(store, &binding.ai).await?;
    api.crypto_register(s.device.machine().device_id().as_str(), "LLM Nest Agent")
        .await?;
    let own = user_id(&binding.ai);
    let existing = api
        .crypto_request(&CryptoRequest {
            device_id: s.device.machine().device_id().to_string(),
            request_id: crate::id(),
            kind: "keysQuery".into(),
            body: json!({"device_keys":{own.clone():[]}}),
        })
        .await?;
    ensure!(
        existing["master_keys"].get(&own).is_none(),
        "existing AI encryption identity requires trusted-device recovery"
    );
    bootstrap_device(api, store, &s).await?;
    let mut bytes = [0; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    let key = llmn_crypto::matrix_sdk_crypto::store::types::BackupDecryptionKey::from_bytes(&bytes);
    let backup = s.device.machine().backup_machine();
    let mut info = key.to_backup_info();
    backup.sign_backup(&mut info).await?;
    let mut metadata = serde_json::to_value(info)?;
    let version = crate::id();
    metadata["version"] = json!(version);
    metadata["deviceId"] = json!(s.device.machine().device_id());
    metadata["expectedVersion"] = serde_json::Value::Null;
    backup
        .save_decryption_key(Some(key.clone()), Some(version.clone()))
        .await?;
    store.db.execute("INSERT INTO crypto_control_outbox(ai,kind,json) VALUES(?1,'backup',?2) ON CONFLICT(ai,kind) DO NOTHING",params![binding.ai,metadata.to_string()])?;
    api.crypto_save_backup(&metadata).await?;
    store.db.execute(
        "DELETE FROM crypto_control_outbox WHERE ai=?1 AND kind='backup'",
        [&binding.ai],
    )?;
    let public = key.megolm_v1_public_key();
    public.set_version(version);
    backup.enable_backup_v1(public).await?;
    let _ = config;
    status(store, &binding.ai).await
}
pub async fn status(store: &Store, ai: &str) -> Result<serde_json::Value> {
    let s = session(store, ai).await?;
    let keys = s.device.machine().identity_keys();
    let own = s
        .device
        .machine()
        .get_identity(s.device.machine().user_id(), None)
        .await?;
    let master = match own {
        Some(llmn_crypto::matrix_sdk_crypto::UserIdentity::Own(i)) => {
            serde_json::to_value(i.master_key())?
        }
        _ => serde_json::Value::Null,
    };
    Ok(
        json!({"deviceId":s.device.machine().device_id(),"ed25519":keys.ed25519.to_base64(),"curve25519":keys.curve25519.to_base64(),"master":master,"ready":capable(store,ai)?}),
    )
}
pub async fn export_recovery(
    api: &NodeClient,
    store: &Store,
    ai: &str,
    path: &std::path::Path,
) -> Result<()> {
    use std::io::Write;
    let s = session(store, ai).await?;
    let pending: Option<(String, String)> = store
        .db
        .query_row(
            "SELECT json,file FROM crypto_control_outbox WHERE ai=?1 AND kind='recovery'",
            [ai],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    let (data, file) = if let Some((data, file)) = pending {
        (
            serde_json::from_str::<serde_json::Value>(&data)?,
            serde_json::from_str::<serde_json::Value>(&file)?,
        )
    } else {
        let previous = match api.crypto_recovery().await {
            Ok(value) => value
                .get("version")
                .cloned()
                .unwrap_or(serde_json::Value::Null),
            Err(NodeError::Api { status: 404, .. }) => serde_json::Value::Null,
            Err(e) => return Err(e.into()),
        };
        let (ciphertext, info) = s.device.export_recovery().await?;
        let version = crate::id();
        let data = json!({"version":version,"deviceId":s.device.machine().device_id(),"expectedVersion":previous,"ciphertext":base64::engine::general_purpose::STANDARD.encode(ciphertext)});
        let file = json!({"format":"llmn-recovery-v1","version":version,"server":api.origin(),"accountId":ai,"mediaEncryptionInfo":serde_json::to_string(&info)?});
        store.db.execute(
            "INSERT INTO crypto_control_outbox(ai,kind,json,file) VALUES(?1,'recovery',?2,?3)",
            params![ai, data.to_string(), file.to_string()],
        )?;
        (data, file)
    };
    if path.exists() {
        ensure!(
            serde_json::from_slice::<serde_json::Value>(&std::fs::read(path)?)? == file,
            "recovery target already exists with different contents"
        );
    } else {
        let mut options = std::fs::OpenOptions::new();
        options.create_new(true).write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut output = options.open(path)?;
        output.write_all(&serde_json::to_vec_pretty(&file)?)?;
        output.sync_all()?;
    }
    api.crypto_save_recovery(&data).await?;
    store.db.execute("INSERT INTO crypto_recovery_ready VALUES(?1,?2) ON CONFLICT(ai) DO UPDATE SET version=excluded.version",params![ai,data["version"].as_str().unwrap()])?;
    store.db.execute(
        "DELETE FROM crypto_control_outbox WHERE ai=?1 AND kind='recovery'",
        [ai],
    )?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub async fn trust(
    api: &NodeClient,
    store: &Store,
    ai: &str,
    account: &str,
    device: &str,
    ed: &str,
    curve: &str,
    master: &str,
) -> Result<()> {
    let s = session(store, ai).await?;
    let user = llmn_crypto::ruma::UserId::parse(user_id(account))?;
    s.device
        .machine()
        .update_tracked_users([user.as_ref()])
        .await?;
    for request in s.device.outgoing().await? {
        relay(api, store, &s, request).await?;
    }
    let target = s
        .device
        .machine()
        .get_device(&user, device.into(), None)
        .await?
        .ok_or_else(|| anyhow::anyhow!("unknown device"))?;
    ensure!(
        target.ed25519_key().is_some_and(|k| k.to_base64() == ed)
            && target
                .curve25519_key()
                .is_some_and(|k| k.to_base64() == curve)
            && target.is_cross_signed_by_owner(),
        "device fingerprint or cross-signing chain mismatch"
    );
    let identity = s
        .device
        .machine()
        .get_identity(&user, None)
        .await?
        .ok_or_else(|| anyhow::anyhow!("missing cross-signing identity"))?;
    let master_json = match &identity {
        llmn_crypto::matrix_sdk_crypto::UserIdentity::Own(i) => {
            serde_json::to_value(i.master_key())?
        }
        llmn_crypto::matrix_sdk_crypto::UserIdentity::Other(i) => {
            serde_json::to_value(i.master_key())?
        }
    };
    ensure!(
        master_json["keys"]
            .as_object()
            .is_some_and(|keys| keys.values().any(|value| value.as_str() == Some(master))),
        "trusted master fingerprint mismatch"
    );
    let signatures = match identity {
        llmn_crypto::matrix_sdk_crypto::UserIdentity::Own(i) => i.verify().await?,
        llmn_crypto::matrix_sdk_crypto::UserIdentity::Other(i) => i.verify().await?,
    };
    relay(
        api,
        store,
        &s,
        Request {
            device_id: s.device.machine().device_id().to_string(),
            request_id: crate::id(),
            kind: "signatures".into(),
            body: serde_json::to_value(signatures.signed_keys)?,
        },
    )
    .await?;
    s.device.machine().mark_all_tracked_users_as_dirty().await?;
    for request in s.device.outgoing().await? {
        relay(api, store, &s, request).await?;
    }
    Ok(())
}
pub async fn import_recovery(
    api: &NodeClient,
    store: &Store,
    config: &Config,
    binding: &Binding,
    path: &std::path::Path,
) -> Result<()> {
    let file: serde_json::Value = serde_json::from_slice(&std::fs::read(path)?)?;
    ensure!(
        file["server"] == api.origin() && file["accountId"] == binding.ai,
        "recovery account or server mismatch"
    );
    let s = session(store, &binding.ai).await?;
    api.crypto_register(s.device.machine().device_id().as_str(), "LLM Nest Agent")
        .await?;
    let cloud = api.crypto_recovery().await?;
    ensure!(
        file["version"] == cloud["version"],
        "recovery version mismatch"
    );
    let encoded = base64::engine::general_purpose::STANDARD.decode(
        cloud["ciphertext"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("invalid recovery ciphertext"))?,
    )?;
    s.device
        .import_recovery(
            &encoded,
            serde_json::from_str(
                file["mediaEncryptionInfo"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("invalid recovery encryption info"))?,
            )?,
        )
        .await?;
    bootstrap_device(api, store, &s).await?;
    let backup = s.device.machine().backup_machine();
    let keys = backup.get_backup_keys().await?;
    let key = keys
        .decryption_key
        .ok_or_else(|| anyhow::anyhow!("recovery bundle missing history backup key"))?;
    let version = keys
        .backup_version
        .ok_or_else(|| anyhow::anyhow!("recovery bundle missing history backup version"))?;
    let metadata = api.crypto_backup().await?;
    ensure!(
        backup
            .verify_backup(serde_json::from_value(metadata.clone())?, false)
            .await?
            .trusted(),
        "backup metadata signature is not trusted"
    );
    ensure!(
        metadata["version"] == version
            && metadata["auth_data"]["public_key"] == key.megolm_v1_public_key().to_base64(),
        "backup identity mismatch"
    );
    let public = key.megolm_v1_public_key();
    public.set_version(version.clone());
    backup.enable_backup_v1(public).await?;
    let cloud = api.crypto_backup_keys().await?;
    let mut restored = Vec::new();
    for (room, data) in cloud["rooms"]
        .as_object()
        .ok_or_else(|| anyhow::anyhow!("invalid backup rooms"))?
    {
        for (id, entry) in data["sessions"]
            .as_object()
            .ok_or_else(|| anyhow::anyhow!("invalid backup sessions"))?
        {
            let data =
                key.decrypt_session_data(serde_json::from_value(entry["session_data"].clone())?)?;
            restored.push(
                llmn_crypto::matrix_sdk_crypto::olm::ExportedRoomKey::from_backed_up_room_key(
                    llmn_crypto::ruma::RoomId::parse(room)?,
                    id.clone(),
                    data,
                ),
            );
        }
    }
    s.device
        .machine()
        .store()
        .import_room_keys(restored, Some(&version), |_, _| {})
        .await?;
    store.db.execute("INSERT INTO crypto_recovery_ready VALUES(?1,?2) ON CONFLICT(ai) DO UPDATE SET version=excluded.version",params![binding.ai,file["version"].as_str().ok_or_else(||anyhow::anyhow!("invalid recovery version"))?])?;
    sync(api, store, config, binding).await?;
    Ok(())
}
pub async fn decrypt_bootstrap(
    api: &NodeClient,
    store: &Store,
    bootstrap: &mut connector::direct::DirectBootstrap,
) -> Result<()> {
    initialize(store)?;
    register_window(store, &bootstrap.conversation, &bootstrap.ai.id)?;
    if bootstrap.conversation.encryption.is_none() {
        return Ok(());
    }
    ensure_ready(store, &bootstrap.ai.id)?;
    let s = session(store, &bootstrap.ai.id).await?;
    apply_resources(api, store, None, &s, Some(&bootstrap.conversation.id)).await?;
    prepare_room(
        api,
        store,
        &s,
        &room_id(&bootstrap.conversation.id),
        &bootstrap.conversation.members,
        true,
        Some(&bootstrap.conversation.revision),
    )
    .await?;
    for message in &mut bootstrap.messages {
        if message.position.parse::<u64>()?
            > bootstrap
                .conversation
                .encryption
                .as_ref()
                .unwrap()
                .activation_position
                .parse::<u64>()?
        {
            ensure!(
                message.sealed.is_some(),
                "encrypted window contains unsealed message"
            );
        }
        decrypt_message(api, &s, &bootstrap.conversation.id, message).await?;
    }
    Ok(())
}
#[allow(clippy::too_many_arguments)]
pub async fn direct_input(
    api: &NodeClient,
    store: &Store,
    ai: &str,
    cid: &str,
    owner: &str,
    text: &str,
    sealed: Option<SealedEnvelope>,
    message_id: Option<String>,
    key_messages: Vec<serde_json::Value>,
) -> Result<Option<NodeMessage>> {
    initialize(store)?;
    let encrypted: bool = store
        .db
        .query_row(
            "SELECT encrypted FROM crypto_windows WHERE cid=?1",
            [cid],
            |r| r.get(0),
        )
        .optional()?
        .unwrap_or(false);
    if !encrypted {
        ensure!(
            sealed.is_none(),
            "sealed input requires an encrypted window"
        );
        return Ok(None);
    }
    ensure!(
        text.is_empty(),
        "encrypted direct input must omit plaintext"
    );
    let mut message = NodeMessage {
        id: message_id
            .ok_or_else(|| anyhow::anyhow!("encrypted direct input requires message id"))?,
        position: "0".into(),
        sender_id: owner.into(),
        sender_name: String::new(),
        reply_to: None,
        mentions: vec![],
        text: String::new(),
        revision: "1".into(),
        created_at: chrono::Utc::now().to_rfc3339(),
        attachments: vec![],
        import_origin: None,
        sealed: Some(
            sealed
                .ok_or_else(|| anyhow::anyhow!("encrypted direct input requires sealed content"))?,
        ),
    };
    let s = session(store, ai).await?;
    let mut events = Vec::new();
    for body in key_messages {
        ensure!(
            body["event_type"] == "m.room.encrypted",
            "local keys require encrypted SDK to-device events"
        );
        let content = &body["messages"][user_id(ai)][s.device.machine().device_id().as_str()];
        ensure!(
            content.is_object(),
            "local key delivery does not address the current AI device"
        );
        events.push(json!({"sender":user_id(owner),"type":"m.room.encrypted","content":content}));
    }
    s.device.receive_local_events(events).await?;
    decrypt_message(api, &s, cid, &mut message).await?;
    Ok(Some(message))
}
pub async fn seal_direct_message(
    api: &NodeClient,
    store: &Store,
    ai: &str,
    cid: &str,
    message: &mut NodeMessage,
    run: Option<&str>,
) -> Result<()> {
    if message.sealed.is_some() {
        message.text.clear();
        return Ok(());
    }
    ensure!(
        message.sender_id == ai,
        "human direct message lacks original sealed payload"
    );
    message.sealed = Some(
        freeze(
            api,
            store,
            ai,
            cid,
            &message.id,
            &message.revision,
            "message",
            &message.text,
            run,
            None,
            message.reply_to.as_deref(),
            &message.mentions,
        )
        .await?,
    );
    message.text.clear();
    Ok(())
}

pub async fn decrypt_receipt(
    api: &NodeClient,
    store: &Store,
    ai: &str,
    cid: &str,
    receipt: &mut connector::direct::DirectSyncReceipt,
) -> Result<()> {
    let encrypted: bool = store
        .db
        .query_row(
            "SELECT encrypted FROM crypto_windows WHERE cid=?1",
            [cid],
            |r| r.get(0),
        )
        .optional()?
        .unwrap_or(false);
    if !encrypted {
        return Ok(());
    }
    let s = session(store, ai).await?;
    for row in &mut receipt.messages {
        ensure!(
            row.message.sealed.is_some(),
            "encrypted receipt lacks ciphertext"
        );
        decrypt_message(api, &s, cid, &mut row.message).await?;
    }
    Ok(())
}
#[allow(clippy::too_many_arguments)]
async fn publish_resource(
    api: &NodeClient,
    store: &Store,
    s: &Session,
    cid: Option<&str>,
    kind: &str,
    id: &str,
    expected: &str,
    data: serde_json::Value,
    sources: Vec<SourceRevision>,
) -> Result<Resource> {
    let existing: Option<String> = store
        .db
        .query_row(
            "SELECT json FROM crypto_resource_outbox WHERE id=?1",
            [id],
            |r| r.get(0),
        )
        .optional()?;
    let request = if let Some(json) = existing {
        serde_json::from_str(&json)?
    } else {
        let audience = api.crypto_resource_participants(&s.ai, cid, kind).await?;
        let members = audience
            .accounts
            .into_iter()
            .map(|id| NodeAccount {
                id,
                name: String::new(),
                bio: String::new(),
                kind: String::new(),
            })
            .collect::<Vec<_>>();
        let room = resource_room(&s.ai, kind, cid);
        prepare_room(
            api,
            store,
            s,
            &room,
            &members,
            false,
            Some(&audience.revision),
        )
        .await?;
        ensure!(
            api.crypto_resource_participants(&s.ai, cid, kind)
                .await?
                .revision
                == audience.revision,
            "resource audience changed during key sharing"
        );
        let scope = format!("{}.{}.{}", s.ai, kind, cid.unwrap_or("global"));
        let revision = (expected.parse::<u64>()? + 1).to_string();
        let p = json!({"version":1,"server":api.origin(),"kind":"resource","conversationId":cid,"messageId":id,"senderId":s.ai,"revision":revision,"text":"","replyTo":null,"mentions":[],"runId":null,"sequence":null,"scope":scope,"id":id,"expectedRevision":expected,"expectedAudienceRevision":audience.revision,"data":data});
        let sealed = SealedEnvelope {
            version: 1,
            room_id: room,
            sender: user_id(&s.ai),
            device_id: s.device.machine().device_id().to_string(),
            event_id: format!("${id}.{revision}.final"),
            content: s.device.seal(&scope, "com.llmn.content.v1", &p).await?,
            device_keys: serde_json::Value::Null,
            signatures: serde_json::Value::Null,
        };
        let sealed = sign(s, sealed).await?;
        let request = SaveResource {
            request_id: crate::id(),
            device_id: s.device.machine().device_id().to_string(),
            ai_id: s.ai.clone(),
            conversation_id: cid.map(str::to_string),
            kind: kind.into(),
            expected_revision: expected.into(),
            expected_audience_revision: audience.revision,
            sealed: Some(sealed),
            sources,
            tombstone: None,
        };
        store.db.execute(
            "INSERT INTO crypto_resource_outbox VALUES(?1,?2)",
            params![id, serde_json::to_string(&request)?],
        )?;
        request
    };
    let accepted = match api.crypto_save_resource(id, &request).await {
        Ok(resource) => resource,
        Err(
            error @ NodeError::Api {
                status: 400 | 403 | 409 | 410,
                ..
            },
        ) => {
            let tx = store.db.unchecked_transaction()?;
            tx.execute("INSERT INTO crypto_resource_conflicts VALUES(?1,?2,'resource_conflict') ON CONFLICT(id) DO NOTHING",params![id,serde_json::to_string(&request)?])?;
            tx.execute("DELETE FROM crypto_resource_outbox WHERE id=?1", [id])?;
            tx.commit()?;
            return Err(error.into());
        }
        Err(error) => return Err(error.into()),
    };
    store
        .db
        .execute("DELETE FROM crypto_resource_outbox WHERE id=?1", [id])?;
    Ok(accepted)
}
pub fn encrypted(store: &Store, cid: &str) -> Result<bool> {
    initialize(store)?;
    Ok(store
        .db
        .query_row(
            "SELECT encrypted FROM crypto_windows WHERE cid=?1",
            [cid],
            |r| r.get(0),
        )
        .optional()?
        .unwrap_or(false))
}
pub async fn publish_summary(
    api: &NodeClient,
    store: &Store,
    ai: &str,
    cid: &str,
    record: &crate::consciousness::SummaryRecord,
) -> Result<()> {
    if !encrypted(store, cid)? {
        return Ok(());
    }
    ensure!(
        store.material_scope(ai, cid)?.revision == record.epoch,
        "summary materials changed before synchronization"
    );
    let s = session(store, ai).await?;
    let existing = api
        .crypto_resources(ai, Some(cid), "summary")
        .await?
        .items
        .into_iter()
        .find(|r| r.id == record.item.id);
    let expected = existing
        .as_ref()
        .map(|r| r.revision.as_str())
        .unwrap_or("0");
    ensure!(
        existing.as_ref().is_none_or(|r| r.tombstone.is_none()),
        "summary is no longer valid"
    );
    let mut item = record.item.clone();
    item.status = "ready".into();
    let data = json!({"item":item,"provenance":{"binding":record.binding,"requestedBy":record.requested_by,"settingsRevision":record.settings_revision,"conversationRevision":record.conversation_revision}});
    publish_resource(
        api,
        store,
        &s,
        Some(cid),
        "summary",
        &record.item.id,
        expected,
        data,
        record.item.sources.clone(),
    )
    .await?;
    Ok(())
}
pub async fn authorize_admin(
    api: &NodeClient,
    store: &Store,
    ai: &str,
    account: &str,
    device: &str,
) -> Result<()> {
    let s = session(store, ai).await?;
    ensure!(
        api.crypto_resource_participants(ai, None, "profile")
            .await?
            .accounts
            .iter()
            .any(|id| id == account),
        "account is not a current AI administrator"
    );
    let user = llmn_crypto::ruma::UserId::parse(user_id(account))?;
    let target = s
        .device
        .machine()
        .get_device(&user, device.into(), None)
        .await?
        .ok_or_else(|| anyhow::anyhow!("unknown administrator device"))?;
    ensure!(
        target.is_verified() && target.is_cross_signed_by_owner(),
        "administrator device requires independent fingerprint confirmation"
    );
    store.db.execute(
        "INSERT INTO crypto_admins(ai,account,device) VALUES(?1,?2,?3) ON CONFLICT DO NOTHING",
        params![ai, account, device],
    )?;
    share_admin_history(api, store, &s).await
}
async fn share_admin_history(api: &NodeClient, store: &Store, s: &Session) -> Result<()> {
    use sha2::{Digest, Sha256};
    let admins: Vec<(String, String, Option<String>)> = store
        .db
        .prepare("SELECT account,device,fingerprint FROM crypto_admins WHERE ai=?1")?
        .query_map([&s.ai], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
        .collect::<rusqlite::Result<_>>()?;
    let audience = api
        .crypto_resource_participants(&s.ai, None, "profile")
        .await?;
    let roster_key = format!("ai-admins:{}", s.ai);
    let previous: Option<String> = store
        .db
        .query_row(
            "SELECT fingerprint FROM crypto_room_audience WHERE room=?1",
            [&roster_key],
            |r| r.get(0),
        )
        .optional()?;
    if previous
        .as_ref()
        .is_some_and(|revision| revision != &audience.revision)
    {
        let keys = s
            .device
            .machine()
            .store()
            .export_room_keys(|_| true)
            .await?;
        let rooms = keys
            .iter()
            .map(|key| key.room_id.clone())
            .collect::<std::collections::BTreeSet<_>>();
        for room in rooms {
            s.device.machine().discard_room_key(&room).await?;
        }
        let cids: Vec<String> = store
            .db
            .prepare("SELECT cid FROM crypto_windows WHERE ai=?1 AND encrypted=1")?
            .query_map([&s.ai], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        for cid in cids {
            match api.history(&cid, "0").await {
                Ok(history) => {
                    prepare_room(
                        api,
                        store,
                        s,
                        &room_id(&cid),
                        &history.conversation.members,
                        true,
                        Some(&history.conversation.revision),
                    )
                    .await?
                }
                Err(NodeError::Api {
                    status: 403 | 404 | 410,
                    ..
                }) => {
                    let local: bool = store.db.query_row(
                        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='direct_windows')",
                        [],
                        |r| r.get(0),
                    )?;
                    if local {
                        crate::direct::conflict(store, &cid, "conversation authorization changed")?;
                    }
                }
                Err(error) => return Err(error.into()),
            }
        }
    }
    store.db.execute("INSERT INTO crypto_room_audience VALUES(?1,?2) ON CONFLICT(room) DO UPDATE SET fingerprint=excluded.fingerprint",params![roster_key,audience.revision])?;
    let current = audience.accounts;
    if admins.is_empty() {
        return Ok(());
    }
    let keys = s
        .device
        .machine()
        .store()
        .export_room_keys(|_| true)
        .await?;
    let body = json!({"aiId":s.ai,"senderDevice":s.device.machine().device_id(),"keys":keys});
    let fingerprint = format!("{:x}", Sha256::digest(serde_json::to_vec(&body)?));
    for (account, device, sent) in admins {
        if !current.contains(&account) {
            store.db.execute(
                "DELETE FROM crypto_admins WHERE ai=?1 AND account=?2",
                params![s.ai, account],
            )?;
            continue;
        }
        if sent.as_ref() == Some(&fingerprint) {
            continue;
        }
        let user = llmn_crypto::ruma::UserId::parse(user_id(&account))?;
        let target = s
            .device
            .machine()
            .get_device(&user, device.as_str().into(), None)
            .await?
            .ok_or_else(|| anyhow::anyhow!("administrator device disappeared"))?;
        ensure!(
            target.is_verified() && target.is_cross_signed_by_owner(),
            "administrator trust changed"
        );
        if let Some((id, r)) = s
            .device
            .machine()
            .get_missing_sessions([user.as_ref()].into_iter())
            .await?
        {
            relay(
                api,
                store,
                s,
                Request {
                    device_id: s.device.machine().device_id().to_string(),
                    request_id: id.to_string(),
                    kind: "keysClaim".into(),
                    body: json!({"one_time_keys":r.one_time_keys}),
                },
            )
            .await?;
        }
        let (requests, withheld) = s
            .device
            .machine()
            .encrypt_content_for_devices(
                vec![(*target).clone()],
                "com.llmn.ai-history.v1",
                &body,
                llmn_crypto::matrix_sdk_crypto::CollectStrategy::OnlyTrustedDevices,
            )
            .await?;
        ensure!(
            withheld.is_empty(),
            "administrator history key sharing withheld"
        );
        for request in requests {
            relay(
                api,
                store,
                s,
                Request {
                    device_id: s.device.machine().device_id().to_string(),
                    request_id: request.txn_id.to_string(),
                    kind: "toDevice".into(),
                    body: {
                        let mut value = serde_json::to_value(request)?;
                        value["llmn_purpose"] = json!("aiAdminHistory");
                        value
                    },
                },
            )
            .await?;
        }
        store.db.execute(
            "UPDATE crypto_admins SET fingerprint=?4 WHERE ai=?1 AND account=?2 AND device=?3",
            params![s.ai, account, device, fingerprint],
        )?;
    }
    Ok(())
}

pub fn draining(store: &Store, ai: &str) -> Result<bool> {
    initialize(store)?;
    Ok(store
        .db
        .query_row(
            "SELECT state!='active' FROM crypto_handover WHERE ai=?1",
            [ai],
            |r| r.get(0),
        )
        .optional()?
        .unwrap_or(false))
}

async fn sign(s: &Session, envelope: SealedEnvelope) -> Result<SealedEnvelope> {
    let mut value = serde_json::to_value(envelope)?;
    s.device.sign_envelope(&mut value).await?;
    Ok(serde_json::from_value(value)?)
}

fn register_window(store: &Store, conversation: &NodeConversation, ai: &str) -> Result<()> {
    let previous: Option<(String, bool)> = store
        .db
        .query_row(
            "SELECT ai,encrypted FROM crypto_windows WHERE cid=?1",
            [&conversation.id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    if let Some((actor, encrypted)) = previous {
        ensure!(
            !encrypted || actor == ai && conversation.encryption.is_some(),
            "encrypted conversation downgrade rejected"
        );
    }
    // Unencrypted group windows can be used by several AIs on this node.
    if conversation.encryption.is_none() {
        return Ok(());
    }
    if let Some(encryption) = &conversation.encryption {
        let activation: Option<String> = store
            .db
            .query_row(
                "SELECT position FROM crypto_window_activation WHERE cid=?1",
                [&conversation.id],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(activation) = activation {
            ensure!(
                activation == encryption.activation_position,
                "encryption activation boundary changed"
            );
        } else {
            store.db.execute(
                "INSERT INTO crypto_window_activation VALUES(?1,?2)",
                params![conversation.id, encryption.activation_position],
            )?;
        }
    }
    store.db.execute("INSERT INTO crypto_windows VALUES(?1,?2,?3) ON CONFLICT(cid) DO UPDATE SET encrypted=excluded.encrypted",params![conversation.id,ai,conversation.encryption.is_some()])?;
    Ok(())
}
pub async fn prepare_direct_run(
    store: &Store,
    ai: &str,
    cid: &str,
    request: &str,
    members: &[NodeAccount],
) -> Result<()> {
    if !encrypted(store, cid)? {
        return Ok(());
    }
    ensure_ready(store, ai)?;
    let s = session(store, ai).await?;
    let users = members
        .iter()
        .map(|m| llmn_crypto::ruma::UserId::parse(user_id(&m.id)))
        .collect::<Result<Vec<_>, _>>()?;
    for user in &users {
        if user == s.device.machine().user_id() {
            continue;
        }
        ensure!(
            s.device
                .machine()
                .get_user_devices(user, None)
                .await?
                .is_any_verified(),
            "offline recipient device is not trusted"
        );
    }
    ensure!(
        s.device
            .machine()
            .get_missing_sessions(users.iter().map(|u| u.as_ref()))
            .await?
            .is_none(),
        "offline encryption requires a previously established Olm session"
    );
    let room = llmn_crypto::ruma::RoomId::parse(room_id(cid))?;
    s.device.machine().discard_room_key(&room).await?;
    let settings = llmn_crypto::matrix_sdk_crypto::EncryptionSettings {
        sharing_strategy: llmn_crypto::matrix_sdk_crypto::CollectStrategy::OnlyTrustedDevices,
        ..Default::default()
    };
    for delivery in s
        .device
        .machine()
        .share_room_key(&room, users.iter().map(|u| u.as_ref()), settings)
        .await?
    {
        let outgoing = Request {
            device_id: s.device.machine().device_id().to_string(),
            request_id: delivery.txn_id.to_string(),
            kind: "toDevice".into(),
            body: serde_json::to_value(&*delivery)?,
        };
        let json = serde_json::to_string(&outgoing)?;
        let tx = store.db.unchecked_transaction()?;
        tx.execute(
            "INSERT INTO crypto_outbox VALUES(?1,?2,?3) ON CONFLICT DO NOTHING",
            params![ai, outgoing.request_id, json],
        )?;
        tx.execute(
            "INSERT INTO crypto_local_keys VALUES(?1,?2,?3,?4,?5) ON CONFLICT DO NOTHING",
            params![
                ai,
                cid,
                request,
                outgoing.request_id,
                serde_json::to_string(&outgoing.body)?
            ],
        )?;
        tx.commit()?;
        // The private durable IPC outbox is now responsible for delivery. Cloud
        // acknowledgement independently drains crypto_outbox after reconnection.
        s.device.acknowledge(&outgoing, json!({})).await?;
    }
    Ok(())
}
pub fn local_key_messages(store: &Store, owner: &str, cid: &str) -> Result<Vec<serde_json::Value>> {
    initialize(store)?;
    let bodies: Vec<String> = store
        .db
        .prepare("SELECT json FROM crypto_local_keys WHERE cid=?1 ORDER BY rowid DESC LIMIT 64")?
        .query_map([cid], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    let mut result = Vec::new();
    for json in bodies.into_iter().rev() {
        let mut body: serde_json::Value = serde_json::from_str(&json)?;
        let messages = body["messages"][user_id(owner)].clone();
        if messages.is_object() {
            body["messages"] = json!({user_id(owner):messages});
            result.push(body);
        }
    }
    Ok(result)
}
pub async fn direct_output(
    api: &NodeClient,
    store: &Store,
    ai: &str,
    cid: &str,
    request: &str,
    sequence: Option<u64>,
) -> Result<Option<SealedEnvelope>> {
    if !encrypted(store, cid)? {
        return Ok(None);
    }
    let (_, answer, _) = crate::direct::turn(store, request)?;
    Ok(Some(
        freeze(
            api,
            store,
            ai,
            cid,
            &answer.id,
            &answer.revision,
            if sequence.is_some() {
                "delta"
            } else {
                "message"
            },
            &answer.text,
            Some(request),
            sequence,
            answer.reply_to.as_deref(),
            &answer.mentions,
        )
        .await?,
    ))
}

async fn bootstrap_device(api: &NodeClient, store: &Store, s: &Session) -> Result<()> {
    let bootstrap = s.device.machine().bootstrap_cross_signing(false).await?;
    if let Some(request) = bootstrap.upload_keys_req {
        relay(
            api,
            store,
            s,
            llmn_crypto::requests::adapt(s.device.machine().device_id().as_str(), &request)?,
        )
        .await?;
    }
    let signing = bootstrap.upload_signing_keys_req;
    relay(api,store,s,Request{device_id:s.device.machine().device_id().to_string(),request_id:crate::id(),kind:"signingKeys".into(),body:json!({"master_key":signing.master_key,"self_signing_key":signing.self_signing_key,"user_signing_key":signing.user_signing_key})}).await?;
    relay(
        api,
        store,
        s,
        Request {
            device_id: s.device.machine().device_id().to_string(),
            request_id: crate::id(),
            kind: "signatures".into(),
            body: serde_json::to_value(bootstrap.upload_signatures_req.signed_keys)?,
        },
    )
    .await?;
    Ok(())
}
pub fn authorization_lost(store: &Store, ai: &str) -> Result<()> {
    initialize(store)?;
    store
        .db
        .execute("UPDATE crypto_devices SET ready=0 WHERE ai=?1", [ai])?;
    let local: bool = store.db.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='direct_windows')",
        [],
        |r| r.get(0),
    )?;
    if local {
        let cids: Vec<String> = store
            .db
            .prepare("SELECT cid FROM direct_windows WHERE ai=?1")?
            .query_map([ai], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        for cid in cids {
            crate::direct::conflict(store, &cid, "AI node authorization was revoked")?;
        }
    }
    Ok(())
}
pub fn enrolled(store: &Store, ai: &str) -> Result<bool> {
    initialize(store)?;
    Ok(store.db.query_row(
        "SELECT EXISTS(SELECT 1 FROM crypto_devices WHERE ai=?1)",
        [ai],
        |r| r.get(0),
    )?)
}
