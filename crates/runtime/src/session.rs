use std::collections::HashMap;

use chrono::{DateTime, Utc};
use common::{Message, Role, SessionId};

#[derive(Debug, Clone)]
pub struct Session {
    id: SessionId,
    title: Option<String>,
    messages: Vec<Message>,
    metadata: HashMap<String, String>,
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

    pub fn messages(&self) -> &[Message] {
        &self.messages
    }

    pub fn system_prompt(&self) -> Option<&str> {
        self.messages.first().and_then(|msg| {
            (msg.role == Role::System).then_some(msg.content.as_str())
        })
    }

    pub fn has_system_prompt(&self) -> bool {
        self.messages
            .first()
            .is_some_and(|msg| msg.role == Role::System)
    }

    pub fn set_system_prompt(&mut self, prompt: impl Into<String>) {
        let prompt = prompt.into();

        match self.messages.first_mut() {
            Some(msg) if msg.role == Role::System => {
                msg.content = prompt;
            }
            _ => {
                self.messages.insert(0, Message::new(Role::System, &prompt));
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