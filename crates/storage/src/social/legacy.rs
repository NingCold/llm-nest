use super::{
    SocialStorageError as Error, SocialStorageResult as Result, schema,
    sqlite::{Db, NewMessage},
    store::BindingCandidate,
};
use crate::SessionRecord;
use ::social::*;
use common::{ContentPart, Interruption, Message, Role, ToolResult};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::{collections::HashSet, fs, io::Write, path::Path, str::FromStr};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LegacySessionPreview {
    pub source_id: String,
    pub session_id: String,
    pub checksum: String,
    pub title: Option<String>,
    pub message_count: usize,
    pub missing_ids: usize,
    pub interrupted_checkpoint: bool,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct LegacyPreview {
    pub sessions: Vec<LegacySessionPreview>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct LegacyImportResult {
    pub imported: usize,
    pub already_imported: usize,
    pub conversations: Vec<ConversationId>,
}
pub(super) struct Source {
    preview: LegacySessionPreview,
    record: SessionRecord,
    backup_ref: String,
}
fn legacy_error(path: &Path, detail: impl ToString) -> Error {
    Error::Legacy {
        path: path.to_owned(),
        detail: detail.to_string(),
    }
}
fn io_error(path: &Path, source: std::io::Error) -> Error {
    Error::Io {
        path: path.to_owned(),
        source,
    }
}

fn read_source(
    root: &Path,
    source_id: &str,
) -> Result<(LegacySessionPreview, SessionRecord, Vec<u8>)> {
    // A preview is untrusted input; only an exact UUID filename in this guarded
    // directory is eligible. Never follow links or inspect config/cache/env files.
    let id = source_id
        .strip_suffix(".json")
        .and_then(|s| common::SessionId::from_str(s).ok())
        .ok_or_else(|| legacy_error(root.join(source_id).as_path(), "invalid source filename"))?;
    if source_id != format!("{id}.json") {
        return Err(legacy_error(root, "source filename is not canonical"));
    }
    let path = root.join(source_id);
    if !fs::symlink_metadata(&path)
        .map_err(|e| io_error(&path, e))?
        .file_type()
        .is_file()
    {
        return Err(legacy_error(&path, "source is not a regular file"));
    }
    let bytes = fs::read(&path).map_err(|e| io_error(&path, e))?;
    let raw: serde_json::Value =
        serde_json::from_slice(&bytes).map_err(|e| legacy_error(&path, e))?;
    let record: SessionRecord =
        serde_json::from_value(raw.clone()).map_err(|e| legacy_error(&path, e))?;
    if record.version != SessionRecord::VERSION || record.id != id {
        return Err(legacy_error(
            &path,
            "version or filename/record ID mismatch",
        ));
    }
    // The old DTO is permissive about unknown/malformed content blocks. Import
    // rejects them rather than silently dropping attachments or tool evidence.
    if let Some(messages) = raw.get("messages").and_then(|v| v.as_array()) {
        for (value, message) in messages.iter().zip(&record.messages) {
            validate_parts(value, message, &path)?;
        }
    }
    if let (Some(value), Some(partial)) = (
        raw.pointer("/run/partial"),
        record.run.as_ref().and_then(|r| r.partial.as_ref()),
    ) {
        validate_parts(value, partial, &path)?;
    }
    let mut ids = HashSet::new();
    for message in &record.messages {
        if let Some(id) = message.id
            && !ids.insert(id)
        {
            return Err(legacy_error(&path, format!("duplicate message ID {id}")));
        }
    }
    if let Some(run) = &record.run
        && run.status == common::RunStatus::Running
    {
        if let Some(partial) = &run.partial
            && (partial.role != Role::Assistant || partial.id.is_some_and(|id| id != run.id))
        {
            return Err(legacy_error(
                &path,
                "running checkpoint identity/role mismatch",
            ));
        }
        if let Some(existing) = record.messages.iter().find(|m| m.id == Some(run.id))
            && (existing.role != Role::Assistant
                || existing.interruption.is_none()
                || run.partial.as_ref().is_some_and(|p| {
                    p.content != existing.content || p.reasoning != existing.reasoning
                }))
        {
            return Err(legacy_error(
                &path,
                "checkpoint ID collides with committed or inconsistent message",
            ));
        }
    }
    let preview = LegacySessionPreview {
        source_id: source_id.to_owned(),
        session_id: id.to_string(),
        checksum: schema::digest(&bytes),
        title: record.title.clone(),
        message_count: record.messages.len(),
        missing_ids: record.messages.iter().filter(|m| m.id.is_none()).count(),
        interrupted_checkpoint: record
            .run
            .as_ref()
            .is_some_and(|r| r.status == common::RunStatus::Running),
    };
    Ok((preview, record, bytes))
}
fn validate_parts(value: &serde_json::Value, message: &Message, path: &Path) -> Result<()> {
    if let Some(parts) = value.get("content").and_then(|v| v.as_array())
        && parts.len() != message.content.len()
    {
        return Err(legacy_error(path, "unsupported or corrupt content block"));
    }
    Ok(())
}
pub(super) fn preview(root: &Path) -> Result<LegacyPreview> {
    let mut sessions = Vec::new();
    let mut ids = HashSet::new();
    let entries = fs::read_dir(root).map_err(|e| io_error(root, e))?;
    for entry in entries {
        let entry = entry.map_err(|e| io_error(root, e))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.ends_with(".json") {
            continue;
        }
        // Non-session JSON (e.g. catalog) is not a legacy session source.
        if common::SessionId::from_str(name.trim_end_matches(".json")).is_err() {
            continue;
        }
        let (preview, record, _) = read_source(root, &name)?;
        for message in &record.messages {
            if let Some(id) = message.id
                && !ids.insert(id)
            {
                return Err(legacy_error(
                    &entry.path(),
                    format!("cross-session duplicate message ID {id}"),
                ));
            }
        }
        if let Some(run) = record
            .run
            .filter(|r| r.status == common::RunStatus::Running)
            && !record.messages.iter().any(|m| m.id == Some(run.id))
            && !ids.insert(run.id)
        {
            return Err(legacy_error(
                &entry.path(),
                "checkpoint ID collides across sessions",
            ));
        }
        sessions.push(preview);
    }
    sessions.sort_by(|a, b| a.source_id.cmp(&b.source_id));
    Ok(LegacyPreview { sessions })
}

pub(super) fn prepare_import(root: &Path, expected: &LegacyPreview) -> Result<Vec<Source>> {
    // Re-read the full set immediately before the transaction. Changed preview,
    // hidden duplicate IDs or caller-supplied metadata cannot bypass validation.
    let current = preview(root)?;
    if &current != expected {
        return Err(legacy_error(
            root,
            "sources changed since preview; preview again",
        ));
    }
    let mut sources = Vec::new();
    for session in &expected.sessions {
        let (p, record, bytes) = read_source(root, &session.source_id)?;
        if &p != session {
            return Err(legacy_error(
                root.join(&p.source_id).as_path(),
                "source changed during preparation",
            ));
        }
        let backups = root.join("legacy-backups");
        fs::create_dir_all(&backups).map_err(|e| io_error(&backups, e))?;
        let backup_ref = format!("legacy-backups/{}-{}.json", p.session_id, p.checksum);
        let backup_path = root.join(&backup_ref);
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&backup_path)
        {
            Ok(mut file) => {
                file.write_all(&bytes)
                    .and_then(|_| file.sync_all())
                    .map_err(|e| io_error(&backup_path, e))?;
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                if fs::symlink_metadata(&backup_path)
                    .map_err(|e| io_error(&backup_path, e))?
                    .file_type()
                    .is_symlink()
                    || fs::read(&backup_path).map_err(|e| io_error(&backup_path, e))? != bytes
                {
                    return Err(legacy_error(&backup_path, "backup differs from source"));
                }
            }
            Err(e) => return Err(io_error(&backup_path, e)),
        }
        sources.push(Source {
            preview: p,
            record,
            backup_ref,
        });
    }
    Ok(sources)
}

pub(super) fn import(
    db: &Db<'_>,
    operator: ActorId,
    sources: Vec<Source>,
) -> Result<LegacyImportResult> {
    let human = db.operator(operator)?;
    let mut result = LegacyImportResult::default();
    for source in sources {
        let p = &source.preview;
        let path = db
            .path
            .parent()
            .unwrap_or(Path::new("."))
            .join(&p.source_id);
        let existing: Option<(String, String)> = db
            .connection
            .query_row(
                "SELECT checksum,conversation_id FROM legacy_imports WHERE source_id=?1",
                [&p.source_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .map_err(|e| schema::sql_error(db.path, e))?;
        if let Some((checksum, id)) = existing {
            if checksum != p.checksum {
                return Err(legacy_error(&path, "already imported source has changed"));
            }
            result.already_imported += 1;
            result
                .conversations
                .push(id.parse().map_err(|e| legacy_error(&path, e))?);
            continue;
        }
        let mut ai = db.new_actor(ActorKind::Ai, format!("历史 AI · {}", p.session_id));
        ai.status = EntityStatus::Archived;
        let binding = source.record.model.as_ref().map(|m| BindingCandidate {
            provider_id: m.provider.clone(),
            model_id: m.model.clone(),
            reasoning_effort: m.reasoning_effort.as_ref().map(|r| r.as_wire().to_owned()),
            temperature: None,
            max_tokens: None,
        });
        db.insert_ai(&ai, String::new(), binding.as_ref(), human.id)?;
        let mut c = db.new_conversation(
            ConversationKind::Group,
            None,
            p.title.clone().unwrap_or_else(|| "历史聊天".into()),
            true,
        );
        c.created_at = source.record.created_at;
        db.insert_conversation(&c, &[human.id, ai.id])?;
        db.execute(
            "INSERT INTO legacy_imports VALUES(?1,?2,?3,?4,?5,?6)",
            params![
                p.source_id,
                p.session_id,
                p.checksum,
                c.id.to_string(),
                ai.id.to_string(),
                source.backup_ref
            ],
        )?;
        let mut record = source.record;
        let recovered = recover(&mut record);
        let session_data = serde_json::json!({"version":record.version,"title":record.title,"model":record.model,"metadata":record.metadata,"created_at":record.created_at,"updated_at":record.updated_at,"run":record.run});
        db.execute(
            "INSERT INTO legacy_session_data VALUES(?1,?2)",
            params![p.source_id, serde_json::to_string(&session_data)?],
        )?;
        for (index, mut message) in record.messages.into_iter().enumerate() {
            let id: MessageId = message
                .id
                .map(|id| id.to_string())
                .unwrap_or_else(|| MessageId::new().to_string())
                .parse()
                .map_err(|e| legacy_error(&path, e))?;
            // Map every legacy entry, including trace-only ones, into one ID
            // namespace; collisions with live social messages are also errors.
            let collision:bool=db.connection.query_row("SELECT EXISTS(SELECT 1 FROM messages WHERE id=?1 UNION ALL SELECT 1 FROM legacy_id_map WHERE message_id=?1)",[id.to_string()],|r|r.get(0)).map_err(|e|schema::sql_error(db.path,e))?;
            if collision {
                return Err(legacy_error(&path, format!("message ID collision {id}")));
            }
            message.id = Some(serde_json::from_value(serde_json::Value::String(
                id.to_string(),
            ))?);
            let key = if index < recovered {
                format!("message:{index}")
            } else {
                format!("recovered:{index}")
            };
            db.execute(
                "INSERT INTO legacy_id_map VALUES(?1,?2,?3)",
                params![p.source_id, key, id.to_string()],
            )?;
            let parts = message
                .content
                .iter()
                .filter_map(|part| match part {
                    ContentPart::Text(text) => Some(SocialContent::Text { text: text.clone() }),
                    ContentPart::Image { mime, data } => Some(SocialContent::Image {
                        mime: mime.clone(),
                        data: data.clone(),
                    }),
                    ContentPart::File { mime, data } => Some(SocialContent::File {
                        mime: mime.clone(),
                        data: data.clone(),
                    }),
                    _ => None,
                })
                .collect::<Vec<_>>();
            let visible = matches!(message.role, Role::User | Role::Assistant)
                && (!parts.is_empty() || message.interruption.is_some());
            db.execute(
                "INSERT INTO legacy_entries VALUES(?1,?2,?3,?4,?5)",
                params![
                    p.source_id,
                    key,
                    id.to_string(),
                    if visible { "visible" } else { "trace" },
                    serde_json::to_string(&message)?
                ],
            )?;
            if visible {
                let sender = if message.role == Role::User {
                    &human
                } else {
                    &ai
                };
                let status = if message.interruption.is_some() {
                    MessageStatus::Interrupted
                } else {
                    MessageStatus::Committed
                };
                let mut social = db.allocate_message(
                    &mut c,
                    sender,
                    NewMessage {
                        body: MessageBody(parts),
                        status,
                        id,
                        run_id: None,
                        reply_to: None,
                        mentions: vec![],
                    },
                )?;
                if let Some(time) = message
                    .created_at
                    .and_then(|s| chrono::DateTime::from_timestamp(s, 0))
                {
                    social.created_at = time;
                    db.execute(
                        "UPDATE messages SET payload=?2 WHERE id=?1",
                        params![id.to_string(), serde_json::to_string(&social)?],
                    )?;
                    db.execute("UPDATE message_revisions SET payload=?2 WHERE message_id=?1 AND revision=1",params![id.to_string(),serde_json::to_string(&social)?])?;
                }
            }
        }
        db.execute(
            "INSERT INTO read_positions VALUES(?1,?2,?3)",
            params![
                c.id.to_string(),
                human.id.to_string(),
                c.last_sequence.get() as i64
            ],
        )?;
        result.imported += 1;
        result.conversations.push(c.id);
    }
    Ok(result)
}

/// Deterministic, offline checkpoint closure; never invokes a tool or provider.
fn recover(record: &mut SessionRecord) -> usize {
    let original_len = record.messages.len();
    let Some(run) = record
        .run
        .as_mut()
        .filter(|r| r.status == common::RunStatus::Running)
    else {
        return original_len;
    };
    let mut partial = record
        .messages
        .iter()
        .find(|m| m.id == Some(run.id))
        .cloned()
        .or_else(|| run.partial.take())
        .unwrap_or_else(|| Message::assistant(""));
    run.partial = None;
    partial.id = Some(run.id);
    partial.interruption.get_or_insert_with(|| {
        Interruption::Failed("Application stopped before this legacy run completed".into())
    });
    if let Some(existing) = record.messages.iter_mut().find(|m| m.id == Some(run.id)) {
        *existing = partial;
    } else {
        record.messages.push(partial);
    }
    let mut pending = Vec::<common::ToolCall>::new();
    for message in &record.messages {
        if message.role == Role::User {
            pending.clear();
        }
        for part in &message.content {
            match part {
                ContentPart::ToolCall(call) => {
                    if !pending.iter().any(|c| c.id == call.id) {
                        pending.push(call.clone());
                    }
                }
                ContentPart::ToolResult(result) => pending.retain(|c| c.id != result.id),
                _ => {}
            }
        }
    }
    for call in pending {
        record.messages.push(Message::new(Role::Tool,vec![ContentPart::ToolResult(ToolResult{id:call.id,name:call.name,content:"Legacy run interrupted; no completed result is available. Do not assume the operation succeeded.".into(),is_error:true,duration_ms:None})]));
    }
    run.status = common::RunStatus::Interrupted;
    original_len
}
