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
}

impl FileSessionStore {
    /// Create the store, creating `dir` (and parents) if needed.
    pub fn new(dir: impl Into<PathBuf>) -> Result<Self> {
        let dir = dir.into();
        fs::create_dir_all(&dir)?;
        Ok(Self { dir })
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
        fs::write(&tmp, json)?;
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
}
