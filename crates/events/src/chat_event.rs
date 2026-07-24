use serde::{Deserialize, Serialize};

use common::MessageId;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ChatEvent {
    Delta {
        message_id: MessageId,
        content: String,
    },
    Finished {
        message_id: MessageId,
    },
    Error {
        message_id: MessageId,
        error: String,
    },
    Cancelled {
        message_id: MessageId,
    },
}