use serde::{Deserialize, Serialize};

use common::{MessageId, MessageTimings, Usage};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ChatEvent {
    Delta {
        message_id: MessageId,
        content: String,
    },
    /// A fragment of the model's thinking chain (shown dimmed by frontends;
    /// never part of the stored assistant message).
    ReasoningDelta {
        message_id: MessageId,
        content: String,
    },
    Finished {
        message_id: MessageId,
        /// Normalized token usage of the turn; `None` when the protocol did
        /// not report it. Also persisted on the assistant message.
        #[serde(default)]
        usage: Option<Usage>,
        /// Timing statistics measured by the chat feature (ttft / thinking /
        /// total); `None` when the stream ended without completion.
        #[serde(default)]
        timings: Option<MessageTimings>,
    },
    /// The model requested a tool call; the feature will execute it and feed
    /// the result back for another model turn.
    ToolCall {
        message_id: MessageId,
        id: String,
        name: String,
        arguments: String,
    },
    /// A tool finished executing (success or error); `is_error` flags failures.
    ToolResult {
        message_id: MessageId,
        id: String,
        name: String,
        content: String,
        is_error: bool,
        /// Execution duration in milliseconds (measured around the tool run).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        duration_ms: Option<u64>,
    },
    Error {
        message_id: MessageId,
        error: String,
    },
    Cancelled {
        message_id: MessageId,
    },
}
