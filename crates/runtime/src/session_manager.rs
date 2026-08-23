use std::collections::HashMap;
use std::sync::Arc;

use ai_client::ModelSelection;
use common::SessionId;
use storage::SessionStore;

use crate::error::{Result, RuntimeError};
use crate::session::Session;

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
        Ok(Self {
            sessions,
            store: Some(store),
        })
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

    /// Append a message and persist the session.
    pub fn push_message(&mut self, session_id: &SessionId, message: common::Message) -> Result<()> {
        let mut next = self
            .sessions
            .get(session_id)
            .cloned()
            .ok_or(RuntimeError::SessionNotFound(*session_id))?;
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
