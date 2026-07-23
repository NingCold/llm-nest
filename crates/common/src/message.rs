use serde::{Deserialize, Serialize};

use crate::role::Role;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    pub content: String,
}

impl Message {
    pub fn new(
        role: Role,
        content: impl Into<String>,
    ) -> Self {
        Self {
            role,
            content: content.into()
        }
    }

    pub fn system(content: impl Into<String>) -> Self {
        Self::new(Role::System, content)
    }

    pub fn user(content: impl Into<String>) -> Self {
        Self::new(Role::User, content)
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self::new(Role::Assistant, content)
    }

    pub fn tool(content: impl Into<String>) -> Self {
        Self::new(Role::Tool, content)
    }

    pub fn developer(content: impl Into<String>) -> Self {
        Self::new(Role::Developer, content)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn message_new_sets_role_and_content() {
        let msg = Message::new(Role::User, "hello");
        assert_eq!(msg.role, Role::User);
        assert_eq!(msg.content, "hello");
    }

    #[test]
    fn message_system_constructor() {
        let msg = Message::system("system prompt");
        assert_eq!(msg.role, Role::System);
        assert_eq!(msg.content, "system prompt");
    }

    #[test]
    fn message_user_constructor() {
        let msg = Message::user("user message");
        assert_eq!(msg.role, Role::User);
    }

    #[test]
    fn message_assistant_constructor() {
        let msg = Message::assistant("assistant reply");
        assert_eq!(msg.role, Role::Assistant);
    }

    #[test]
    fn message_developer_constructor() {
        let msg = Message::developer("developer instruction");
        assert_eq!(msg.role, Role::Developer);
    }

    #[test]
    fn message_accepts_string_slice_and_string() {
        let _ = Message::user("&str");
        let _ = Message::user(String::from("String"));
    }
}