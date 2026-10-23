//! Node-owned conscious materials. Cloud sees source IDs and epochs, never their bodies.
use crate::store::{Binding, Config, Store};
use ai_client::{AiClient, ChatChunk, ChatRequest, ModelRouter, ModelSelection};
use anyhow::{Result, bail, ensure};
use connector::node::*;
use futures_util::StreamExt;
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Clone, Serialize, Deserialize)]
pub struct SummaryRecord {
    pub item: SummaryItem,
    pub binding: SummaryModel,
    pub settings_revision: String,
    pub conversation_revision: String,
    pub epoch: String,
    pub requested_by: String,
    pub start_request: String,
    pub intent: bool,
    pub confirmation: Option<SummaryConfirmation>,
    pub finished: bool,
}
pub fn initialize(db: &Connection) -> Result<()> {
    db.execute_batch("CREATE TABLE IF NOT EXISTS conscious_scopes(ai TEXT NOT NULL,cid TEXT NOT NULL,epoch TEXT NOT NULL,announced TEXT NOT NULL DEFAULT '',PRIMARY KEY(ai,cid));
 CREATE TABLE IF NOT EXISTS conscious_memory(ai TEXT NOT NULL,cid TEXT NOT NULL,id TEXT NOT NULL,json TEXT NOT NULL,PRIMARY KEY(ai,cid,id));
 CREATE TABLE IF NOT EXISTS conscious_summaries(ai TEXT NOT NULL,cid TEXT NOT NULL,id TEXT NOT NULL,json TEXT NOT NULL,PRIMARY KEY(ai,cid,id));
 CREATE TABLE IF NOT EXISTS conscious_settings(ai TEXT NOT NULL,cid TEXT NOT NULL,json TEXT NOT NULL,PRIMARY KEY(ai,cid));
 CREATE TABLE IF NOT EXISTS conscious_receipts(ai TEXT NOT NULL,request TEXT NOT NULL,fingerprint TEXT NOT NULL,PRIMARY KEY(ai,request));
 CREATE TABLE IF NOT EXISTS conscious_cursors(ai TEXT PRIMARY KEY,cursor TEXT NOT NULL);")?;
    // A crash never authorizes another model invocation. Preserve only terminal records.
    let rows: Vec<(String, String, String, String)> = db
        .prepare("SELECT ai,cid,id,json FROM conscious_summaries")?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?
        .collect::<rusqlite::Result<_>>()?;
    for (ai, cid, id, json) in rows {
        let mut row: SummaryRecord = serde_json::from_str(&json)?;
        if matches!(row.item.status.as_str(), "queued" | "running") {
            row.item.status = "interrupted".into();
            row.item.error = Some("node_restarted".into());
            row.item.text.clear();
            db.execute(
                "UPDATE conscious_summaries SET json=?4 WHERE ai=?1 AND cid=?2 AND id=?3",
                params![ai, cid, id, serde_json::to_string(&row)?],
            )?;
        }
    }
    Ok(())
}
fn next(rev: &str) -> Result<String> {
    Ok((rev.parse::<u64>()? + 1).to_string())
}
fn now() -> String {
    chrono::Utc::now().to_rfc3339()
}
impl Store {
    pub fn material_scope(&self, ai: &str, cid: &str) -> Result<MaterialScope> {
        self.db.execute(
            "INSERT INTO conscious_scopes(ai,cid,epoch) VALUES(?1,?2,?3) ON CONFLICT DO NOTHING",
            params![ai, cid, crate::id()],
        )?;
        Ok(MaterialScope {
            conversation_id: cid.into(),
            revision: self.db.query_row(
                "SELECT epoch FROM conscious_scopes WHERE ai=?1 AND cid=?2",
                params![ai, cid],
                |r| r.get(0),
            )?,
        })
    }
    fn change_epoch(&self, ai: &str, cid: &str) -> Result<()> {
        self.material_scope(ai, cid)?;
        self.db.execute(
            "UPDATE conscious_scopes SET epoch=?3 WHERE ai=?1 AND cid=?2",
            params![ai, cid, crate::id()],
        )?;
        Ok(())
    }
    pub fn scopes_current(&self, ai: &str, scopes: &[MaterialScope]) -> Result<bool> {
        for s in scopes {
            if self.material_scope(ai, &s.conversation_id)?.revision != s.revision {
                return Ok(false);
            }
        }
        Ok(true)
    }
    pub fn memories(&self, ai: &str, cid: &str) -> Result<Vec<MemoryItem>> {
        let texts = self
            .db
            .prepare("SELECT json FROM conscious_memory WHERE ai=?1 AND cid=?2 ORDER BY json_extract(json,'$.updatedAt') DESC,id")?
            .query_map(params![ai, cid], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        texts
            .into_iter()
            .map(|s| Ok(serde_json::from_str(&s)?))
            .collect()
    }
    pub fn summaries(&self, ai: &str, cid: &str) -> Result<Vec<SummaryRecord>> {
        let texts = self
            .db
            .prepare("SELECT json FROM conscious_summaries WHERE ai=?1 AND cid=?2 ORDER BY CAST(json_extract(json,'$.item.throughPosition') AS INTEGER) DESC,id")?
            .query_map(params![ai, cid], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        texts
            .into_iter()
            .map(|s| Ok(serde_json::from_str(&s)?))
            .collect()
    }
    pub fn settings(&self, ai: &str, cid: &str) -> Result<SummarySettings> {
        let json: Option<String> = self
            .db
            .query_row(
                "SELECT json FROM conscious_settings WHERE ai=?1 AND cid=?2",
                params![ai, cid],
                |r| r.get(0),
            )
            .optional()?;
        Ok(match json {
            Some(json) => serde_json::from_str(&json)?,
            None => SummarySettings {
                revision: "0".into(),
                enabled: false,
                binding: None,
            },
        })
    }
    fn save_memory(&self, ai: &str, cid: &str, m: &MemoryItem) -> Result<()> {
        self.db.execute("INSERT INTO conscious_memory VALUES(?1,?2,?3,?4) ON CONFLICT(ai,cid,id) DO UPDATE SET json=excluded.json",params![ai,cid,m.id,serde_json::to_string(m)?])?;
        Ok(())
    }
    fn save_summary(&self, ai: &str, cid: &str, s: &SummaryRecord) -> Result<()> {
        self.db.execute("INSERT INTO conscious_summaries VALUES(?1,?2,?3,?4) ON CONFLICT(ai,cid,id) DO UPDATE SET json=excluded.json",params![ai,cid,s.item.id,serde_json::to_string(s)?])?;
        Ok(())
    }
    pub fn invalidate(&self, ai: &str, page: &ConsciousnessChanges) -> Result<()> {
        let tx = self.db.unchecked_transaction()?;
        for change in &page.changes {
            let cid = &change.conversation_id;
            let mut affected = false;
            if change.message_id.is_none() && change.reason == "window_changed" {
                for mut summary in self.summaries(ai, cid)? {
                    if matches!(summary.item.status.as_str(), "queued" | "running") {
                        summary.item.status = "interrupted".into();
                        summary.item.text.clear();
                        summary.item.error = Some("window_changed".into());
                        self.save_summary(ai, cid, &summary)?;
                    }
                }
                self.change_epoch(ai, cid)?;
                continue;
            }
            for mut m in self.memories(ai, cid)? {
                if m.status != "forgotten"
                    && (change.message_id.is_none()
                        || m.source.as_ref().is_some_and(|source| {
                            Some(&source.message_id) == change.message_id.as_ref()
                        }))
                {
                    m.text.clear();
                    m.status = "stale".into();
                    m.revision = next(&m.revision)?;
                    m.updated_at = now();
                    self.save_memory(ai, cid, &m)?;
                    affected = true;
                }
            }
            for mut s in self.summaries(ai, cid)? {
                if change.message_id.is_none()
                    || s.item
                        .sources
                        .iter()
                        .any(|s| Some(&s.message_id) == change.message_id.as_ref())
                {
                    s.item.text.clear();
                    s.item.status = "stale".into();
                    s.item.revision = next(&s.item.revision)?;
                    s.item.updated_at = now();
                    s.item.error = Some("source_changed".into());
                    self.save_summary(ai, cid, &s)?;
                    affected = true;
                }
            }
            // Window/reference invalidation must also fence active raw-context snapshots.
            if affected || change.message_id.is_none() {
                self.change_epoch(ai, cid)?;
            }
        }
        tx.execute("INSERT INTO conscious_cursors VALUES(?1,?2) ON CONFLICT(ai) DO UPDATE SET cursor=excluded.cursor",params![ai,page.cursor])?;
        tx.commit()?;
        Ok(())
    }
}
pub async fn sync(api: &NodeClient, store: &Store, ai: &str) -> Result<()> {
    loop {
        let cursor: String = store
            .db
            .query_row(
                "SELECT cursor FROM conscious_cursors WHERE ai=?1",
                [ai],
                |r| r.get(0),
            )
            .optional()?
            .unwrap_or_else(|| "0".into());
        let page = api.changes(&cursor).await?;
        store.invalidate(ai, &page)?;
        if !page.has_more {
            break;
        }
    }
    let scopes: Vec<MaterialScope> = store
        .db
        .prepare("SELECT cid,epoch FROM conscious_scopes WHERE ai=?1 AND epoch<>announced")?
        .query_map([ai], |r| {
            Ok(MaterialScope {
                conversation_id: r.get(0)?,
                revision: r.get(1)?,
            })
        })?
        .collect::<rusqlite::Result<_>>()?;
    for scope in scopes {
        match api.announce(&scope).await {
            Ok(_) | Err(NodeError::Api { status: 403, .. }) => {
                store.db.execute(
                    "UPDATE conscious_scopes SET announced=?3 WHERE ai=?1 AND cid=?2 AND epoch=?3",
                    params![ai, scope.conversation_id, scope.revision],
                )?;
            }
            Err(e) => return Err(e.into()),
        }
    }
    Ok(())
}
fn selection(binding: &SummaryModel) -> Result<ModelSelection> {
    Ok(ModelSelection {
        provider: binding.provider_id.clone(),
        model: binding.model_id.clone(),
        reasoning_effort: binding
            .reasoning_effort
            .as_ref()
            .map(|s| serde_json::from_value(serde_json::Value::String(s.clone())))
            .transpose()?,
    })
}
fn summary_options(
    router: &ModelRouter,
    b: &SummaryModel,
) -> Result<(ai_client::ResolvedSelection, common::GenerationOptions)> {
    let resolved = router.resolve(&selection(b)?)?;
    let options = builtin_agent::generation::options(
        &builtin_agent::generation::Parameters {
            temperature: None,
            max_tokens: Some(b.max_tokens),
        },
        &resolved,
    )?;
    Ok((resolved, options))
}
fn choices(router: &ModelRouter) -> Vec<SummaryModelChoice> {
    router
        .list_models()
        .into_iter()
        .map(|m| SummaryModelChoice {
            provider_id: m.provider,
            model_id: m.spec.id,
            name: m.spec.display_name,
            reasoning_efforts: m
                .spec
                .reasoning
                .map(|r| {
                    r.levels
                        .into_iter()
                        .map(|e| e.as_wire().to_string())
                        .collect()
                })
                .unwrap_or_default(),
        })
        .collect()
}
fn view(
    store: &Store,
    ai: &str,
    cid: &str,
    summary: bool,
    router: &ModelRouter,
) -> Result<ConsciousnessView> {
    Ok(if summary {
        ConsciousnessView {
            memories: vec![],
            summaries: store
                .summaries(ai, cid)?
                .into_iter()
                .map(|s| s.item)
                .collect(),
            settings: Some(store.settings(ai, cid)?),
            models: choices(router),
        }
    } else {
        ConsciousnessView {
            memories: store.memories(ai, cid)?,
            summaries: vec![],
            settings: None,
            models: vec![],
        }
    })
}
async fn validate_source(
    api: &NodeClient,
    c: &NodeConversation,
    source: &Option<SourceRevision>,
) -> Result<()> {
    let sources = source
        .iter()
        .map(|s| ScopedSource {
            conversation_id: c.id.clone(),
            message_id: s.message_id.clone(),
            revision: s.revision.clone(),
        })
        .collect();
    ensure!(
        api.validate(&ValidateSources {
            sources,
            conversations: vec![MaterialScope {
                conversation_id: c.id.clone(),
                revision: c.revision.clone()
            }]
        })
        .await?
        .valid,
        "source_changed"
    );
    Ok(())
}
// Keep network/state and the authenticated command identity explicit at this boundary.
#[allow(clippy::too_many_arguments)]
pub async fn manage(
    api: &NodeClient,
    store: &Store,
    config: &Config,
    ai: &str,
    request: &str,
    operator: &str,
    c: &NodeConversation,
    op: ConsciousnessOperation,
) -> Result<ConsciousnessView> {
    sync(api, store, ai).await?;
    let router = ModelRouter::new(&config.providers)?;
    let summary = matches!(
        op,
        ConsciousnessOperation::Summaries
            | ConsciousnessOperation::ConfigureSummaries { .. }
            | ConsciousnessOperation::GenerateSummary { .. }
    );
    if matches!(
        op,
        ConsciousnessOperation::Memories | ConsciousnessOperation::Summaries
    ) {
        return view(store, ai, &c.id, summary, &router);
    }
    let fingerprint = format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&(operator, &c.id, &op))?)
    );
    if let Some(old) = store
        .db
        .query_row(
            "SELECT fingerprint FROM conscious_receipts WHERE ai=?1 AND request=?2",
            params![ai, request],
            |r| r.get::<_, String>(0),
        )
        .optional()?
    {
        ensure!(old == fingerprint, "request_conflict");
        api.announce(&store.material_scope(ai, &c.id)?).await?;
        return view(store, ai, &c.id, summary, &router);
    }
    match &op {
        ConsciousnessOperation::CreateMemory { kind, text, source }
        | ConsciousnessOperation::EditMemory {
            kind, text, source, ..
        } => {
            ensure!(
                matches!(
                    kind.as_str(),
                    "preference" | "fact" | "shared_experience" | "open_topic"
                ) && !text.trim().is_empty()
                    && text.chars().count() <= 4000,
                "invalid_memory"
            );
            validate_source(api, c, source).await?;
        }
        ConsciousnessOperation::ConfigureSummaries {
            enabled, binding, ..
        } => {
            if *enabled {
                ensure!(binding.is_some(), "summary_model_required");
            }
            if let Some(binding) = binding {
                summary_options(&router, binding)?;
            }
        }
        _ => {}
    }
    let queued = if let ConsciousnessOperation::GenerateSummary { retry_id } = &op {
        Some(
            make_segment(
                api,
                store,
                config,
                ai,
                c,
                operator,
                retry_id.as_deref(),
                true,
            )
            .await?,
        )
    } else {
        None
    };
    let tx = store.db.unchecked_transaction()?;
    match op {
        ConsciousnessOperation::CreateMemory { kind, text, source } => {
            let timestamp = now();
            store.save_memory(
                ai,
                &c.id,
                &MemoryItem {
                    id: request.into(),
                    revision: "1".into(),
                    kind,
                    text,
                    status: "confirmed".into(),
                    source,
                    created_at: timestamp.clone(),
                    updated_at: timestamp,
                },
            )?;
        }
        ConsciousnessOperation::EditMemory {
            id,
            revision,
            kind,
            text,
            source,
        } => {
            let mut m = store
                .memories(ai, &c.id)?
                .into_iter()
                .find(|m| m.id == id)
                .ok_or_else(|| anyhow::anyhow!("memory_not_found"))?;
            ensure!(
                m.revision == revision && m.status != "forgotten",
                "memory_conflict"
            );
            m.revision = next(&m.revision)?;
            m.kind = kind;
            m.text = text;
            m.source = source;
            m.status = "confirmed".into();
            m.updated_at = now();
            store.save_memory(ai, &c.id, &m)?;
            store.change_epoch(ai, &c.id)?;
        }
        ConsciousnessOperation::ForgetMemory { id, revision } => {
            let mut m = store
                .memories(ai, &c.id)?
                .into_iter()
                .find(|m| m.id == id)
                .ok_or_else(|| anyhow::anyhow!("memory_not_found"))?;
            ensure!(m.revision == revision, "memory_conflict");
            m.revision = next(&m.revision)?;
            m.text.clear();
            m.status = "forgotten".into();
            m.updated_at = now();
            store.save_memory(ai, &c.id, &m)?;
            store.change_epoch(ai, &c.id)?;
        }
        ConsciousnessOperation::ConfigureSummaries {
            revision,
            enabled,
            binding,
        } => {
            let current = store.settings(ai, &c.id)?;
            ensure!(current.revision == revision, "settings_conflict");
            let settings = SummarySettings {
                revision: next(&revision)?,
                enabled,
                binding,
            };
            store.db.execute("INSERT INTO conscious_settings VALUES(?1,?2,?3) ON CONFLICT(ai,cid) DO UPDATE SET json=excluded.json",params![ai,c.id,serde_json::to_string(&settings)?])?;
            store.change_epoch(ai, &c.id)?;
            for mut record in store.summaries(ai, &c.id)? {
                if matches!(record.item.status.as_str(), "queued" | "running") {
                    record.item.status = "interrupted".into();
                    record.item.error = Some("settings_changed".into());
                    store.save_summary(ai, &c.id, &record)?;
                }
            }
        }
        ConsciousnessOperation::GenerateSummary { .. } => {
            if let Some(Some(record)) = queued {
                store.save_summary(ai, &c.id, &record)?;
            }
        }
        _ => {}
    }
    store.material_scope(ai, &c.id)?;
    store.db.execute(
        "INSERT INTO conscious_receipts VALUES(?1,?2,?3)",
        params![ai, request, fingerprint],
    )?;
    tx.commit()?;
    api.announce(&store.material_scope(ai, &c.id)?).await?;
    view(store, ai, &c.id, summary, &router)
}
pub async fn poll_state(
    api: &NodeClient,
    store: &Store,
    binding: &Binding,
    scopes: &[MaterialScope],
) -> Result<()> {
    sync(api, store, &binding.ai).await?;
    ensure!(
        store.scopes_current(&binding.ai, scopes)?,
        "source_cancelled"
    );
    Ok(())
}
/// Called only AFTER socket.recv wins its select branch. A consumed mutation is never dropped by a provider chunk.
pub async fn handle_frame(
    frame: Option<ServerFrame>,
    socket: &mut NodeConnection,
    api: &NodeClient,
    store: &Store,
    config: &Config,
    binding: &Binding,
    scopes: &[MaterialScope],
) -> Result<Option<ServerFrame>> {
    match frame {
        Some(ServerFrame::ConsciousnessRequest {
            relay_id,
            request_id,
            operator_id,
            conversation,
            operation,
        }) => {
            let response = manage(
                api,
                store,
                config,
                &binding.ai,
                &request_id,
                &operator_id,
                &conversation,
                operation,
            )
            .await;
            let (view, error) = match response {
                Ok(view) => (Some(view), None),
                Err(error) => {
                    let code = if error.downcast_ref::<ai_client::AiError>().is_some() {
                        "model_invalid"
                    } else {
                        match error.to_string().as_str() {
                            "source_changed" => "source_changed",
                            "request_conflict" | "memory_conflict" | "settings_conflict" => {
                                "revision_conflict"
                            }
                            "invalid_memory" => "invalid_memory",
                            "summary_model_required" => "summary_model_required",
                            _ => "node_storage_failed",
                        }
                    };
                    (
                        None,
                        Some(ConsciousnessError {
                            code: code.into(),
                            message: "节点未能保存或读取意识资料，请刷新后以原命令重试。".into(),
                        }),
                    )
                }
            };
            socket
                .send(&ClientFrame::ConsciousnessResponse {
                    relay_id,
                    request_id,
                    view,
                    error,
                })
                .await?;
            ensure!(
                store.scopes_current(&binding.ai, scopes)?,
                "source_cancelled"
            );

            Ok(Some(ServerFrame::TasksChanged))
        }
        Some(ServerFrame::TasksChanged) => {
            poll_state(api, store, binding, scopes).await?;
            Ok(Some(ServerFrame::TasksChanged))
        }
        other => Ok(other),
    }
}

pub fn revision(value: &str) -> Result<social::Revision> {
    Ok(serde_json::from_value(serde_json::Value::String(
        value.into(),
    ))?)
}
pub fn memory_record(ai: &str, cid: &str, m: &MemoryItem) -> Result<social::MemoryRecord> {
    let actor = ai.parse()?;
    let conversation_id = cid.parse()?;
    Ok(social::MemoryRecord {
        id: m.id.parse()?,
        owner_actor_id: actor,
        conversation_id,
        scope: social::MemoryScope::Conversation { conversation_id },
        kind: serde_json::from_value(serde_json::Value::String(m.kind.clone()))?,
        text: m.text.clone(),
        status: serde_json::from_value(serde_json::Value::String(m.status.clone()))?,
        revision: revision(&m.revision)?,
        sources: m
            .source
            .iter()
            .map(|s| {
                Ok(social::SourceRef::Message {
                    conversation_id,
                    message_id: s.message_id.parse()?,
                    revision: revision(&s.revision)?,
                })
            })
            .collect::<Result<_>>()?,
        created_by: actor,
        updated_by: actor,
        created_at: m.created_at.parse()?,
        updated_at: m.updated_at.parse()?,
    })
}
pub fn summary_record(cid: &str, s: &SummaryRecord) -> Result<social::ConversationSummary> {
    Ok(social::ConversationSummary {
        id: s.item.id.parse()?,
        conversation_id: cid.parse()?,
        from_sequence: social::Sequence::new(s.item.from_position.parse()?)?,
        through_sequence: social::Sequence::new(s.item.through_position.parse()?)?,
        text: s.item.text.clone(),
        status: serde_json::from_value(serde_json::Value::String(s.item.status.clone()))?,
        revision: revision(&s.item.revision)?,
        sources: s
            .item
            .sources
            .iter()
            .map(|src| {
                Ok(social::SourceRef::Message {
                    conversation_id: cid.parse()?,
                    message_id: src.message_id.parse()?,
                    revision: revision(&src.revision)?,
                })
            })
            .collect::<Result<_>>()?,
        binding: social::SummaryBinding {
            provider_id: s.binding.provider_id.clone(),
            model_id: s.binding.model_id.clone(),
            reasoning_effort: s.binding.reasoning_effort.clone(),
            max_tokens: s.binding.max_tokens,
        },
        settings_revision: revision(&s.settings_revision)?,
        requested_by: s.requested_by.parse()?,
        membership_revision: revision(&s.conversation_revision)?,
        participation_epoch: social::Epoch::ONE,
        attempt: social::Revision::ONE,
        error: s.item.error.clone(),
        created_at: s.item.created_at.parse()?,
        updated_at: s.item.updated_at.parse()?,
    })
}
pub async fn full_history(api: &NodeClient, cid: &str) -> Result<NodeHistory> {
    let mut page = api.history(cid, "0").await?;
    let mut all = Vec::new();
    loop {
        let count = page.messages.len();
        let after = page.messages.last().map(|m| m.position.clone());
        all.append(&mut page.messages);
        if count < 100 {
            page.messages = all;
            return Ok(page);
        }
        page = api.history(cid, after.as_deref().unwrap()).await?;
    }
}
// Segment selection keeps caller, explicit/manual authorization, and source scope separate.
#[allow(clippy::too_many_arguments)]
async fn make_segment(
    api: &NodeClient,
    store: &Store,
    config: &Config,
    ai: &str,
    c: &NodeConversation,
    operator: &str,
    retry: Option<&str>,
    manual: bool,
) -> Result<Option<SummaryRecord>> {
    let settings = store.settings(ai, &c.id)?;
    let binding = settings
        .binding
        .clone()
        .ok_or_else(|| anyhow::anyhow!("summary_model_required"))?;
    if !manual && !settings.enabled {
        return Ok(None);
    }
    let history = full_history(api, &c.id).await?;
    ensure!(
        history.conversation.revision == c.revision,
        "source_changed"
    );
    let records = store.summaries(ai, &c.id)?;
    if records
        .iter()
        .any(|r| matches!(r.item.status.as_str(), "queued" | "running"))
    {
        return Ok(None);
    }
    let previous = retry
        .map(|id| {
            records
                .iter()
                .find(|r| r.item.id == id)
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("summary_not_found"))
        })
        .transpose()?;
    if let Some(previous) = &previous {
        ensure!(
            matches!(
                previous.item.status.as_str(),
                "failed" | "interrupted" | "stale"
            ),
            "summary_not_retryable"
        );
    }
    let through = records
        .iter()
        .map(|r| r.item.through_position.parse::<u64>())
        .collect::<std::result::Result<Vec<_>, _>>()?
        .into_iter()
        .max()
        .unwrap_or(0);
    let boundary = history
        .recent_boundary
        .as_deref()
        .map(str::parse::<u64>)
        .transpose()?
        .unwrap_or(0);
    let mut messages = history
        .messages
        .into_iter()
        .filter(|m| {
            let pos = m.position.parse::<u64>().expect("cloud position");
            if let Some(previous) = &previous {
                previous.item.sources.iter().any(|s| s.message_id == m.id)
            } else {
                pos > through && pos < boundary
            }
        })
        .collect::<Vec<_>>();
    if messages.is_empty() || !manual && messages.len() < 16 {
        return Ok(None);
    }
    messages.truncate(32);
    let router = ModelRouter::new(&config.providers)?;
    let (resolved, _) = summary_options(&router, &binding)?;
    while previous.is_none() && messages.len() > 1 {
        let converted = messages
            .iter()
            .map(|m| crate::context::text_message(&c.id, m))
            .collect::<Result<Vec<_>>>()?;
        let prompt = builtin_agent::summary::summary_prompt(&converted);
        let estimate = serde_json::to_vec(&prompt)?.len() as u64;
        if estimate + binding.max_tokens as u64
            <= resolved.spec.context_window.unwrap_or(8192) as u64
        {
            break;
        }
        messages.pop();
    }
    let timestamp = now();
    let epoch = store.material_scope(ai, &c.id)?.revision;
    Ok(Some(SummaryRecord {
        item: SummaryItem {
            id: previous
                .as_ref()
                .map(|r| r.item.id.clone())
                .unwrap_or_else(crate::id),
            revision: previous
                .as_ref()
                .map(|r| next(&r.item.revision))
                .transpose()?
                .unwrap_or_else(|| "1".into()),
            status: "queued".into(),
            text: String::new(),
            from_position: messages.first().unwrap().position.clone(),
            through_position: messages.last().unwrap().position.clone(),
            sources: messages
                .iter()
                .map(|m| SourceRevision {
                    message_id: m.id.clone(),
                    revision: m.revision.clone(),
                })
                .collect(),
            error: None,
            created_at: previous
                .as_ref()
                .map(|r| r.item.created_at.clone())
                .unwrap_or_else(|| timestamp.clone()),
            updated_at: timestamp,
        },
        binding,
        settings_revision: settings.revision,
        conversation_revision: c.revision.clone(),
        epoch,
        requested_by: operator.into(),
        start_request: crate::id(),
        intent: false,
        confirmation: None,
        finished: false,
    }))
}
impl Store {
    fn reserve_summary(
        &self,
        ai: &str,
        cid: &str,
        record: &mut SummaryRecord,
        tokens: u64,
        config: &Config,
    ) -> Result<()> {
        let tx = self.db.unchecked_transaction()?;
        let day = chrono::Utc::now().format("%Y-%m-%d").to_string();
        let last: Option<String> = tx
            .query_row("SELECT value FROM meta WHERE key='last_day'", [], |r| {
                r.get(0)
            })
            .optional()?;
        ensure!(
            !last.as_ref().is_some_and(|last| day < *last),
            "clock_moved_behind_budget_day"
        );
        let (calls, used): (u64, u64) = tx
            .query_row(
                "SELECT calls,tokens FROM budget WHERE day=?1",
                [&day],
                |r| Ok((r.get::<_, i64>(0)? as u64, r.get::<_, i64>(1)? as u64)),
            )
            .optional()?
            .unwrap_or((0, 0));
        ensure!(
            calls < config.daily_calls && tokens <= config.daily_tokens.saturating_sub(used),
            "node_budget_exhausted"
        );
        record.item.status = "running".into();
        record.intent = true;
        self.save_summary(ai, cid, record)?;
        tx.execute("INSERT INTO budget VALUES(?1,?2,?3) ON CONFLICT(day) DO UPDATE SET calls=excluded.calls,tokens=excluded.tokens",params![day,i64::try_from(calls+1)?,i64::try_from(used+tokens)?])?;
        tx.execute("INSERT INTO meta VALUES('last_day',?1) ON CONFLICT(key) DO UPDATE SET value=excluded.value",[&day])?;
        tx.commit()?;
        Ok(())
    }
}
/// Queue bounded automatic segments, then run at most four in this wake. No chat task is created.
pub async fn summaries_tick(
    api: &NodeClient,
    store: &Store,
    config: &Config,
    binding: &Binding,
    socket: &mut NodeConnection,
    slot: &tokio::sync::Mutex<()>,
) -> Result<()> {
    let cids: Vec<String> = store
        .db
        .prepare("SELECT cid FROM conscious_settings WHERE ai=?1")?
        .query_map([&binding.ai], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    for cid in cids {
        if !store.settings(&binding.ai, &cid)?.enabled {
            continue;
        }
        let history =
            match crate::while_connected_with(socket, api, store, config, binding, &[], "", async {
                Ok(api.history(&cid, "0").await)
            })
            .await?
            {
                Ok(h) => h,
                Err(NodeError::Api {
                    status: 403 | 404, ..
                }) => continue,
                Err(e) => return Err(e.into()),
            };
        if let Some(record) = crate::while_connected_with(
            socket,
            api,
            store,
            config,
            binding,
            &[],
            "",
            make_segment(
                api,
                store,
                config,
                &binding.ai,
                &history.conversation,
                &binding.ai,
                None,
                false,
            ),
        )
        .await?
        {
            store.save_summary(&binding.ai, &cid, &record)?;
        }
    }
    for _ in 0..4 {
        let Ok(_slot) = slot.try_lock() else {
            return Ok(());
        };
        let mut queued = None;
        let cids: Vec<String> = store
            .db
            .prepare("SELECT DISTINCT cid FROM conscious_summaries WHERE ai=?1")?
            .query_map([&binding.ai], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        for cid in cids {
            if let Some(record) = store
                .summaries(&binding.ai, &cid)?
                .into_iter()
                .find(|s| s.item.status == "queued")
            {
                queued = Some((cid, record));
                break;
            }
        }
        let Some((cid, mut record)) = queued else {
            return Ok(());
        };
        let result = run_summary(api, store, config, binding, socket, &cid, &mut record).await;
        if result.is_err() {
            let current = store
                .summaries(&binding.ai, &cid)?
                .into_iter()
                .find(|s| s.item.id == record.item.id)
                .unwrap();
            if current.item.status == "running" || current.item.status == "queued" {
                record.item.status = "failed".into();
                record.item.error = Some("summary_failed".into());
                record.item.text.clear();
                store.save_summary(&binding.ai, &cid, &record)?;
            }
        }
    }
    Ok(())
}
async fn run_summary(
    api: &NodeClient,
    store: &Store,
    config: &Config,
    binding: &Binding,
    socket: &mut NodeConnection,
    cid: &str,
    record: &mut SummaryRecord,
) -> Result<()> {
    sync(api, store, &binding.ai).await?;
    let scope = store.material_scope(&binding.ai, cid)?;
    ensure!(
        scope.revision == record.epoch
            && store.settings(&binding.ai, cid)?.revision == record.settings_revision,
        "summary_stale"
    );
    let history = crate::while_connected_with(
        socket,
        api,
        store,
        config,
        binding,
        std::slice::from_ref(&scope),
        "",
        full_history(api, cid),
    )
    .await?;
    let mut messages = Vec::new();
    for source in &record.item.sources {
        let message = history
            .messages
            .iter()
            .find(|m| m.id == source.message_id && m.revision == source.revision)
            .ok_or_else(|| anyhow::anyhow!("source_changed"))?;
        messages.push(crate::context::text_message(cid, message)?);
    }
    let router = ModelRouter::new(&config.providers)?;
    let (resolved, options) = summary_options(&router, &record.binding)?;
    let prompt = builtin_agent::summary::summary_prompt(&messages);
    let estimate = prompt
        .iter()
        .map(|m| serde_json::to_vec(m).map(|v| v.len() as u64))
        .collect::<std::result::Result<Vec<_>, _>>()?
        .into_iter()
        .sum::<u64>();
    ensure!(
        estimate + record.binding.max_tokens as u64
            <= resolved.spec.context_window.unwrap_or(8192) as u64,
        "summary_budget_exceeded"
    );
    store.reserve_summary(
        &binding.ai,
        cid,
        record,
        resolved.spec.context_window.unwrap_or(8192) as u64,
        config,
    )?;
    api.announce(&scope).await?;
    let confirm = crate::while_connected_with(
        socket,
        api,
        store,
        config,
        binding,
        std::slice::from_ref(&scope),
        "",
        async {
            Ok(api
                .summary_start(&SummaryStart {
                    request_id: record.start_request.clone(),
                    conversation_id: cid.into(),
                    conversation_revision: record.conversation_revision.clone(),
                    sources: record.item.sources.clone(),
                    settings_revision: record.settings_revision.clone(),
                    material_scope: scope.clone(),
                })
                .await?)
        },
    )
    .await?;
    record.confirmation = Some(confirm.clone());
    store.save_summary(&binding.ai, cid, record)?;
    let request = ChatRequest {
        selection: selection(&record.binding)?,
        messages: prompt,
        options,
        tools: vec![],
        resolved: Some(resolved),
    };
    let llm = AiClient::from_config(&config.providers)?.snapshot().await;
    let active_scopes = [scope.clone()];
    let work = async {
        let mut invalidations = tokio::time::interval(std::time::Duration::from_secs(1));
        let mut stream = crate::while_connected_with(
            socket,
            api,
            store,
            config,
            binding,
            &active_scopes,
            "",
            async { Ok(llm.complete_stream(request).await?) },
        )
        .await?;
        let mut text = String::new();
        let usage;
        loop {
            tokio::select! {frame=socket.recv()=>{let frame=handle_frame(frame?,socket,api,store,config,binding,&active_scopes).await?;match frame {None|Some(ServerFrame::Error{..})=>bail!("node_disconnected"),_=>{}}},_=invalidations.tick()=>{poll_state(api,store,binding,&active_scopes).await?;},chunk=stream.next()=>match chunk {Some(Ok(ChatChunk::Delta{content}))=>{ensure!(text.len()+content.len()<=64*1024,"summary_output_limit");text.push_str(&content);},Some(Ok(ChatChunk::Done{usage:reported}))=>{usage=reported;break;},Some(Ok(ChatChunk::ReasoningDelta{..}))=>{},Some(Err(e))=>return Err(e.into()),_=>bail!("summary_incomplete")}}
        }
        ensure!(!text.trim().is_empty(), "empty_summary");
        Ok::<_, anyhow::Error>((text, usage))
    };
    let outcome = tokio::time::timeout(std::time::Duration::from_secs(120), work).await;
    let (text, usage, status) = match outcome {
        Ok(Ok((text, usage))) => (text, usage, "ready"),
        _ => (String::new(), None, "interrupted"),
    };
    sync(api, store, &binding.ai).await?;
    let valid = store.scopes_current(&binding.ai, &[scope])?
        && store.settings(&binding.ai, cid)?.revision == record.settings_revision;
    let status = if valid { status } else { "stale" };
    api.summary_finish(
        &confirm.id,
        &SummaryFinish {
            confirmation_id: confirm.confirmation_id,
            status: status.into(),
            usage: usage.map(|u| NodeUsage {
                input_tokens: u.prompt_tokens as u64,
                output_tokens: u.completion_tokens as u64,
                cached_tokens: u.cached_tokens as u64,
            }),
            error_code: if status == "ready" {
                None
            } else {
                Some("summary_interrupted".into())
            },
        },
    )
    .await?;
    // Re-read after every await; an edit/settings mutation may have run in the receiver.
    ensure!(
        store.material_scope(&binding.ai, cid)?.revision == record.epoch
            && store.settings(&binding.ai, cid)?.revision == record.settings_revision,
        "summary_stale"
    );
    record.finished = true;
    record.item.status = status.into();
    record.item.text = if status == "ready" {
        text
    } else {
        String::new()
    };
    record.item.updated_at = now();
    record.item.error = if status == "ready" {
        None
    } else {
        Some("summary_interrupted".into())
    };
    store.save_summary(&binding.ai, cid, record)?;
    Ok(())
}

/// Reconcile confirmation metadata only. This path NEVER opens a provider stream.
pub async fn recover_summaries(api: &NodeClient, store: &Store, ai: &str) -> Result<()> {
    let cids: Vec<String> = store
        .db
        .prepare("SELECT DISTINCT cid FROM conscious_summaries WHERE ai=?1")?
        .query_map([ai], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    for cid in cids {
        for mut record in store.summaries(ai, &cid)? {
            if !record.intent
                || record.finished
                || matches!(record.item.status.as_str(), "queued" | "running")
            {
                continue;
            }
            let receipt = match record.confirmation.clone() {
                Some(receipt) => receipt,
                None => match api
                    .summary_start(&SummaryStart {
                        request_id: record.start_request.clone(),
                        conversation_id: cid.clone(),
                        conversation_revision: record.conversation_revision.clone(),
                        sources: record.item.sources.clone(),
                        settings_revision: record.settings_revision.clone(),
                        material_scope: MaterialScope {
                            conversation_id: cid.clone(),
                            revision: record.epoch.clone(),
                        },
                    })
                    .await
                {
                    Ok(receipt) => {
                        record.confirmation = Some(receipt.clone());
                        store.save_summary(ai, &cid, &record)?;
                        receipt
                    }
                    Err(NodeError::Api {
                        status: 400 | 403 | 404 | 409 | 410,
                        ..
                    }) => {
                        record.finished = true;
                        store.save_summary(ai, &cid, &record)?;
                        continue;
                    }
                    Err(error) => return Err(error.into()),
                },
            };
            match api
                .summary_finish(
                    &receipt.id,
                    &SummaryFinish {
                        confirmation_id: receipt.confirmation_id,
                        status: if record.item.status == "stale" {
                            "stale"
                        } else {
                            "interrupted"
                        }
                        .into(),
                        usage: None,
                        error_code: Some("node_restarted".into()),
                    },
                )
                .await
            {
                Ok(_)
                | Err(NodeError::Api {
                    status: 400 | 403 | 404 | 409 | 410,
                    ..
                }) => {
                    record.finished = true;
                    store.save_summary(ai, &cid, &record)?;
                }
                Err(error) => return Err(error.into()),
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
