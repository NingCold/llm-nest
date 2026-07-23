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