use std::collections::HashMap;

use ai_client::ModelSelection;
use chrono::{DateTime, Utc};
use common::{ContentPart, Message, Role, SessionId};

#[derive(Debug, Clone)]
pub struct Session {
    id: SessionId,
    title: Option<String>,
    messages: Vec<Message>,
    metadata: HashMap<String, String>,
    /// Model selection this session remembers; `None` means "use the global
    /// default at request time".
    model: Option<ModelSelection>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl Session {
    pub fn new(title: Option<String>) -> Self {
        Self {
            id: SessionId::new(),
            title,
            messages: Vec::new(),
            metadata: HashMap::new(),
            model: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    pub fn with_system_prompt(prompt: impl Into<String>) -> Self {
        let mut session = Self::new(None);
        session.set_system_prompt(prompt);
        session
    }

    pub fn id(&self) -> SessionId {
        self.id
    }

    pub fn title(&self) -> Option<&str> {
        self.title.as_deref()
    }

    pub fn set_title(&mut self, title: impl Into<String>) {
        self.title = Some(title.into());
        self.updated_at = Utc::now();
    }

    /// The model selection this session remembers, if any.
    pub fn model(&self) -> Option<&ModelSelection> {
        self.model.as_ref()
    }

    /// Remember a model selection for this session.
    pub fn set_model(&mut self, model: ModelSelection) {
        self.model = Some(model);
        self.updated_at = Utc::now();
    }

    pub fn messages(&self) -> &[Message] {
        &self.messages
    }

    pub fn system_prompt(&self) -> Option<&str> {
        self.messages.first().and_then(|msg| {
            (msg.role == Role::System).then(|| {
                msg.content
                    .iter()
                    .find_map(|part| match part {
                        ContentPart::Text(t) => Some(t.as_str()),
                        _ => None,
                    })
                    .unwrap_or("")
            })
        })
    }

    pub fn has_system_prompt(&self) -> bool {
        self.messages
            .first()
            .is_some_and(|msg| msg.role == Role::System)
    }

    pub fn set_system_prompt(&mut self, prompt: impl Into<String>) {
        let prompt = ContentPart::Text(prompt.into());

        match self.messages.first_mut() {
            Some(msg) if msg.role == Role::System => {
                msg.content = vec![prompt];
            }
            _ => {
                self.messages
                    .insert(0, Message::new(Role::System, vec![prompt]));
            }
        }
        self.updated_at = Utc::now();
    }

    pub fn get_metadata(&self, key: &str) -> Option<&str> {
        self.metadata.get(key).map(String::as_str)
    }

    pub fn set_metadata(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.metadata.insert(key.into(), value.into());
        self.updated_at = Utc::now();
    }

    pub fn iter_metadata(&self) -> impl Iterator<Item = (&str, &str)> {
        self.metadata.iter().map(|(k, v)| (k.as_str(), v.as_str()))
    }

    pub fn remove_metadata(&mut self, key: &str) {
        self.metadata.remove(key);
        self.updated_at = Utc::now();
    }

    pub fn push(&mut self, message: Message) {
        self.messages.push(message);
        self.updated_at = Utc::now();
    }

    pub fn created_at(&self) -> DateTime<Utc> {
        self.created_at
    }

    pub fn updated_at(&self) -> DateTime<Utc> {
        self.updated_at
    }
}

impl Session {
    /// Full persisted snapshot of this session.
    pub fn to_record(&self) -> storage::SessionRecord {
        storage::SessionRecord {
            version: storage::SessionRecord::VERSION,
            id: self.id,
            title: self.title.clone(),
            messages: self.messages.clone(),
            metadata: self.metadata.clone(),
            model: self.model.clone(),
            created_at: self.created_at,
            updated_at: self.updated_at,
        }
    }

    /// Rebuild a session from a persisted snapshot.
    pub fn from_record(record: storage::SessionRecord) -> Self {
        Self {
            id: record.id,
            title: record.title,
            messages: record.messages,
            metadata: record.metadata,
            model: record.model,
            created_at: record.created_at,
            updated_at: record.updated_at,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_new_with_title() {
        let session = Session::new(Some("Test".into()));
        assert_eq!(session.title(), Some("Test"));
        assert!(session.messages().is_empty());
    }

    #[test]
    fn session_new_without_title() {
        let session = Session::new(None);
        assert_eq!(session.title(), None);
    }

    #[test]
    fn session_rename() {
        let mut session = Session::new(None);
        session.set_title("New Name");
        assert_eq!(session.title(), Some("New Name"));
    }

    #[test]
    fn session_push_message() {
        let mut session = Session::new(None);
        session.push(Message::user("hello"));
        assert_eq!(session.messages().len(), 1);
        assert_eq!(session.messages()[0].text(), "hello");
    }

    #[test]
    fn session_with_system_prompt() {
        let mut session = Session::with_system_prompt("you are helpful");
        assert!(session.has_system_prompt());
        assert_eq!(session.system_prompt(), Some("you are helpful"));

        session.set_system_prompt("updated prompt");
        assert_eq!(session.system_prompt(), Some("updated prompt"));
    }

    #[test]
    fn session_metadata() {
        let mut session = Session::new(None);
        session.set_metadata("key1", "value1");
        assert_eq!(session.get_metadata("key1"), Some("value1"));

        session.remove_metadata("key1");
        assert_eq!(session.get_metadata("key1"), None);
    }

    #[test]
    fn session_id_is_stable() {
        let session = Session::new(Some("id".into()));
        let id = session.id();
        let retrieved = session.id();
        assert_eq!(id, retrieved);
    }

    #[test]
    fn session_timestamps() {
        let session = Session::new(None);
        assert!(session.created_at() <= session.updated_at());
    }
}
