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