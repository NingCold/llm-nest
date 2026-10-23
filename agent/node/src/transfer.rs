use crate::{
    consciousness::{self, SummaryRecord},
    management,
    store::{Config, Profile, Store},
};
use ai_client::{ModelRouter, ModelSelection};
use anyhow::{Context, Result, ensure};
use connector::node::{transfer::*, *};
use rusqlite::{Connection, OpenFlags, OptionalExtension, params};
use sha2::{Digest, Sha256};
use std::{collections::HashSet, fs::OpenOptions as FileOptions, io::Write, path::Path};

pub fn export(path: &Path, ai: &str, output: &Path) -> Result<()> {
    let db = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    db.execute_batch("BEGIN")?;
    let profile: Profile = serde_json::from_str(&db.query_row(
        "SELECT json FROM profiles WHERE ai=?1",
        [ai],
        |r| r.get::<_, String>(0),
    )?)?;
    let mut windows = Vec::new();
    let cids = db.prepare("SELECT cid FROM conscious_scopes WHERE ai=?1 UNION SELECT cid FROM conscious_memory WHERE ai=?1 UNION SELECT cid FROM conscious_summaries WHERE ai=?1 UNION SELECT cid FROM conscious_settings WHERE ai=?1")?.query_map([ai], |r| r.get::<_,String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
    let store = Store { db };
    // All reads remain in the transaction opened above on this connection.
    for cid in cids {
        let summaries = store
            .summaries(ai, &cid)?
            .into_iter()
            .map(|mut s| {
                if matches!(s.item.status.as_str(), "queued" | "running") {
                    s.item.status = "interrupted".into();
                    s.item.text.clear();
                    s.item.error = Some("transfer_interrupted".into());
                }
                TransferSummary {
                    item: s.item,
                    binding: s.binding,
                    requested_by: s.requested_by,
                }
            })
            .collect();
        windows.push(TransferWindow {
            conversation_id: cid.clone(),
            memories: store.memories(ai, &cid)?,
            settings: store.settings(ai, &cid)?,
            summaries,
        });
    }
    let package = AiTransfer {
        version: 1,
        package_id: crate::id(),
        origin: "node".into(),
        ai_id: ai.into(),
        profile: TransferProfile {
            persona: profile.persona,
            provider_id: profile.selection.provider,
            model_id: profile.selection.model,
            reasoning_effort: profile
                .selection
                .reasoning_effort
                .map(|r| r.as_wire().into()),
            temperature: profile.temperature,
            max_tokens: profile.max_tokens,
            auto_daily_tokens: profile.auto_daily_tokens,
        },
        windows,
    };
    let bytes = serde_json::to_vec_pretty(&package)?;
    let mut file = FileOptions::new()
        .write(true)
        .create_new(true)
        .open(output)?;
    file.write_all(&bytes)?;
    file.sync_all()?;
    Ok(())
}
fn profile(config: &Config, p: &TransferProfile) -> Result<Profile> {
    let result = Profile {
        revision: crate::id(),
        persona: p.persona.clone(),
        selection: ModelSelection {
            provider: p.provider_id.clone(),
            model: p.model_id.clone(),
            reasoning_effort: p
                .reasoning_effort
                .as_ref()
                .map(|r| serde_json::from_value(serde_json::Value::String(r.clone())))
                .transpose()?,
        },
        temperature: p.temperature,
        max_tokens: p.max_tokens,
        auto_daily_tokens: p.auto_daily_tokens,
    };
    management::validate(config, &result)?;
    Ok(result)
}
fn check(package: &AiTransfer, config: &Config) -> Result<Profile> {
    ensure!(
        package.version == 1 && matches!(package.origin.as_str(), "local" | "node"),
        "unsupported transfer format"
    );
    package.package_id.parse::<uuid::Uuid>()?;
    package.ai_id.parse::<uuid::Uuid>()?;
    let profile = profile(config, &package.profile)?;
    let router = ModelRouter::new(&config.providers)?;
    let mut windows = HashSet::new();
    for w in &package.windows {
        w.conversation_id.parse::<uuid::Uuid>()?;
        ensure!(windows.insert(&w.conversation_id), "duplicate conversation");
        w.settings.revision.parse::<u64>()?;
        let mut ids = HashSet::new();
        for m in &w.memories {
            consciousness::memory_record(&package.ai_id, &w.conversation_id, m)?;
            ensure!(ids.insert(&m.id), "duplicate memory");
            ensure!(
                matches!(m.status.as_str(), "confirmed" | "stale" | "forgotten"),
                "invalid memory status"
            );
            ensure!(
                matches!(
                    m.kind.as_str(),
                    "preference" | "fact" | "shared_experience" | "open_topic"
                ) && m.text.chars().count() <= 4000,
                "invalid memory"
            );
            ensure!(
                m.status != "confirmed" || !m.text.trim().is_empty(),
                "empty confirmed memory"
            );
        }
        if let Some(b) = &w.settings.binding {
            consciousness::summary_options(&router, b)?;
        }
        let mut ids = HashSet::new();
        for s in &w.summaries {
            consciousness::summary_options(&router, &s.binding)?;
            ensure!(ids.insert(&s.item.id), "duplicate summary");
            ensure!(
                matches!(
                    s.item.status.as_str(),
                    "ready" | "stale" | "failed" | "interrupted" | "queued" | "running"
                ),
                "invalid summary status"
            );
            ensure!(
                s.item.from_position.parse::<u64>()? <= s.item.through_position.parse::<u64>()?,
                "invalid summary range"
            );
            ensure!(
                s.item.status != "ready"
                    || (!s.item.sources.is_empty() && !s.item.text.trim().is_empty()),
                "empty ready summary"
            );
            consciousness::summary_record(
                &w.conversation_id,
                &record(s, &w.settings, "1", &crate::id()),
            )?;
        }
    }
    Ok(profile)
}
fn record(
    s: &TransferSummary,
    settings: &SummarySettings,
    revision: &str,
    epoch: &str,
) -> SummaryRecord {
    SummaryRecord {
        item: s.item.clone(),
        binding: s.binding.clone(),
        settings_revision: settings.revision.clone(),
        conversation_revision: revision.into(),
        epoch: epoch.into(),
        requested_by: s.requested_by.clone(),
        start_request: crate::id(),
        intent: false,
        confirmation: None,
        finished: true,
    }
}
fn initialize(store: &Store) -> Result<()> {
    store.db.execute_batch("CREATE TABLE IF NOT EXISTS transfer_receipts(ai TEXT PRIMARY KEY,package TEXT NOT NULL,fingerprint TEXT NOT NULL);")?;
    Ok(())
}
pub async fn import(
    config: &Config,
    store: &Store,
    ai: &str,
    path: &Path,
    expected: &str,
    preview: bool,
) -> Result<()> {
    let bytes = std::fs::read(path)?;
    let mut package: AiTransfer = serde_json::from_slice(&bytes)?;
    ensure!(package.ai_id == ai, "transfer AI does not match target");
    let profile = check(&package, config)?;
    let binding = store
        .bindings()?
        .into_iter()
        .find(|b| b.ai == ai)
        .context("AI must be paired before import")?;
    let fingerprint = format!("{:x}", Sha256::digest(&bytes));
    initialize(store)?;
    if let Some((old_id, old_hash)) = store
        .db
        .query_row(
            "SELECT package,fingerprint FROM transfer_receipts WHERE ai=?1",
            [ai],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
        )
        .optional()?
    {
        ensure!(
            old_id == package.package_id && old_hash == fingerprint,
            "AI already has a different import"
        );
        let api = crate::client(config, &binding.secret)?;
        consciousness::sync(&api, store, ai).await?;
        println!(
            "{}",
            serde_json::json!({"type":"alreadyImported","packageId":package.package_id})
        );
        return Ok(());
    }
    ensure!(
        store.profile(ai)?.revision == expected,
        "profile revision conflict"
    );
    for table in [
        "conscious_memory",
        "conscious_summaries",
        "conscious_settings",
    ] {
        let count: i64 = store.db.query_row(
            &format!("SELECT COUNT(*) FROM {table} WHERE ai=?1"),
            [ai],
            |r| r.get(0),
        )?;
        ensure!(count == 0, "target AI already has materials");
    }
    let api = crate::client(config, &binding.secret)?;
    consciousness::sync(&api, store, ai).await?;
    let mut revisions = Vec::new();
    let mut stale = 0;
    for w in &mut package.windows {
        let history = consciousness::full_history(&api, &w.conversation_id).await?;
        ensure!(
            history.conversation.members.iter().any(|m| m.id == ai),
            "AI no longer belongs to transfer window"
        );
        let sources = history
            .messages
            .iter()
            .map(|m| (m.id.as_str(), (m.revision.as_str(), m.position.as_str())))
            .collect::<std::collections::HashMap<_, _>>();
        for m in &mut w.memories {
            if m.status != "confirmed"
                || m.source.as_ref().is_some_and(|s| {
                    sources
                        .get(s.message_id.as_str())
                        .is_none_or(|(r, _)| *r != s.revision)
                })
            {
                if m.status == "confirmed" {
                    m.status = "stale".into();
                    stale += 1;
                }
                m.text.clear();
            }
        }
        for s in &mut w.summaries {
            if s.item.import_origin.is_none() {
                s.item.import_origin = Some(format!("{}:{}", package.origin, package.package_id));
            }
            if matches!(s.item.status.as_str(), "queued" | "running") {
                s.item.status = "interrupted".into();
            }
            let valid = s.item.sources.iter().all(|src| {
                sources.get(src.message_id.as_str()).is_some_and(|(r, p)| {
                    *r == src.revision
                        && p.parse::<u64>().is_ok_and(|p| {
                            p >= s.item.from_position.parse().unwrap()
                                && p <= s.item.through_position.parse().unwrap()
                        })
                })
            });
            if s.item.status != "ready" || !valid {
                if s.item.status == "ready" {
                    s.item.status = "stale".into();
                    stale += 1;
                }
                s.item.text.clear();
            }
        }
        w.settings.enabled = false;
        revisions.push(history.conversation.revision);
    }
    if preview {
        println!(
            "{}",
            serde_json::json!({"type":"importPreview","packageId":package.package_id,"windows":package.windows.len(),"stale":stale,"automaticSummaries":false,"expectedProfileRevision":store.profile(ai)?.revision})
        );
        return Ok(());
    }
    let tx = store.db.unchecked_transaction()?;
    ensure!(
        store.profile(ai)?.revision == expected,
        "profile revision conflict"
    );
    tx.execute(
        "UPDATE profiles SET json=?2 WHERE ai=?1",
        params![ai, serde_json::to_string(&profile)?],
    )?;
    for (w, revision) in package.windows.iter().zip(revisions) {
        let epoch = crate::id();
        tx.execute("INSERT INTO conscious_scopes(ai,cid,epoch,announced) VALUES(?1,?2,?3,'') ON CONFLICT(ai,cid) DO UPDATE SET epoch=excluded.epoch,announced=''",params![ai,w.conversation_id,epoch])?;
        tx.execute(
            "INSERT INTO conscious_settings VALUES(?1,?2,?3)",
            params![ai, w.conversation_id, serde_json::to_string(&w.settings)?],
        )?;
        for m in &w.memories {
            tx.execute(
                "INSERT INTO conscious_memory VALUES(?1,?2,?3,?4)",
                params![ai, w.conversation_id, m.id, serde_json::to_string(m)?],
            )?;
        }
        for s in &w.summaries {
            tx.execute(
                "INSERT INTO conscious_summaries VALUES(?1,?2,?3,?4)",
                params![
                    ai,
                    w.conversation_id,
                    s.item.id,
                    serde_json::to_string(&record(s, &w.settings, &revision, &epoch))?
                ],
            )?;
        }
    }
    tx.execute(
        "INSERT INTO transfer_receipts VALUES(?1,?2,?3)",
        params![ai, package.package_id, fingerprint],
    )?;
    tx.commit()?;
    consciousness::sync(&api, store, ai).await?;
    println!(
        "{}",
        serde_json::json!({"type":"imported","packageId":package.package_id,"profileRevision":profile.revision,"stale":stale})
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    fn config() -> Config {
        toml::from_str(
            r#"
server="http://127.0.0.1:1"
node_id="test"
node_name="test"
daily_calls=20
daily_tokens=200000
[profile]
revision="initial"
persona="test"
[profile.selection]
provider="test"
model="small"
[providers.test]
protocol="openai"
base_url="http://127.0.0.1:1/v1"
api_key={env="UNUSED_TEST_KEY"}
[providers.test.models.small]
model="small"
context_window=8192
max_tokens=4096
"#,
        )
        .unwrap()
    }
    fn package(ai: &str, cid: &str) -> AiTransfer {
        AiTransfer {
            version: 1,
            package_id: crate::id(),
            origin: "node".into(),
            ai_id: ai.into(),
            profile: TransferProfile {
                persona: "transferred persona".into(),
                provider_id: "test".into(),
                model_id: "small".into(),
                reasoning_effort: None,
                temperature: None,
                max_tokens: None,
                auto_daily_tokens: None,
            },
            windows: vec![TransferWindow {
                conversation_id: cid.into(),
                memories: vec![MemoryItem {
                    id: crate::id(),
                    revision: "1".into(),
                    kind: "fact".into(),
                    text: "private fact".into(),
                    status: "confirmed".into(),
                    source: None,
                    created_at: chrono::Utc::now().to_rfc3339(),
                    updated_at: chrono::Utc::now().to_rfc3339(),
                }],
                settings: SummarySettings {
                    revision: "1".into(),
                    enabled: false,
                    binding: None,
                },
                summaries: vec![],
            }],
        }
    }
    async fn server(ai: String, cid: String) -> (String, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let task = tokio::spawn(async move {
            loop {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut data = vec![0; 65536];
                let n = socket.read(&mut data).await.unwrap();
                let request = String::from_utf8_lossy(&data[..n]);
                let body = if request.contains("/history?") {
                    serde_json::json!({"conversation":{"id":cid,"kind":"group","name":"test","revision":"1","members":[{"id":ai,"name":"ai","bio":"","kind":"ai"}]},"messages":[],"recentBoundary":null})
                } else if request.contains("/changes?") {
                    serde_json::json!({"cursor":"10","hasMore":false,"changes":[]})
                } else {
                    serde_json::json!({"valid":true})
                };
                let body = body.to_string();
                socket.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",body.len(),body).as_bytes()).await.unwrap();
            }
        });
        (format!("http://{address}"), task)
    }
    #[tokio::test]
    async fn import_is_atomic_and_replay_does_not_restore_forgotten_memory() {
        let temp = tempfile::tempdir().unwrap();
        let store = Store::open(&temp.path().join("node.sqlite")).unwrap();
        let ai = crate::id();
        let cid = crate::id();
        let mut config = config();
        let (url, task) = server(ai.clone(), cid.clone()).await;
        config.server = url;
        store
            .bind(
                &crate::store::Binding {
                    ai: ai.clone(),
                    version: "1".into(),
                    secret: "test-secret".into(),
                },
                &config.profile,
            )
            .unwrap();
        let p = package(&ai, &cid);
        let file = temp.path().join("package.json");
        std::fs::write(&file, serde_json::to_vec(&p).unwrap()).unwrap();
        import(&config, &store, &ai, &file, "wrong", false)
            .await
            .unwrap_err();
        assert!(store.memories(&ai, &cid).unwrap().is_empty());
        import(&config, &store, &ai, &file, "initial", true)
            .await
            .unwrap();
        assert_eq!(store.profile(&ai).unwrap().revision, "initial");
        assert!(store.memories(&ai, &cid).unwrap().is_empty());
        store.db.execute_batch("CREATE TRIGGER reject_transfer BEFORE INSERT ON conscious_memory BEGIN SELECT RAISE(ABORT,'test_write_failure'); END;").unwrap();
        assert!(
            import(&config, &store, &ai, &file, "initial", false)
                .await
                .is_err()
        );
        assert_eq!(store.profile(&ai).unwrap().revision, "initial");
        assert!(store.memories(&ai, &cid).unwrap().is_empty());
        let receipts: i64 = store
            .db
            .query_row("SELECT COUNT(*) FROM transfer_receipts", [], |r| r.get(0))
            .unwrap();
        assert_eq!(receipts, 0);
        store
            .db
            .execute_batch("DROP TRIGGER reject_transfer;")
            .unwrap();
        import(&config, &store, &ai, &file, "initial", false)
            .await
            .unwrap();
        let mut memory = store.memories(&ai, &cid).unwrap().remove(0);
        memory.status = "forgotten".into();
        memory.text.clear();
        store
            .db
            .execute(
                "UPDATE conscious_memory SET json=?3 WHERE ai=?1 AND cid=?2",
                params![ai, cid, serde_json::to_string(&memory).unwrap()],
            )
            .unwrap();
        import(&config, &store, &ai, &file, "initial", false)
            .await
            .unwrap();
        assert_eq!(store.memories(&ai, &cid).unwrap()[0].status, "forgotten");
        let mut another = p.clone();
        another.package_id = crate::id();
        std::fs::write(&file, serde_json::to_vec(&another).unwrap()).unwrap();
        assert!(
            import(&config, &store, &ai, &file, "initial", false)
                .await
                .is_err()
        );
        task.abort();
    }
    #[tokio::test]
    async fn missing_sources_are_erased_and_unfinished_summaries_never_resume() {
        let temp = tempfile::tempdir().unwrap();
        let store = Store::open(&temp.path().join("node.sqlite")).unwrap();
        let ai = crate::id();
        let cid = crate::id();
        let mut config = config();
        let (url, task) = server(ai.clone(), cid.clone()).await;
        config.server = url;
        store
            .bind(
                &crate::store::Binding {
                    ai: ai.clone(),
                    version: "1".into(),
                    secret: "test-secret".into(),
                },
                &config.profile,
            )
            .unwrap();
        let mut p = package(&ai, &cid);
        p.windows[0].memories[0].source = Some(SourceRevision {
            message_id: crate::id(),
            revision: "1".into(),
        });
        p.windows[0].settings = SummarySettings {
            revision: "1".into(),
            enabled: true,
            binding: Some(SummaryModel {
                provider_id: "test".into(),
                model_id: "small".into(),
                reasoning_effort: None,
                max_tokens: 512,
            }),
        };
        let summary_binding = p.windows[0].settings.binding.clone().unwrap();
        p.windows[0].summaries.push(TransferSummary {
            item: SummaryItem {
                id: crate::id(),
                revision: "1".into(),
                status: "running".into(),
                text: "unfinished private text".into(),
                from_position: "0".into(),
                through_position: "0".into(),
                sources: vec![],
                error: None,
                created_at: chrono::Utc::now().to_rfc3339(),
                updated_at: chrono::Utc::now().to_rfc3339(),
                import_origin: Some("local:fixture".into()),
            },
            binding: summary_binding,
            requested_by: ai.clone(),
        });
        let mut unmarked = p.windows[0].summaries[0].clone();
        unmarked.item.id = crate::id();
        unmarked.item.import_origin = None;
        p.windows[0].summaries.push(unmarked);
        let file = temp.path().join("package.json");
        std::fs::write(&file, serde_json::to_vec(&p).unwrap()).unwrap();
        import(&config, &store, &ai, &file, "initial", false)
            .await
            .unwrap();
        let summaries = store.summaries(&ai, &cid).unwrap();
        assert!(
            summaries
                .iter()
                .any(|s| s.item.import_origin == Some(format!("node:{}", p.package_id)))
        );
        let memory = &store.memories(&ai, &cid).unwrap()[0];
        assert_eq!(memory.status, "stale");
        assert!(memory.text.is_empty());
        let summary = summaries
            .iter()
            .find(|s| s.item.import_origin.as_deref() == Some("local:fixture"))
            .unwrap();
        assert_eq!(summary.item.status, "interrupted");
        assert!(summary.item.text.is_empty());
        assert!(!summary.intent);
        assert!(summary.finished);
        assert!(summary.confirmation.is_none());
        assert_eq!(summary.item.import_origin.as_deref(), Some("local:fixture"));
        assert!(!store.settings(&ai, &cid).unwrap().enabled);
        task.abort();
        let (url, task) = server(crate::id(), cid.clone()).await;
        config.server = url;
        let other = Store::open(&temp.path().join("other.sqlite")).unwrap();
        other
            .bind(
                &crate::store::Binding {
                    ai: ai.clone(),
                    version: "2".into(),
                    secret: "new-secret".into(),
                },
                &config.profile,
            )
            .unwrap();
        assert!(
            import(&config, &other, &ai, &file, "initial", false)
                .await
                .is_err()
        );
        assert!(other.memories(&ai, &cid).unwrap().is_empty());
        assert_eq!(other.profile(&ai).unwrap().revision, "initial");
        task.abort();
    }
    #[test]
    fn invalid_models_and_duplicate_materials_fail_before_writing() {
        let mut p = package(&crate::id(), &crate::id());
        let config = config();
        p.profile.model_id = "missing".into();
        assert!(check(&p, &config).is_err());
        p.profile.model_id = "small".into();
        let duplicate = p.windows[0].memories[0].clone();
        p.windows[0].memories.push(duplicate);
        assert!(check(&p, &config).is_err());
    }
    #[test]
    fn readonly_export_excludes_credentials_and_never_overwrites() {
        let temp = tempfile::tempdir().unwrap();
        let db = temp.path().join("node.sqlite");
        let ai = crate::id();
        let store = Store::open(&db).unwrap();
        store
            .bind(
                &crate::store::Binding {
                    ai: ai.clone(),
                    version: "1".into(),
                    secret: "NEVER_EXPORT_SECRET".into(),
                },
                &config().profile,
            )
            .unwrap();
        let cid = crate::id();
        let item = SummaryItem {
            id: crate::id(),
            revision: "1".into(),
            status: "running".into(),
            text: "partial-private-draft".into(),
            from_position: "1".into(),
            through_position: "1".into(),
            sources: vec![],
            error: None,
            created_at: chrono::Utc::now().to_rfc3339(),
            updated_at: chrono::Utc::now().to_rfc3339(),
            import_origin: None,
        };
        let summary = SummaryRecord {
            item: item.clone(),
            binding: SummaryModel {
                provider_id: "test".into(),
                model_id: "small".into(),
                reasoning_effort: None,
                max_tokens: 512,
            },
            settings_revision: "1".into(),
            conversation_revision: "1".into(),
            epoch: crate::id(),
            requested_by: ai.clone(),
            start_request: crate::id(),
            intent: true,
            confirmation: Some(SummaryConfirmation {
                id: crate::id(),
                confirmation_id: "NEVER_EXPORT_CONFIRMATION".into(),
            }),
            finished: false,
        };
        store
            .db
            .execute(
                "INSERT INTO conscious_summaries VALUES(?1,?2,?3,?4)",
                params![ai, cid, item.id, serde_json::to_string(&summary).unwrap()],
            )
            .unwrap();
        drop(store);
        let before = std::fs::read(&db).unwrap();
        let file = temp.path().join("export.json");
        export(&db, &ai, &file).unwrap();
        assert_eq!(before, std::fs::read(&db).unwrap());
        let output = std::fs::read_to_string(&file).unwrap();
        assert!(!output.contains("NEVER_EXPORT_SECRET"));
        assert!(!output.contains("api_key"));
        assert!(!output.contains("NEVER_EXPORT_CONFIRMATION"));
        assert!(!output.contains("partial-private-draft"));
        let transferred: AiTransfer = serde_json::from_str(&output).unwrap();
        assert_eq!(
            transferred.windows[0].summaries[0].item.status,
            "interrupted"
        );
        let untouched = Connection::open_with_flags(&db, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
        let json: String = untouched
            .query_row("SELECT json FROM conscious_summaries", [], |r| r.get(0))
            .unwrap();
        assert_eq!(
            serde_json::from_str::<SummaryRecord>(&json)
                .unwrap()
                .item
                .status,
            "running"
        );
        assert!(export(&db, &ai, &file).is_err());
        assert_eq!(output, std::fs::read_to_string(file).unwrap());
    }
}
