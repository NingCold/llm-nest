use std::fs;
use std::path::{Path, PathBuf};

use common::SessionId;

use crate::error::{Result, StorageError};
use crate::record::SessionRecord;
use crate::store::SessionStore;

/// One JSON file per session: `<dir>/<session-id>.json`.
///
/// Writes are atomic (temp file in the same directory + rename), so a crash
/// mid-write never truncates a session file; a stale temp file is simply
/// ignored by `load_sessions` (its extension is `.tmp`).
pub struct FileSessionStore {
    dir: PathBuf,
    _lock: fs::File,
}

impl FileSessionStore {
    /// Create the store, creating `dir` (and parents) if needed.
    pub fn new(dir: impl Into<PathBuf>) -> Result<Self> {
        let dir = dir.into();
        fs::create_dir_all(&dir)?;
        let lock = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(dir.join(".llmn.lock"))?;
        lock.try_lock().map_err(|error| {
            std::io::Error::other(format!(
                "data directory is already in use ({}): {error}",
                dir.display()
            ))
        })?;
        Ok(Self { dir, _lock: lock })
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn path_for(&self, id: &SessionId) -> PathBuf {
        self.dir.join(format!("{id}.json"))
    }
}

impl SessionStore for FileSessionStore {
    fn save_session(&self, record: &SessionRecord) -> Result<()> {
        let path = self.path_for(&record.id);
        let json = serde_json::to_string_pretty(record)?;
        let tmp = self.dir.join(format!(".{}.tmp", record.id));
        use std::io::Write;
        let mut file = fs::File::create(&tmp)?;
        file.write_all(json.as_bytes())?;
        file.sync_all()?;
        drop(file);
        fs::rename(&tmp, &path)?;
        Ok(())
    }

    fn load_sessions(&self) -> Result<Vec<SessionRecord>> {
        let mut records = Vec::new();
        for entry in fs::read_dir(&self.dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().is_some_and(|ext| ext == "json") {
                let text = fs::read_to_string(&path)?;
                let record: SessionRecord =
                    serde_json::from_str(&text).map_err(|source| StorageError::CorruptFile {
                        path: path.clone(),
                        source,
                    })?;
                let mut record = record;
                let mut migrated = false;
                let mut ids = std::collections::HashSet::new();
                for message in &mut record.messages {
                    if message.id.is_none() {
                        message.id = Some(common::MessageId::new());
                        migrated = true;
                    }
                    if !ids.insert(message.id) {
                        return Err(std::io::Error::other(format!(
                            "duplicate message ID in {}",
                            path.display()
                        ))
                        .into());
                    }
                }
                if migrated {
                    self.save_session(&record)?;
                }
                records.push(record);
            }
        }
        Ok(records)
    }

    fn delete_session(&self, id: &SessionId) -> Result<()> {
        match fs::remove_file(self.path_for(id)) {
            Ok(()) => Ok(()),
            // Idempotent: the session may already be gone (or was never stored).
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use ai_client::ModelSelection;
    use chrono::Utc;
    use common::{Message, SessionId};

    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("llmn-storage-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    fn record(id: SessionId, title: Option<&str>, messages: Vec<Message>) -> SessionRecord {
        SessionRecord {
            version: SessionRecord::VERSION,
            run: None,
            id,
            title: title.map(String::from),
            messages,
            metadata: HashMap::new(),
            model: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    #[test]
    fn new_creates_directory() {
        let dir = temp_dir("new");
        let store = FileSessionStore::new(&dir).unwrap();
        assert!(store.dir().exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn save_load_roundtrip() {
        let dir = temp_dir("roundtrip");
        let store = FileSessionStore::new(&dir).unwrap();
        let id = SessionId::new();
        let mut r = record(
            id,
            Some("hello"),
            vec![Message::user("hi"), Message::assistant("yo")],
        );
        r.model = Some(ModelSelection {
            provider: "deepseek".into(),
            model: "deepseek-chat".into(),
            reasoning_effort: None,
        });
        store.save_session(&r).unwrap();

        let loaded = store.load_sessions().unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0], r);
        assert_eq!(loaded[0].messages[1].text(), "yo");
        assert_eq!(loaded[0].model.as_ref().unwrap().model, "deepseek-chat");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn multiple_sessions_all_load() {
        let dir = temp_dir("multi");
        let store = FileSessionStore::new(&dir).unwrap();
        store
            .save_session(&record(SessionId::new(), None, vec![]))
            .unwrap();
        store
            .save_session(&record(SessionId::new(), None, vec![]))
            .unwrap();
        assert_eq!(store.load_sessions().unwrap().len(), 2);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn save_updates_existing_session() {
        let dir = temp_dir("update");
        let store = FileSessionStore::new(&dir).unwrap();
        let id = SessionId::new();
        store
            .save_session(&record(id, Some("old"), vec![Message::user("a")]))
            .unwrap();
        store
            .save_session(&record(
                id,
                Some("new"),
                vec![Message::user("a"), Message::user("b")],
            ))
            .unwrap();
        let loaded = store.load_sessions().unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].title.as_deref(), Some("new"));
        assert_eq!(loaded[0].messages.len(), 2);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn delete_removes_file_and_is_idempotent() {
        let dir = temp_dir("delete");
        let store = FileSessionStore::new(&dir).unwrap();
        let id = SessionId::new();
        store.save_session(&record(id, None, vec![])).unwrap();
        assert_eq!(store.load_sessions().unwrap().len(), 1);
        store.delete_session(&id).unwrap();
        assert!(store.load_sessions().unwrap().is_empty());
        // deleting a session that is not stored is not an error
        store.delete_session(&id).unwrap();
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn corrupt_file_is_reported_with_path() {
        let dir = temp_dir("corrupt");
        let store = FileSessionStore::new(&dir).unwrap();
        fs::write(dir.join("broken.json"), "{ not json").unwrap();
        let err = store.load_sessions().unwrap_err();
        match err {
            StorageError::CorruptFile { path, .. } => {
                assert_eq!(path, dir.join("broken.json"));
            }
            other => panic!("expected CorruptFile, got {other:?}"),
        }
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn non_json_files_are_ignored() {
        let dir = temp_dir("nonjson");
        let store = FileSessionStore::new(&dir).unwrap();
        fs::write(dir.join("readme.txt"), "hi").unwrap();
        fs::write(dir.join(".stale.tmp"), "tmp").unwrap();
        assert!(store.load_sessions().unwrap().is_empty());
        let _ = fs::remove_dir_all(&dir);
    }
    #[test]
    fn lock_child() {
        let Some(dir) = std::env::var_os("LLMN_LOCK_CHILD") else {
            return;
        };
        let store = FileSessionStore::new(PathBuf::from(dir)).unwrap();
        fs::write(store.dir().join("ready"), b"ready").unwrap();
        loop {
            std::thread::sleep(std::time::Duration::from_secs(1));
        }
    }
    #[test]
    fn cross_process_lock_is_released_after_kill() {
        let dir = std::env::temp_dir().join(format!("llmn-lock-{}", SessionId::new()));
        fs::create_dir_all(&dir).unwrap();
        let mut child = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "file::tests::lock_child"])
            .env("LLMN_LOCK_CHILD", &dir)
            .stdout(std::process::Stdio::null())
            .spawn()
            .unwrap();
        let started = std::time::Instant::now();
        while !dir.join("ready").exists() {
            if started.elapsed().as_secs() > 5 {
                let _ = child.kill();
                panic!("lock fixture did not start");
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(FileSessionStore::new(&dir).is_err());
        child.kill().unwrap();
        child.wait().unwrap();
        let reopened = FileSessionStore::new(&dir).unwrap();
        drop(reopened);
        fs::remove_file(dir.join("ready")).unwrap();
        fs::remove_file(dir.join(".llmn.lock")).unwrap();
        fs::remove_dir(dir).unwrap();
    }
    #[test]
    fn legacy_message_ids_are_written_once() {
        let dir = std::env::temp_dir().join(format!("llmn-ids-{}", SessionId::new()));
        let store = FileSessionStore::new(&dir).unwrap();
        let id = SessionId::new();
        let mut legacy = Message::user("old");
        legacy.id = None;
        store.save_session(&record(id, None, vec![legacy])).unwrap();
        let first = store.load_sessions().unwrap()[0].messages[0].id.unwrap();
        assert_eq!(
            store.load_sessions().unwrap()[0].messages[0].id,
            Some(first)
        );
        drop(store);
        let store = FileSessionStore::new(&dir).unwrap();
        assert_eq!(
            store.load_sessions().unwrap()[0].messages[0].id,
            Some(first)
        );
        drop(store);
        fs::remove_file(dir.join(format!("{id}.json"))).unwrap();
        fs::remove_file(dir.join(".llmn.lock")).unwrap();
        fs::remove_dir(dir).unwrap();
    }
}
