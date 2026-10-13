//! A single-window, read-only snapshot. Export never reads revision history or AI settings.
use super::{SocialStorageResult as Result, schema, sqlite::Db};
use ::social::*;
use chrono::{DateTime, Utc};
use serde::Serialize;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationTranscript {
    pub captured_at: DateTime<Utc>,
    pub conversation_id: ConversationId,
    pub title: String,
    pub kind: ConversationKind,
    pub legacy: bool,
    pub messages: Vec<TranscriptMessage>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptMessage {
    pub id: MessageId,
    pub sequence: Sequence,
    pub sender_id: ActorId,
    pub sender_name: String,
    pub created_at: DateTime<Utc>,
    pub revision: Revision,
    pub status: MessageStatus,
    pub reply_to: Option<MessageId>,
    pub context_reference: Option<ContextReference>,
    pub mentions: Vec<ActorId>,
    pub parts: Vec<TranscriptPart>,
}

#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TranscriptPart {
    Text {
        text: String,
    },
    Image {
        mime: Option<String>,
        #[serde(rename = "byteLength")]
        byte_length: usize,
    },
    File {
        mime: String,
        #[serde(rename = "byteLength")]
        byte_length: usize,
    },
}

impl From<SocialMessage> for TranscriptMessage {
    fn from(m: SocialMessage) -> Self {
        let parts = if m.status == MessageStatus::Withdrawn {
            vec![]
        } else {
            m.body
                .0
                .into_iter()
                .map(|p| match p {
                    SocialContent::Text { text } => TranscriptPart::Text { text },
                    SocialContent::Image { mime, data } => TranscriptPart::Image {
                        mime,
                        byte_length: data.len(),
                    },
                    SocialContent::File { mime, data } => TranscriptPart::File {
                        mime,
                        byte_length: data.len(),
                    },
                })
                .collect()
        };
        Self {
            id: m.id,
            sequence: m.sequence,
            sender_id: m.sender_id,
            sender_name: m.sender_name,
            created_at: m.created_at,
            revision: m.revision,
            status: m.status,
            reply_to: m.reply_to,
            mentions: m.mentions,
            context_reference: m.context_reference,
            parts,
        }
    }
}

impl Db<'_> {
    pub(super) fn conversation_transcript(
        &self,
        operator: ActorId,
        id: ConversationId,
    ) -> Result<ConversationTranscript> {
        self.operator(operator)?;
        self.member(operator, id)?;
        let c: Conversation = self.get("conversations", id)?;
        let title = if let (false, Some(rid)) = (c.legacy, c.relationship_id) {
            let r: Relationship = self.get("relationships", rid)?;
            let other = if r.participant_a == operator {
                r.participant_b
            } else {
                r.participant_a
            };
            self.get::<Actor>("actors", other)?.display_name
        } else {
            c.title
        };
        let mut statement = self
            .connection
            .prepare("SELECT payload FROM messages WHERE conversation_id=?1 ORDER BY sequence")
            .map_err(|e| schema::sql_error(self.path, e))?;
        let rows = statement
            .query_map([id.to_string()], |r| r.get::<_, String>(0))
            .map_err(|e| schema::sql_error(self.path, e))?;
        // Project one row at a time, so attachment bytes are never retained in the export.
        let messages = rows
            .map(|row| {
                let json = row.map_err(|e| schema::sql_error(self.path, e))?;
                Ok(TranscriptMessage::from(serde_json::from_str::<
                    SocialMessage,
                >(&json)?))
            })
            .collect::<Result<_>>()?;
        Ok(ConversationTranscript {
            captured_at: self.now,
            conversation_id: id,
            title,
            kind: c.kind,
            legacy: c.legacy,
            messages,
        })
    }
}
