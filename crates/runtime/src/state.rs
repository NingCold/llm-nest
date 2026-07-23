use std::collections::HashMap;

use common::SessionId;

use crate::session::Session;

#[derive(Debug, Clone)]
pub struct RuntimeState {
    sessions: HashMap<SessionId, Session>,
}

impl RuntimeState {
    pub fn new() -> Self {
        Self {
            sessions: HashMap::new()
        }
    }

    pub fn exists(
        &self,
        id: &SessionId,
    ) -> bool {
        self.sessions.contains_key(id)
    }

    pub fn len(&self) -> usize {
        self.sessions.len()
    }

    pub fn is_empty(&self) -> bool {
        self.sessions.is_empty()
    }

    pub fn insert(
        &mut self,
        session: Session,
    ) {
        self.sessions.insert(
            session.id(),
            session
        );
    }

    pub fn remove(
        &mut self,
        id: &SessionId,
    ) -> Option<Session> {
        self.sessions.remove(id)
    }

    pub fn get(
        &self,
        id: &SessionId,
    ) -> Option<&Session> {
        self.sessions.get(id)
    }

    pub fn get_mut(
        &mut self,
        id: &SessionId,
    ) -> Option<&mut Session> {
        self.sessions.get_mut(id)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&SessionId, &Session)> {
        self.sessions.iter()
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = (&SessionId, &mut Session)> {
        self.sessions.iter_mut()
    }
}

impl Default for RuntimeState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_empty_on_new() {
        let state = RuntimeState::new();
        assert!(state.is_empty());
        assert_eq!(state.len(), 0);
    }

    #[test]
    fn state_insert_and_get() {
        let mut state = RuntimeState::new();
        let session = Session::new(Some("s1".into()));
        let id = session.id();
        state.insert(session);
        assert!(state.exists(&id));
        assert_eq!(state.len(), 1);
        assert!(state.get(&id).is_some());
    }

    #[test]
    fn state_remove() {
        let mut state = RuntimeState::new();
        let session = Session::new(None);
        let id = session.id();
        state.insert(session);
        let removed = state.remove(&id);
        assert!(removed.is_some());
        assert!(state.is_empty());
    }

    #[test]
    fn state_remove_nonexistent() {
        let mut state = RuntimeState::new();
        let result = state.remove(&SessionId::new());
        assert!(result.is_none());
    }

    #[test]
    fn state_iter() {
        let mut state = RuntimeState::new();
        let s1 = Session::new(Some("a".into()));
        let s2 = Session::new(Some("b".into()));
        state.insert(s1);
        state.insert(s2);
        assert_eq!(state.iter().count(), 2);
    }
}