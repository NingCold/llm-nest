use common::SessionId;

#[derive(Debug, Clone)]
pub enum Command {
    CreateSession {
        title: Option<String>,
    },
    RenameSession {
        session_id: SessionId,
        title: String,
    },
    DeleteSession {
        session_id: SessionId,
    },
    CompactSession {
        session_id: SessionId,
    },
    ExportSession {
        session_id: SessionId,
        path: String,
    },
    ImportSession {
        path: String,
    },
}