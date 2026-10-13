use crate::{ActorId, ConversationId, MessageId, Revision, RunId, Sequence};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Social content has no provider role or tool execution blocks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SocialContent {
    Text {
        text: String,
    },
    Image {
        mime: Option<String>,
        data: Vec<u8>,
    },
    File {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        mime: String,
        data: Vec<u8>,
    },
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct MessageBody(pub Vec<SocialContent>);
impl MessageBody {
    pub fn text(text: impl Into<String>) -> Self {
        Self(vec![SocialContent::Text { text: text.into() }])
    }
    pub fn is_empty(&self) -> bool {
        self.0
            .iter()
            .all(|p| matches!(p, SocialContent::Text { text } if text.trim().is_empty()))
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageStatus {
    Committed,
    Generating,
    Interrupted,
    Withdrawn,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SocialMessage {
    pub id: MessageId,
    pub conversation_id: ConversationId,
    pub sender_id: ActorId,
    pub sender_name: String,
    pub sequence: Sequence,
    pub body: MessageBody,
    pub reply_to: Option<MessageId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_reference: Option<crate::ContextReference>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mentions: Vec<ActorId>,
    pub run_id: Option<RunId>,
    pub status: MessageStatus,
    pub revision: Revision,
    pub created_at: DateTime<Utc>,
}
