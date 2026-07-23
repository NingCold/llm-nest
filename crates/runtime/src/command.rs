use common::{ChatOptions, RequestId, SessionId};

#[derive(Debug, Clone)]
pub enum Command {
    SendMessage {
        session_id: SessionId,
        request_id: RequestId,
        content: String,
        options: ChatOptions,
    },

    CreateSession{
        title: Option<String>,
    },

    RenameSession {
        session_id: SessionId,
        title: String,
    },

    DeleteSession {
        session_id: SessionId,
    },
}