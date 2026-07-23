use common::{RequestId, SessionId};

#[derive(Debug, Clone)]
pub enum RuntimeEvent {
    SessionCreated {
        session_id: SessionId,
    },
    SessionChange {
        session_id: SessionId,
    },
    SessionDeleted {
        session_id: SessionId,
    },
    ResponseStarted {
        request_id: RequestId,
        session_id: SessionId,
    },
    ResponseDelta {
        request_id: RequestId,
        session_id: SessionId,
        delta: String,
    },
    ResponseFinished {
        request_id: RequestId,
        session_id: SessionId,
    },
    ToolCallStarted {
        session_id: SessionId,
        tool: String,
    },
    ToolCallFinished {
        session_id: SessionId,
        tool: String,
    },
    Error {
        request_id: RequestId,
        session_id: Option<SessionId>,
        kind: String,
        message: String,
    }
}