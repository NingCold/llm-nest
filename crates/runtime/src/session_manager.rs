use std::collections::HashMap;
use std::sync::Arc;

use ai_client::ModelSelection;
use common::{Feedback, Role, SessionId};
use storage::SessionStore;

use crate::error::{Result, RuntimeError};
use crate::session::Session;

/// Replace a user turn only if the frontend still has the current history.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatEdit {
    pub user_id: String,
    pub expected_message_count: usize,
    pub expected_revision: String,
}

/// In-memory session registry with optional write-through persistence.
///
/// With a [`SessionStore`] attached (via [`SessionManager::from_store`]),
/// every mutation (create / push / rename / model / delete) is persisted
/// synchronously before the in-memory state changes — a failed write leaves
/// the in-memory state untouched and surfaces the error to the caller.
#[derive(Clone)]
pub struct SessionManager {
    sessions: HashMap<SessionId, Session>,
    store: Option<Arc<dyn SessionStore>>,
}

impl SessionManager {
    /// In-memory only; nothing is persisted.
    pub fn new() -> Self {
        Self {
            sessions: HashMap::new(),
            store: None,
        }
    }

    /// Load every persisted session from `store` and attach it for
    /// write-through persistence. A corrupt session file fails construction
    /// (fail-fast, the error names the file).
    pub fn from_store(store: Arc<dyn SessionStore>) -> Result<Self> {
        let sessions = store
            .load_sessions()?
            .into_iter()
            .map(|record| {
                let session = Session::from_record(record);
                (session.id(), session)
            })
            .collect();
        let mut manager = Self {
            sessions,
            store: Some(store),
        };
        let interrupted: Vec<_> = manager
            .sessions
            .iter()
            .filter_map(|(id, session)| {
                session
                    .run
                    .as_ref()
                    .filter(|run| run.status == common::RunStatus::Running)
                    .map(|run| (*id, run.partial.clone()))
            })
            .collect();
        for (id, partial) in interrupted {
            let mut partial = partial.unwrap_or_else(|| common::Message::assistant(""));
            partial.interruption = Some(common::Interruption::Failed("Process stopped before the run finished; recovered from checkpoint. Tools were not replayed.".into()));
            manager.finish_interrupted_turn(&id, partial)?;
            let mut recovered = manager.sessions[&id].clone();
            if let Some(run) = &mut recovered.run {
                run.status = common::RunStatus::Interrupted;
            }
            manager.persist(&recovered)?;
            manager.sessions.insert(id, recovered);
        }
        Ok(manager)
    }

    /// True when mutations are persisted to a store.
    pub fn is_persistent(&self) -> bool {
        self.store.is_some()
    }

    fn persist(&self, session: &Session) -> Result<()> {
        if let Some(store) = &self.store {
            store.save_session(&session.to_record())?;
        }
        Ok(())
    }

    fn delete_persisted(&self, id: &SessionId) -> Result<()> {
        if let Some(store) = &self.store {
            store.delete_session(id)?;
        }
        Ok(())
    }

    pub fn exists(&self, id: &SessionId) -> bool {
        self.sessions.contains_key(id)
    }

    pub fn len(&self) -> usize {
        self.sessions.len()
    }

    pub fn is_empty(&self) -> bool {
        self.sessions.is_empty()
    }

    pub fn get(&self, id: &SessionId) -> Option<&Session> {
        self.sessions.get(id)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&SessionId, &Session)> {
        self.sessions.iter()
    }

    /// Create a session and persist it.
    pub fn create(&mut self, title: Option<String>) -> Result<SessionId> {
        let session = Session::new(title);
        let id = session.id();
        self.persist(&session)?;
        self.sessions.insert(id, session);
        Ok(id)
    }

    /// Delete a session and remove it from the store. Unknown sessions error
    /// with `SessionNotFound`.
    pub fn remove(&mut self, id: &SessionId) -> Result<Session> {
        let session = self
            .sessions
            .get(id)
            .cloned()
            .ok_or(RuntimeError::SessionNotFound(*id))?;
        self.delete_persisted(id)?;
        self.sessions.remove(id);
        Ok(session)
    }

    pub fn get_messages(&self, session_id: &SessionId) -> Vec<common::Message> {
        self.sessions
            .get(session_id)
            .map(|s| s.messages().to_vec())
            .unwrap_or_default()
    }

    /// Persist title, model and the new/revised user turn as one transaction.
    pub fn begin_turn(
        &mut self,
        id: &SessionId,
        message: common::Message,
        model: ModelSelection,
        edit: Option<&ChatEdit>,
        run_id: Option<common::MessageId>,
    ) -> Result<()> {
        let mut next = self
            .sessions
            .get(id)
            .cloned()
            .ok_or(RuntimeError::SessionNotFound(*id))?;
        if let Some(edit) = edit {
            let visible: Vec<_> = next
                .messages()
                .iter()
                .enumerate()
                .filter(|(_, m)| matches!(m.role, Role::User | Role::Assistant | Role::Tool))
                .collect();
            if visible.len() != edit.expected_message_count
                || next.updated_at().to_rfc3339() != edit.expected_revision
            {
                return Err(RuntimeError::ConfigError(
                    "history changed; reload before editing".into(),
                ));
            }
            let (index, target) = visible
                .iter()
                .find(|(_, message)| message.id.is_some_and(|id| id.to_string() == edit.user_id))
                .ok_or_else(|| RuntimeError::ConfigError("edit target not found".into()))?;
            if target.role != Role::User {
                return Err(RuntimeError::ConfigError(
                    "edit target must be a user message".into(),
                ));
            }
            let index = *index;
            next.messages_mut().truncate(index);
        }
        if next.title().is_none()
            && next.messages().iter().all(|m| m.role == Role::System)
            && let Some(title) = crate::session::derive_session_title(&message.text())
        {
            next.set_title(title);
        }
        if next
            .run
            .as_ref()
            .is_some_and(|run| run.status == common::RunStatus::Running)
        {
            return Err(RuntimeError::ConfigError(
                "a run is active for this session".into(),
            ));
        }
        next.run = run_id.map(|id| common::RunCheckpoint {
            id,
            status: common::RunStatus::Running,
            partial: None,
        });
        next.set_model(model);
        next.push(message);
        self.persist(&next)?;
        self.sessions.insert(*id, next);
        Ok(())
    }

    /// Save interrupted text and close unanswered tool calls in one write-through transaction.
    /// The interruption marker keeps partial prose out of the next provider request.
    pub fn finish_interrupted_turn(
        &mut self,
        id: &SessionId,
        partial: common::Message,
    ) -> Result<()> {
        let mut next = self
            .sessions
            .get(id)
            .cloned()
            .ok_or(RuntimeError::SessionNotFound(*id))?;
        let mut pending = Vec::<common::ToolCall>::new();
        for message in next.messages() {
            if message.role == Role::User {
                pending.clear();
            }
            for part in &message.content {
                match part {
                    common::ContentPart::ToolCall(call) => pending.push(call.clone()),
                    common::ContentPart::ToolResult(result) => {
                        pending.retain(|call| call.id != result.id)
                    }
                    _ => {}
                }
            }
        }
        for call in pending {
            next.push(common::Message::new(Role::Tool, vec![common::ContentPart::ToolResult(common::ToolResult {
                id: call.id, name: call.name, content: "Turn interrupted; no completed tool result is available. Do not assume the operation succeeded.".into(), is_error: true, duration_ms: None,
            })]));
        }
        // Even an empty response needs a durable cancellation/error marker.
        if let Some(run) = &mut next.run {
            run.status = if matches!(partial.interruption, Some(common::Interruption::Cancelled)) {
                common::RunStatus::Cancelled
            } else {
                common::RunStatus::Failed
            };
            run.partial = None;
        }
        next.push(partial);
        self.persist(&next)?;
        self.sessions.insert(*id, next);
        Ok(())
    }

    pub fn checkpoint(
        &mut self,
        id: &SessionId,
        run_id: common::MessageId,
        mut partial: common::Message,
    ) -> Result<()> {
        let mut next = self
            .sessions
            .get(id)
            .cloned()
            .ok_or(RuntimeError::SessionNotFound(*id))?;
        let run = next
            .run
            .as_mut()
            .filter(|run| run.id == run_id && run.status == common::RunStatus::Running)
            .ok_or_else(|| RuntimeError::ConfigError("run is no longer active".into()))?;
        partial.id = Some(run_id);
        if run.partial.as_ref().is_some_and(|saved| {
            saved.content == partial.content && saved.reasoning == partial.reasoning
        }) {
            return Ok(());
        }
        run.partial = Some(partial);
        self.persist(&next)?;
        self.sessions.insert(*id, next);
        Ok(())
    }
    pub fn finish_run(&mut self, id: &SessionId, message: common::Message) -> Result<()> {
        let mut next = self
            .sessions
            .get(id)
            .cloned()
            .ok_or(RuntimeError::SessionNotFound(*id))?;
        if let Some(run) = &mut next.run {
            run.status = common::RunStatus::Succeeded;
            run.partial = None;
        }
        next.push(message);
        self.persist(&next)?;
        self.sessions.insert(*id, next);
        Ok(())
    }

    /// Append a message and persist the session.
    pub fn push_message(&mut self, session_id: &SessionId, message: common::Message) -> Result<()> {
        let mut next = self
            .sessions
            .get(session_id)
            .cloned()
            .ok_or(RuntimeError::SessionNotFound(*session_id))?;
        if message.role == Role::Assistant {
            if let Some(run) = &mut next.run {
                run.partial = None;
            }
        }
        next.push(message);
        self.persist(&next)?;
        self.sessions.insert(*session_id, next);
        Ok(())
    }

    /// The model selection a session remembers, if any.
    pub fn get_model(&self, session_id: &SessionId) -> Option<ModelSelection> {
        self.sessions
            .get(session_id)
            .and_then(|s| s.model().cloned())
    }

    /// Remember a model selection for a session and persist it.
    pub fn set_model(&mut self, session_id: &SessionId, model: ModelSelection) -> Result<()> {
        let mut next = self
            .sessions
            .get(session_id)
            .cloned()
            .ok_or(RuntimeError::SessionNotFound(*session_id))?;
        next.set_model(model);
        self.persist(&next)?;
        self.sessions.insert(*session_id, next);
        Ok(())
    }

    /// Rename a session and persist it.
    pub fn set_title(&mut self, session_id: &SessionId, title: impl Into<String>) -> Result<()> {
        let mut next = self
            .sessions
            .get(session_id)
            .cloned()
            .ok_or(RuntimeError::SessionNotFound(*session_id))?;
        next.set_title(title);
        self.persist(&next)?;
        self.sessions.insert(*session_id, next);
        Ok(())
    }

    pub fn feedback_by_id(
        &mut self,
        session_id: &SessionId,
        message_id: &str,
        revision: &str,
        feedback: Option<Feedback>,
    ) -> Result<()> {
        let mut next = self
            .sessions
            .get(session_id)
            .cloned()
            .ok_or(RuntimeError::SessionNotFound(*session_id))?;
        if next.updated_at().to_rfc3339() != revision {
            return Err(RuntimeError::ConfigError(
                "history changed; reload before feedback".into(),
            ));
        }
        let target = next
            .messages_mut()
            .iter_mut()
            .find(|message| message.id.is_some_and(|id| id.to_string() == message_id))
            .ok_or_else(|| RuntimeError::ConfigError("message no longer exists".into()))?;
        target.feedback = feedback;
        next.touch();
        self.persist(&next)?;
        self.sessions.insert(*session_id, next);
        Ok(())
    }
}

impl Default for SessionManager {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for SessionManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SessionManager")
            .field("sessions", &self.sessions)
            .field("persistence", &self.store.is_some())
            .finish()
    }
}

#[cfg(test)]
mod edit_tests {
    use super::*;
    use common::Message;
    fn model() -> ModelSelection {
        ModelSelection {
            provider: "fake".into(),
            model: "m".into(),
            reasoning_effort: None,
        }
    }
    #[test]
    fn edit_replaces_suffix_and_rejects_stale_revision() {
        let mut manager = SessionManager::new();
        let id = manager.create(None).unwrap();
        manager
            .push_message(&id, Message::system("system"))
            .unwrap();
        manager.push_message(&id, Message::user("old")).unwrap();
        manager
            .push_message(&id, Message::assistant("answer"))
            .unwrap();
        let edit = ChatEdit {
            user_id: manager.get_messages(&id)[1].id.unwrap().to_string(),
            expected_message_count: 2,
            expected_revision: manager.get(&id).unwrap().updated_at().to_rfc3339(),
        };
        manager
            .begin_turn(&id, Message::user("new"), model(), Some(&edit), None)
            .unwrap();
        let messages = manager.get_messages(&id);
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].role, Role::System);
        assert_eq!(messages[1].text(), "new");
        manager
            .push_message(&id, Message::assistant("new answer"))
            .unwrap();
        assert!(
            manager
                .begin_turn(&id, Message::user("stale"), model(), Some(&edit), None)
                .is_err()
        );
        assert_eq!(manager.get_messages(&id)[1].text(), "new");
    }
    #[test]
    fn feedback_index_includes_tool_messages() {
        let mut manager = SessionManager::new();
        let id = manager.create(None).unwrap();
        for message in [
            Message::user("q"),
            Message::new(Role::Tool, vec![]),
            Message::assistant("a"),
        ] {
            manager.push_message(&id, message).unwrap();
        }
        let target = manager.get_messages(&id)[2].id.unwrap().to_string();
        let revision = manager.get(&id).unwrap().updated_at().to_rfc3339();
        manager
            .feedback_by_id(&id, &target, &revision, Some(Feedback::Up))
            .unwrap();
        assert_eq!(manager.get_messages(&id)[2].feedback, Some(Feedback::Up));
        assert_eq!(manager.get_messages(&id)[1].feedback, None);
    }
    #[test]
    fn interruption_completes_only_unanswered_calls_and_survives_reload() {
        let dir = std::env::temp_dir().join(format!("llmn-interrupted-{}", SessionId::new()));
        let store = Arc::new(storage::FileSessionStore::new(&dir).unwrap());
        let mut manager = SessionManager::from_store(store.clone()).unwrap();
        let id = manager.create(None).unwrap();
        manager.push_message(&id, Message::user("q")).unwrap();
        let calls = ["a", "b"]
            .into_iter()
            .map(|id| {
                common::ContentPart::ToolCall(common::ToolCall {
                    id: id.into(),
                    name: "echo".into(),
                    arguments: "{}".into(),
                    thought_signature: None,
                })
            })
            .collect::<Vec<_>>();
        manager
            .push_message(&id, Message::new(Role::Assistant, calls))
            .unwrap();
        manager
            .push_message(
                &id,
                Message::new(
                    Role::Tool,
                    vec![common::ContentPart::ToolResult(common::ToolResult {
                        id: "a".into(),
                        name: "echo".into(),
                        content: "ok".into(),
                        is_error: false,
                        duration_ms: None,
                    })],
                ),
            )
            .unwrap();
        let mut partial = Message::assistant_with_reasoning("partial", "thinking");
        partial.interruption = Some(common::Interruption::Cancelled);
        manager.finish_interrupted_turn(&id, partial).unwrap();
        let loaded = SessionManager::from_store(store).unwrap();
        let messages = loaded.get_messages(&id);
        assert_eq!(messages.len(), 5);
        assert!(
            matches!(&messages[3].content[0],common::ContentPart::ToolResult(result) if result.id == "b" && result.is_error)
        );
        assert_eq!(
            messages[4].interruption,
            Some(common::Interruption::Cancelled)
        );
        std::fs::remove_file(dir.join(format!("{id}.json"))).unwrap();
        drop(loaded);
        drop(manager);
        std::fs::remove_file(dir.join(".llmn.lock")).unwrap();
        std::fs::remove_dir(dir).unwrap();
    }
    #[test]
    fn interrupted_save_failure_does_not_change_memory() {
        struct Reject;
        impl storage::SessionStore for Reject {
            fn load_sessions(&self) -> storage::Result<Vec<storage::SessionRecord>> {
                Ok(vec![])
            }
            fn save_session(&self, _: &storage::SessionRecord) -> storage::Result<()> {
                Err(std::io::Error::other("disk full").into())
            }
            fn delete_session(&self, _: &SessionId) -> storage::Result<()> {
                Ok(())
            }
        }
        let mut manager = SessionManager::new();
        let id = manager.create(None).unwrap();
        manager.push_message(&id, Message::user("q")).unwrap();
        manager.store = Some(Arc::new(Reject));
        let before = manager.get_messages(&id);
        let mut partial = Message::assistant("partial");
        partial.interruption = Some(common::Interruption::Cancelled);
        assert!(manager.finish_interrupted_turn(&id, partial).is_err());
        assert_eq!(manager.get_messages(&id), before);
    }
    #[test]
    fn running_checkpoint_recovers_once_without_tool_replay() {
        let dir = std::env::temp_dir().join(format!("llmn-run-{}", SessionId::new()));
        let store = Arc::new(storage::FileSessionStore::new(&dir).unwrap());
        let mut manager = SessionManager::from_store(store.clone()).unwrap();
        let id = manager.create(None).unwrap();
        let run_id = common::MessageId::new();
        manager
            .begin_turn(&id, Message::user("q"), model(), None, Some(run_id))
            .unwrap();
        manager
            .push_message(
                &id,
                Message::new(
                    Role::Assistant,
                    vec![common::ContentPart::ToolCall(common::ToolCall {
                        id: "call".into(),
                        name: "echo".into(),
                        arguments: "{}".into(),
                        thought_signature: None,
                    })],
                ),
            )
            .unwrap();
        manager
            .checkpoint(
                &id,
                run_id,
                Message::assistant_with_reasoning("saved partial", "saved thinking"),
            )
            .unwrap();
        drop(manager);
        drop(store);
        let mut recovered =
            SessionManager::from_store(Arc::new(storage::FileSessionStore::new(&dir).unwrap()))
                .unwrap();
        assert_eq!(
            recovered.get(&id).unwrap().run.as_ref().unwrap().status,
            common::RunStatus::Interrupted
        );
        assert_eq!(recovered.get(&id).unwrap().run.as_ref().unwrap().id, run_id);
        let messages = recovered.get_messages(&id);
        assert_eq!(messages.len(), 4);
        assert!(
            matches!(&messages[2].content[0], common::ContentPart::ToolResult(result) if result.id == "call" && result.is_error)
        );
        assert_eq!(messages[3].text(), "saved partial");
        let message_id = messages[0].id.unwrap().to_string();
        let old_revision = recovered.get(&id).unwrap().updated_at().to_rfc3339();
        recovered.set_title(&id, "changed").unwrap();
        assert!(
            recovered
                .feedback_by_id(&id, &message_id, &old_revision, Some(Feedback::Up))
                .is_err()
        );
        drop(recovered);
        let reopened =
            SessionManager::from_store(Arc::new(storage::FileSessionStore::new(&dir).unwrap()))
                .unwrap();
        assert_eq!(reopened.get_messages(&id).len(), 4);
        drop(reopened);
        std::fs::remove_file(dir.join(format!("{id}.json"))).unwrap();
        std::fs::remove_file(dir.join(".llmn.lock")).unwrap();
        std::fs::remove_dir(dir).unwrap();
    }
    #[test]
    fn checkpoint_child() {
        let Some(dir) = std::env::var_os("LLMN_CHECKPOINT_CHILD") else {
            return;
        };
        let dir = std::path::PathBuf::from(dir);
        let mut manager =
            SessionManager::from_store(Arc::new(storage::FileSessionStore::new(&dir).unwrap()))
                .unwrap();
        let id = manager.create(None).unwrap();
        let run_id = common::MessageId::new();
        manager
            .begin_turn(&id, Message::user("q"), model(), None, Some(run_id))
            .unwrap();
        manager
            .checkpoint(&id, run_id, Message::assistant("durable before kill"))
            .unwrap();
        std::fs::write(dir.join("ready"), id.to_string()).unwrap();
        loop {
            std::thread::sleep(std::time::Duration::from_secs(1));
        }
    }
    #[test]
    fn killed_process_recovers_checkpoint() {
        let dir = std::env::temp_dir().join(format!("llmn-kill-{}", SessionId::new()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut child = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "session_manager::edit_tests::checkpoint_child"])
            .env("LLMN_CHECKPOINT_CHILD", &dir)
            .stdout(std::process::Stdio::null())
            .spawn()
            .unwrap();
        let start = std::time::Instant::now();
        while !dir.join("ready").exists() {
            if start.elapsed().as_secs() > 5 {
                let _ = child.kill();
                panic!("checkpoint fixture failed");
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        child.kill().unwrap();
        child.wait().unwrap();
        let id: SessionId = std::fs::read_to_string(dir.join("ready"))
            .unwrap()
            .parse()
            .unwrap();
        let recovered =
            SessionManager::from_store(Arc::new(storage::FileSessionStore::new(&dir).unwrap()))
                .unwrap();
        assert_eq!(
            recovered.get_messages(&id).last().unwrap().text(),
            "durable before kill"
        );
        assert_eq!(
            recovered.get(&id).unwrap().run.as_ref().unwrap().status,
            common::RunStatus::Interrupted
        );
        drop(recovered);
        for file in [
            "ready".to_string(),
            ".llmn.lock".to_string(),
            format!("{id}.json"),
        ] {
            std::fs::remove_file(dir.join(file)).unwrap();
        }
        std::fs::remove_dir(dir).unwrap();
    }
}
