use ai_client::{ModelSelection, ProviderConfig, ProviderId};
use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, path::Path};

#[derive(Serialize, Deserialize, Clone)]
pub struct Profile {
    pub revision: String,
    pub persona: String,
    pub selection: ModelSelection,
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
}
#[derive(Serialize, Deserialize)]
pub struct Config {
    pub server: String,
    pub node_id: String,
    pub node_name: String,
    pub ca_file: Option<String>,
    pub daily_calls: u64,
    pub daily_tokens: u64,
    pub providers: HashMap<ProviderId, ProviderConfig>,
    pub profile: Profile,
}
// Intentionally no Debug: these records contain the AI-specific node credential.
pub struct Binding {
    pub ai: String,
    pub version: String,
    pub secret: String,
}
pub struct BudgetLimits {
    pub calls: u64,
    pub tokens: u64,
}
pub struct PendingTask {
    pub id: String,
    pub start: String,
    pub receipt: Option<String>,
    pub result: Option<String>,
    pub draft: String,
}
pub struct Store {
    pub db: Connection,
}
#[derive(Debug)]
pub struct BudgetDenied(pub &'static str);
impl std::fmt::Display for BudgetDenied {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}
impl std::error::Error for BudgetDenied {}
impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        let db = Connection::open(path)?;
        db.execute_batch("PRAGMA journal_mode=DELETE; PRAGMA synchronous=FULL;
CREATE TABLE IF NOT EXISTS bindings(ai TEXT PRIMARY KEY,version TEXT NOT NULL,secret TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS profiles(ai TEXT PRIMARY KEY,json TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS tasks(id TEXT PRIMARY KEY,ai TEXT NOT NULL,state TEXT NOT NULL,start_json TEXT,receipt_json TEXT,result_json TEXT,draft TEXT NOT NULL DEFAULT '');
CREATE TABLE IF NOT EXISTS budget(day TEXT PRIMARY KEY,calls INTEGER NOT NULL CHECK(calls>=0),tokens INTEGER NOT NULL CHECK(tokens>=0));
CREATE TABLE IF NOT EXISTS meta(key TEXT PRIMARY KEY,value TEXT NOT NULL);")?;
        crate::consciousness::initialize(&db)?;
        Ok(Self { db })
    }
    pub fn bind(&self, binding: &Binding, profile: &Profile) -> Result<()> {
        let tx = self.db.unchecked_transaction()?;
        tx.execute("INSERT INTO bindings VALUES(?1,?2,?3) ON CONFLICT(ai) DO UPDATE SET version=excluded.version,secret=excluded.secret", params![binding.ai,binding.version,binding.secret])?;
        tx.execute(
            "INSERT INTO profiles VALUES(?1,?2) ON CONFLICT(ai) DO NOTHING",
            params![binding.ai, serde_json::to_string(profile)?],
        )?;
        tx.commit()?;
        Ok(())
    }
    pub fn bindings(&self) -> Result<Vec<Binding>> {
        Ok(self
            .db
            .prepare("SELECT ai,version,secret FROM bindings")?
            .query_map([], |r| {
                Ok(Binding {
                    ai: r.get(0)?,
                    version: r.get(1)?,
                    secret: r.get(2)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?)
    }
    pub fn profile(&self, ai: &str) -> Result<Profile> {
        let json: String =
            self.db
                .query_row("SELECT json FROM profiles WHERE ai=?1", [ai], |r| r.get(0))?;
        Ok(serde_json::from_str(&json)?)
    }
    pub fn state(&self, id: &str) -> Result<Option<String>> {
        Ok(self
            .db
            .query_row("SELECT state FROM tasks WHERE id=?1", [id], |r| r.get(0))
            .optional()?)
    }
    pub fn reserve(
        &self,
        task: &str,
        ai: &str,
        start: &connector::node::StartRequest,
        day: &str,
        tokens: u64,
        limits: BudgetLimits,
    ) -> Result<()> {
        let tx = self.db.unchecked_transaction()?;
        let last: Option<String> = tx
            .query_row("SELECT value FROM meta WHERE key='last_day'", [], |r| {
                r.get(0)
            })
            .optional()?;
        if last.as_deref().is_some_and(|last| day < last) {
            return Err(BudgetDenied("clock moved behind budget day").into());
        }
        let (calls, used): (u64, u64) = tx
            .query_row("SELECT calls,tokens FROM budget WHERE day=?1", [day], |r| {
                Ok((r.get::<_, i64>(0)? as u64, r.get::<_, i64>(1)? as u64))
            })
            .optional()?
            .unwrap_or((0, 0));
        if calls >= limits.calls || tokens > limits.tokens.saturating_sub(used) {
            return Err(BudgetDenied("daily model budget exhausted").into());
        }
        tx.execute(
            "INSERT INTO tasks(id,ai,state,start_json) VALUES(?1,?2,'intent',?3)",
            params![task, ai, serde_json::to_string(start)?],
        )?;
        tx.execute("INSERT INTO budget VALUES(?1,?2,?3) ON CONFLICT(day) DO UPDATE SET calls=excluded.calls,tokens=excluded.tokens",params![day,i64::try_from(calls+1)?,i64::try_from(used+tokens)?])?;
        tx.execute("INSERT INTO meta VALUES('last_day',?1) ON CONFLICT(key) DO UPDATE SET value=excluded.value",[day])?;
        tx.commit().context("persist model start intent")?;
        Ok(())
    }
    /// Only after the cloud explicitly confirms this invocation has not started.
    pub fn defer_unstarted(&self, id: &str, day: &str, tokens: u64) -> Result<()> {
        let tx = self.db.unchecked_transaction()?;
        let removed = tx.execute(
            "DELETE FROM tasks WHERE id=?1 AND state='intent' AND receipt_json IS NULL",
            [id],
        )?;
        anyhow::ensure!(removed == 1, "only an unstarted intent may be deferred");
        tx.execute(
            "UPDATE budget SET calls=calls-1,tokens=tokens-?2 WHERE day=?1",
            params![day, i64::try_from(tokens)?],
        )?;
        tx.commit()?;
        Ok(())
    }
    pub fn started(&self, id: &str, receipt: &connector::node::StartReceipt) -> Result<()> {
        self.db.execute(
            "UPDATE tasks SET state='started',receipt_json=?2 WHERE id=?1",
            params![id, serde_json::to_string(receipt)?],
        )?;
        Ok(())
    }
    pub fn draft(&self, id: &str, text: &str) -> Result<()> {
        self.db
            .execute("UPDATE tasks SET draft=?2 WHERE id=?1", params![id, text])?;
        Ok(())
    }
    pub fn final_result(&self, id: &str, result: &connector::node::ResultRequest) -> Result<()> {
        self.db.execute(
            "UPDATE tasks SET state='final',result_json=?2 WHERE id=?1",
            params![id, serde_json::to_string(result)?],
        )?;
        Ok(())
    }
    pub fn acknowledged(&self, id: &str) -> Result<()> {
        self.db
            .execute("UPDATE tasks SET state='acknowledged' WHERE id=?1", [id])?;
        Ok(())
    }
    pub fn pending(&self, ai: &str) -> Result<Vec<PendingTask>> {
        Ok(self.db.prepare("SELECT id,start_json,receipt_json,result_json,draft FROM tasks WHERE ai=?1 AND state IN ('intent','started','final')")?.query_map([ai],|r|Ok(PendingTask {id:r.get(0)?,start:r.get(1)?,receipt:r.get(2)?,result:r.get(3)?,draft:r.get(4)?}))?.collect::<rusqlite::Result<_>>()?)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn start() -> connector::node::StartRequest {
        connector::node::StartRequest {
            request_id: "r".into(),
            task_revision: "1".into(),
            conversation_revision: "1".into(),
            derived_sources: vec![],
            material_scopes: vec![],
            sources: vec![],
            configuration_revision: "1".into(),
        }
    }
    #[test]
    fn explicit_unstarted_deferral_releases_only_its_reservation() {
        let directory = tempfile::tempdir().unwrap();
        let s = Store::open(&directory.path().join("node.db")).unwrap();
        s.reserve(
            "deferred",
            "ai",
            &start(),
            "2026-10-22",
            20,
            BudgetLimits {
                calls: 2,
                tokens: 100,
            },
        )
        .unwrap();
        s.reserve(
            "other",
            "ai",
            &start(),
            "2026-10-22",
            30,
            BudgetLimits {
                calls: 2,
                tokens: 100,
            },
        )
        .unwrap();
        s.defer_unstarted("deferred", "2026-10-22", 20).unwrap();
        assert!(s.state("deferred").unwrap().is_none());
        let budget: (i64, i64) =
            s.db.query_row("SELECT calls,tokens FROM budget", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .unwrap();
        assert_eq!(budget, (1, 30));
        s.db.execute("UPDATE tasks SET state='started' WHERE id='other'", [])
            .unwrap();
        assert!(s.defer_unstarted("other", "2026-10-22", 30).is_err());
    }
    #[test]
    fn reservation_is_persistent_and_never_refunded() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("node.db");
        {
            let s = Store::open(&path).unwrap();
            s.reserve(
                "a",
                "ai",
                &start(),
                "2026-10-22",
                90,
                BudgetLimits {
                    calls: 2,
                    tokens: 100,
                },
            )
            .unwrap();
        }
        let s = Store::open(&path).unwrap();
        assert_eq!(s.state("a").unwrap().as_deref(), Some("intent"));
        assert!(
            s.reserve(
                "b",
                "ai",
                &start(),
                "2026-10-22",
                11,
                BudgetLimits {
                    calls: 2,
                    tokens: 100
                }
            )
            .is_err()
        );
        assert!(
            s.reserve(
                "b",
                "ai",
                &start(),
                "2026-10-21",
                1,
                BudgetLimits {
                    calls: 2,
                    tokens: 100
                }
            )
            .is_err()
        );
        s.reserve(
            "b",
            "ai",
            &start(),
            "2026-10-23",
            90,
            BudgetLimits {
                calls: 2,
                tokens: 100,
            },
        )
        .unwrap();
        assert!(
            s.reserve(
                "b",
                "ai",
                &start(),
                "2026-10-23",
                1,
                BudgetLimits {
                    calls: 2,
                    tokens: 100
                }
            )
            .is_err()
        );
    }
}
