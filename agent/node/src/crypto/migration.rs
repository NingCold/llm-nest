//! Explicit conversion of node-owned materials. Never opens a model stream.
use super::*;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Item {
    id: String,
    cid: Option<String>,
    kind: String,
    local_revision: String,
    #[serde(skip)]
    data: serde_json::Value,
    data_digest: String,
    sources: Vec<SourceRevision>,
    tombstone: Option<String>,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Plan {
    ai: String,
    binding_version: String,
    profile_revision: String,
    scopes: Vec<MaterialScope>,
    windows: Vec<(String, String)>,
    items: Vec<Item>,
    authoritative: Vec<serde_json::Value>,
}
fn idle(store: &Store, ai: &str) -> Result<()> {
    ensure!(!store.db.query_row("SELECT EXISTS(SELECT 1 FROM tasks WHERE ai=?1 AND state IN ('intent','started','final'))",[ai],|r|r.get::<_,bool>(0))?,"material migration requires all cloud executions to be confirmed");
    let local: bool = store.db.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='direct_windows')",
        [],
        |r| r.get(0),
    )?;
    if local {
        ensure!(!store.db.query_row("SELECT EXISTS(SELECT 1 FROM direct_windows w WHERE w.ai=?1 AND (w.sync_json IS NOT NULL OR EXISTS(SELECT 1 FROM direct_turns t WHERE t.cid=w.cid AND (t.synced=0 OR t.state IN ('queued','running')))))",[ai],|r|r.get::<_,bool>(0))?,"material migration requires local conversations to finish synchronization");
    }
    ensure!(!store.db.query_row("SELECT EXISTS(SELECT 1 FROM conscious_summaries WHERE ai=?1 AND json_extract(json,'$.item.status') IN ('queued','running','pending_sync'))",[ai],|r|r.get::<_,bool>(0))?,"material migration requires summary work to settle");
    Ok(())
}
pub(super) fn has_gaps(store: &Store, ai: &str) -> Result<bool> {
    scope_has_gaps(store, ai, None)
}
pub(super) fn scope_has_gaps(store: &Store, ai: &str, cid: Option<&str>) -> Result<bool> {
    for (table, kind) in [
        ("conscious_memory", "memory"),
        ("conscious_summaries", "summary"),
    ] {
        // New summary work is not an old material awaiting conversion. Migration
        // and handover separately require these transitions to settle.
        let completed = if kind == "summary" {
            "AND json_extract(m.json,'$.item.status') NOT IN ('queued','running','pending_sync')"
        } else {
            ""
        };
        let sql = format!(
            "SELECT EXISTS(SELECT 1 FROM {table} m WHERE m.ai=?1 AND (?3 IS NULL OR m.cid=?3) {completed} AND NOT EXISTS(SELECT 1 FROM crypto_resource_scopes s JOIN crypto_resources r ON r.id=s.id WHERE s.ai=m.ai AND s.cid=m.cid AND s.id=m.id AND s.kind=?2 AND r.status IN ('applied','stale','forgotten')))"
        );
        if store
            .db
            .query_row(&sql, params![ai, kind, cid], |r| r.get::<_, bool>(0))?
        {
            return Ok(true);
        }
    }
    Ok(store.db.query_row("SELECT EXISTS(SELECT 1 FROM conscious_settings m WHERE m.ai=?1 AND (?2 IS NULL OR m.cid=?2) AND (json_extract(m.json,'$.enabled')=1 OR json_extract(m.json,'$.binding') IS NOT NULL) AND NOT EXISTS(SELECT 1 FROM crypto_resource_scopes s JOIN crypto_resources r ON r.id=s.id WHERE s.ai=m.ai AND s.cid=m.cid AND s.kind='summarySettings' AND r.status IN ('applied','stale','forgotten')))",params![ai,cid],|r|r.get(0))?)
}
fn report(plan: &Plan, digest: &str) -> serde_json::Value {
    json!({"type":"materialMigrationPreview","digest":digest,"aiId":plan.ai,"bindingVersion":plan.binding_version,"profileRevision":plan.profile_revision,"profileAction":if plan.authoritative.iter().any(|r|r["kind"]=="profile" && r["revision"]!=plan.profile_revision){"applyCloudAuthoritative"}else{"alreadyAuthoritative"},"scopes":plan.scopes,"windows":plan.windows,"authoritative":plan.authoritative,"items":plan.items.iter().map(|i|json!({"id":i.id,"conversationId":i.cid,"kind":i.kind,"localRevision":i.local_revision,"sources":i.sources,"tombstone":i.tombstone})).collect::<Vec<_>>()})
}
async fn collect(
    api: &NodeClient,
    store: &Store,
    binding: &Binding,
    config: &Config,
) -> Result<Plan> {
    let ai = &binding.ai;
    let cids:Vec<String>=store.db.prepare("SELECT cid FROM conscious_memory WHERE ai=?1 UNION SELECT cid FROM conscious_summaries WHERE ai=?1 UNION SELECT cid FROM conscious_settings WHERE ai=?1")?.query_map([ai],|r|r.get(0))?.collect::<rusqlite::Result<_>>()?;
    let mut plan = Plan {
        ai: ai.clone(),
        binding_version: binding.version.clone(),
        profile_revision: store.profile(ai)?.revision,
        scopes: vec![],
        windows: vec![],
        items: vec![],
        authoritative: vec![],
    };
    let s = session(store, ai).await?;
    for r in api.crypto_resources(ai, None, "profile").await?.items {
        plan.authoritative.push(json!({"id":r.id,"kind":r.kind,"revision":r.revision,"conversationId":r.conversation_id,"tombstone":r.tombstone}));
    }
    for cid in cids {
        if !scope_has_gaps(store, ai, Some(&cid))? {
            continue;
        }
        let history = api.history(&cid, "0").await?;
        ensure!(
            history.conversation.members.iter().any(|m| m.id == *ai),
            "material migration requires current window access"
        );
        apply_resources(api, store, Some(config), &s, Some(&cid)).await?;
        plan.windows
            .push((cid.clone(), history.conversation.revision));
        plan.scopes.push(store.material_scope(ai, &cid)?);
        for kind in ["memory", "summary", "summarySettings"] {
            let authority = api.crypto_resources(ai, Some(&cid), kind).await?.items;
            for resource in &authority {
                remember_resource_scope(store, resource)?;
                plan.authoritative.push(json!({"id":resource.id,"kind":resource.kind,"revision":resource.revision,"conversationId":resource.conversation_id,"tombstone":resource.tombstone}));
            }
            match kind {
                "memory" => {
                    for memory in store.memories(ai, &cid)? {
                        if authority.iter().any(|r| r.id == memory.id) {
                            continue;
                        }
                        let tombstone = match memory.status.as_str() {
                            "forgotten" => Some("forgotten".into()),
                            "stale" => Some("stale".into()),
                            _ => None,
                        };
                        let sources = memory.source.clone().into_iter().collect();
                        plan.items.push(Item {
                            id: memory.id.clone(),
                            cid: Some(cid.clone()),
                            kind: kind.into(),
                            local_revision: memory.revision.clone(),
                            data_digest: String::new(),
                            data: if tombstone.is_some() {
                                serde_json::Value::Null
                            } else {
                                serde_json::to_value(memory)?
                            },
                            sources,
                            tombstone,
                        });
                    }
                }
                "summary" => {
                    for summary in store.summaries(ai, &cid)? {
                        if authority.iter().any(|r| r.id == summary.item.id) {
                            continue;
                        }
                        let tombstone = if summary.item.status == "stale" {
                            Some("stale".into())
                        } else {
                            None
                        };
                        plan.items.push(Item {id:summary.item.id.clone(),cid:Some(cid.clone()),kind:kind.into(),local_revision:summary.item.revision.clone(),data_digest:String::new(),sources:summary.item.sources.clone(),data:if tombstone.is_some(){serde_json::Value::Null}else{json!({"item":summary.item,"provenance":{"binding":summary.binding,"requestedBy":summary.requested_by,"settingsRevision":summary.settings_revision,"conversationRevision":summary.conversation_revision}})},tombstone});
                    }
                }
                _ => {
                    if authority.is_empty() {
                        let settings = store.settings(ai, &cid)?;
                        if !settings.enabled && settings.binding.is_none() {
                            continue;
                        }
                        plan.items.push(Item {
                            id: crate::id(),
                            cid: Some(cid.clone()),
                            kind: kind.into(),
                            local_revision: settings.revision.clone(),
                            sources: vec![],
                            data_digest: String::new(),
                            data: serde_json::to_value(settings)?,
                            tombstone: None,
                        });
                    }
                }
            }
        }
    }
    for item in &mut plan.items {
        item.data_digest = format!("{:x}", Sha256::digest(serde_json::to_vec(&item.data)?));
    }
    Ok(plan)
}
pub async fn preview(
    api: &NodeClient,
    store: &Store,
    config: &Config,
    binding: &Binding,
) -> Result<serde_json::Value> {
    initialize(store)?;
    idle(store, &binding.ai)?;
    ensure!(
        api.tasks().await?.is_empty(),
        "material migration requires cloud tasks to settle"
    );
    crate::consciousness::sync(api, store, &binding.ai).await?;
    super::sync_inner(api, store, config, binding, false).await?;
    ensure!(
        identity_ready(store, &binding.ai)?,
        "material migration requires authorized identity and recovery material"
    );
    idle(store, &binding.ai)?;
    let pending:Option<(String,String)>=store.db.query_row("SELECT digest,json FROM crypto_material_migrations WHERE ai=?1 AND status='committing'",[&binding.ai],|r|Ok((r.get(0)?,r.get(1)?))).optional()?;
    if let Some((digest, json)) = pending {
        return Ok(report(&serde_json::from_str(&json)?, &digest));
    }
    let plan = collect(api, store, binding, config).await?;
    let json = serde_json::to_string(&plan)?;
    let digest = format!("{:x}", Sha256::digest(json.as_bytes()));
    store.db.execute("INSERT INTO crypto_material_migrations VALUES(?1,?2,?3,'preview') ON CONFLICT(ai) DO UPDATE SET digest=excluded.digest,json=excluded.json,status='preview'",params![binding.ai,digest,json])?;
    Ok(report(&plan, &digest))
}
fn local_revision(store: &Store, ai: &str, item: &Item) -> Result<String> {
    let cid = item.cid.as_deref().expect("window material");
    match item.kind.as_str() {
        "memory" => Ok(store
            .memories(ai, cid)?
            .into_iter()
            .find(|m| m.id == item.id)
            .ok_or_else(|| anyhow::anyhow!("local memory disappeared"))?
            .revision),
        "summary" => Ok(store
            .summaries(ai, cid)?
            .into_iter()
            .find(|m| m.item.id == item.id)
            .ok_or_else(|| anyhow::anyhow!("local summary disappeared"))?
            .item
            .revision),
        _ => Ok(store.settings(ai, cid)?.revision),
    }
}
fn item_data(store: &Store, ai: &str, item: &Item) -> Result<serde_json::Value> {
    if item.tombstone.is_some() {
        return Ok(serde_json::Value::Null);
    }
    let cid = item.cid.as_deref().expect("window material");
    match item.kind.as_str() {
        "memory" => Ok(serde_json::to_value(
            store
                .memories(ai, cid)?
                .into_iter()
                .find(|m| m.id == item.id)
                .ok_or_else(|| anyhow::anyhow!("local memory disappeared"))?,
        )?),
        "summary" => {
            let s = store
                .summaries(ai, cid)?
                .into_iter()
                .find(|s| s.item.id == item.id)
                .ok_or_else(|| anyhow::anyhow!("local summary disappeared"))?;
            Ok(
                json!({"item":s.item,"provenance":{"binding":s.binding,"requestedBy":s.requested_by,"settingsRevision":s.settings_revision,"conversationRevision":s.conversation_revision}}),
            )
        }
        _ => Ok(serde_json::to_value(store.settings(ai, cid)?)?),
    }
}
async fn applied(
    api: &NodeClient,
    store: &Store,
    config: &Config,
    s: &Session,
    r: &Resource,
) -> Result<()> {
    apply_resource(api, store, Some(config), s, r).await?;
    store.db.execute("INSERT INTO crypto_resources VALUES(?1,?2,?3) ON CONFLICT(id) DO UPDATE SET revision=excluded.revision,status=excluded.status",params![r.id,r.revision,r.tombstone.as_deref().unwrap_or("applied")])?;
    api.crypto_apply_resource(
        &r.id,
        &ApplyResource {
            device_id: s.device.machine().device_id().to_string(),
            revision: r.revision.clone(),
            status: "applied".into(),
        },
    )
    .await?;
    Ok(())
}
pub async fn commit(
    api: &NodeClient,
    store: &Store,
    config: &Config,
    binding: &Binding,
    digest: &str,
) -> Result<serde_json::Value> {
    initialize(store)?;
    idle(store, &binding.ai)?;
    ensure!(
        api.tasks().await?.is_empty(),
        "material migration requires cloud tasks to settle"
    );
    let (stored, json, status): (String, String, String) = store.db.query_row(
        "SELECT digest,json,status FROM crypto_material_migrations WHERE ai=?1",
        [&binding.ai],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?;
    ensure!(stored == digest, "material migration preview changed");
    let plan: Plan = serde_json::from_str(&json)?;
    ensure!(
        plan.binding_version == binding.version,
        "material migration node binding changed"
    );
    if status == "complete" {
        return Ok(
            json!({"type":"materialMigrationComplete","digest":digest,"alreadyComplete":true}),
        );
    }
    crate::consciousness::sync(api, store, &binding.ai).await?;
    super::sync_inner(api, store, config, binding, false).await?;
    ensure!(
        identity_ready(store, &binding.ai)?,
        "material migration requires authorized identity and recovery material"
    );
    idle(store, &binding.ai)?;
    if status == "preview" {
        ensure!(
            store.profile(&binding.ai)?.revision == plan.profile_revision
                && store.scopes_current(&binding.ai, &plan.scopes)?,
            "material migration local snapshot changed"
        );
    }
    for (cid, revision) in &plan.windows {
        ensure!(
            api.history(cid, "0").await?.conversation.revision == *revision,
            "material migration window changed"
        );
    }
    store.db.execute(
        "UPDATE crypto_material_migrations SET status='committing' WHERE ai=?1",
        [&binding.ai],
    )?;
    let s = session(store, &binding.ai).await?;
    let authority = api
        .crypto_resources(&binding.ai, None, "profile")
        .await?
        .items;
    let profile = authority
        .first()
        .ok_or_else(|| anyhow::anyhow!("profile authority missing"))?;
    let preview_profile = plan
        .authoritative
        .iter()
        .find(|r| r["kind"] == "profile")
        .ok_or_else(|| anyhow::anyhow!("profile was not previewed"))?;
    ensure!(
        preview_profile["id"] == profile.id && preview_profile["revision"] == profile.revision,
        "cloud profile authority changed since preview"
    );
    applied(api, store, config, &s, profile).await?;
    for item in &plan.items {
        let cloud = api
            .crypto_resources(&binding.ai, item.cid.as_deref(), &item.kind)
            .await?
            .items;
        if let Some(authority) = cloud
            .iter()
            .find(|r| r.id == item.id || item.kind == "summarySettings")
        {
            applied(api, store, config, &s, authority).await?;
            continue;
        }
        ensure!(
            !store.db.query_row(
                "SELECT EXISTS(SELECT 1 FROM crypto_resource_conflicts WHERE id=?1)",
                [&item.id],
                |r| r.get::<_, bool>(0)
            )?,
            "material migration resource conflict requires a new preview"
        );
        ensure!(
            local_revision(store, &binding.ai, item)? == item.local_revision,
            "material migration local item changed"
        );
        let data = item_data(store, &binding.ai, item)?;
        ensure!(
            format!("{:x}", Sha256::digest(serde_json::to_vec(&data)?)) == item.data_digest,
            "material migration local content changed"
        );
        let accepted = publish_resource(
            api,
            store,
            &s,
            item.cid.as_deref(),
            &item.kind,
            &item.id,
            "0",
            data,
            item.sources.clone(),
            item.tombstone.clone(),
        )
        .await?;
        applied(api, store, config, &s, &accepted).await?;
    }
    super::sync(api, store, config, binding).await?;
    ensure!(
        !has_gaps(store, &binding.ai)?,
        "material migration still has unpublished resources"
    );
    while let Some((id, request)) = s.device.machine().backup_machine().backup().await? {
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
    ensure!(
        s.device
            .machine()
            .backup_machine()
            .room_key_counts()
            .await?
            .backed_up
            == s.device
                .machine()
                .backup_machine()
                .room_key_counts()
                .await?
                .total,
        "material migration history key backup is incomplete"
    );
    store.db.execute(
        "UPDATE crypto_material_migrations SET status='complete' WHERE ai=?1",
        [&binding.ai],
    )?;
    Ok(json!({"type":"materialMigrationComplete","digest":digest,"resources":plan.items.len()}))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unsettled_cloud_and_local_intents_block_material_conversion() {
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        initialize(&store).unwrap();
        crate::direct::initialize(&store).unwrap();
        store
            .db
            .execute(
                "INSERT INTO tasks(id,ai,state) VALUES('task','ai','intent')",
                [],
            )
            .unwrap();
        assert!(idle(&store, "ai").is_err());
        assert!(idle(&store, "other").is_ok());
        store
            .db
            .execute("UPDATE tasks SET state='acknowledged'", [])
            .unwrap();
        store.db.execute("INSERT INTO direct_windows(cid,owner,ai,grant,bootstrap,sync_json) VALUES('cid','owner','ai','grant','{}','{}')",[]).unwrap();
        assert!(idle(&store, "ai").is_err());
        store
            .db
            .execute("UPDATE direct_windows SET sync_json=NULL", [])
            .unwrap();
        store.db.execute("INSERT INTO direct_turns(request,cid,fingerprint,human,answer,state,payload,synced) VALUES('request','cid','fingerprint','{}','{}','uncertain','{}',0)",[]).unwrap();
        assert!(idle(&store, "ai").is_err());
        store
            .db
            .execute("UPDATE direct_turns SET synced=1,state='interrupted'", [])
            .unwrap();
        assert!(idle(&store, "ai").is_ok());
    }
    #[test]
    fn handover_coverage_requires_each_ai_scope_and_applied_tombstones() {
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        initialize(&store).unwrap();
        store
            .db
            .execute(
                "INSERT INTO conscious_memory VALUES('ai','cid','memory','{}')",
                [],
            )
            .unwrap();
        assert!(has_gaps(&store, "ai").unwrap());
        store
            .db
            .execute(
                "INSERT INTO crypto_devices VALUES('ai','device',x'01','0',1)",
                [],
            )
            .unwrap();
        store
            .db
            .execute(
                "INSERT INTO crypto_recovery_ready VALUES('ai','backup')",
                [],
            )
            .unwrap();
        store
            .db
            .execute(
                r#"INSERT INTO profiles VALUES('ai','{"revision":"1"}')"#,
                [],
            )
            .unwrap();
        store
            .db
            .execute(
                "INSERT INTO crypto_resource_scopes VALUES('profile','ai',NULL,'profile')",
                [],
            )
            .unwrap();
        store
            .db
            .execute(
                "INSERT INTO crypto_resources VALUES('profile','1','applied')",
                [],
            )
            .unwrap();
        assert!(ensure_scope_ready(&store, "ai", "cid").is_err());
        assert!(ensure_scope_ready(&store, "ai", "unrelated").is_ok());
        store
            .db
            .execute(
                r#"UPDATE profiles SET json='{"revision":"local-edit"}' WHERE ai='ai'"#,
                [],
            )
            .unwrap();
        assert!(!capable(&store, "ai").unwrap());
        store
            .db
            .execute(
                r#"UPDATE profiles SET json='{"revision":"1"}' WHERE ai='ai'"#,
                [],
            )
            .unwrap();
        store
            .db
            .execute(
                "INSERT INTO crypto_resource_scopes VALUES('memory','other','cid','memory')",
                [],
            )
            .unwrap();
        store
            .db
            .execute(
                "INSERT INTO crypto_resources VALUES('memory','1','stale')",
                [],
            )
            .unwrap();
        assert!(has_gaps(&store, "ai").unwrap());
        store
            .db
            .execute("UPDATE crypto_resource_scopes SET ai='ai'", [])
            .unwrap();
        assert!(!has_gaps(&store, "ai").unwrap());
        store
            .db
            .execute(
                r#"INSERT INTO conscious_settings VALUES('ai','cid','{"enabled":true}')"#,
                [],
            )
            .unwrap();
        assert!(has_gaps(&store, "ai").unwrap());
    }
    #[test]
    fn new_summary_work_does_not_fence_its_own_preparation_but_old_results_do() {
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        initialize(&store).unwrap();
        store
            .db
            .execute(
                "INSERT INTO crypto_devices VALUES('ai','device',x'01','0',1)",
                [],
            )
            .unwrap();
        store
            .db
            .execute(
                "INSERT INTO crypto_recovery_ready VALUES('ai','backup')",
                [],
            )
            .unwrap();
        store
            .db
            .execute(
                r#"INSERT INTO profiles VALUES('ai','{"revision":"1"}')"#,
                [],
            )
            .unwrap();
        store
            .db
            .execute(
                "INSERT INTO crypto_resource_scopes VALUES('profile','ai',NULL,'profile')",
                [],
            )
            .unwrap();
        store
            .db
            .execute(
                "INSERT INTO crypto_resources VALUES('profile','1','applied')",
                [],
            )
            .unwrap();
        store.db.execute(r#"INSERT INTO conscious_summaries VALUES('ai','cid','summary','{"item":{"status":"queued"}}')"#,[]).unwrap();
        for state in ["queued", "running", "pending_sync"] {
            store
                .db
                .execute(
                    "UPDATE conscious_summaries SET json=json_set(json,'$.item.status',?1)",
                    [state],
                )
                .unwrap();
            ensure_scope_ready(&store, "ai", "cid").unwrap();
            assert!(
                idle(&store, "ai").is_err(),
                "migration must still wait for active summary work"
            );
        }
        for state in ["ready", "stale", "failed", "interrupted"] {
            store
                .db
                .execute(
                    "UPDATE conscious_summaries SET json=json_set(json,'$.item.status',?1)",
                    [state],
                )
                .unwrap();
            assert!(
                ensure_scope_ready(&store, "ai", "cid").is_err(),
                "old summary results still need conversion"
            );
        }
        store
            .db
            .execute(
                "INSERT INTO crypto_resource_scopes VALUES('summary','ai','cid','summary')",
                [],
            )
            .unwrap();
        store
            .db
            .execute(
                "INSERT INTO crypto_resources VALUES('summary','1','applied')",
                [],
            )
            .unwrap();
        ensure_scope_ready(&store, "ai", "cid").unwrap();
    }
    #[test]
    fn preview_report_never_contains_material_bodies_or_recovery_secrets() {
        let plan = Plan {
            ai: "ai".into(),
            binding_version: "1".into(),
            profile_revision: "1".into(),
            scopes: vec![],
            windows: vec![],
            authoritative: vec![],
            items: vec![Item {
                id: "item".into(),
                cid: Some("cid".into()),
                kind: "memory".into(),
                local_revision: "1".into(),
                data: json!({"text":"private body"}),
                data_digest: "digest".into(),
                sources: vec![],
                tombstone: None,
            }],
        };
        let stored = serde_json::to_string(&plan).unwrap();
        assert!(!stored.contains("private body"));
        let value = report(&plan, "digest").to_string();
        assert!(!value.contains("private body"));
        assert!(!value.contains("dataDigest"));
        assert!(value.contains("digest"));
    }
}
