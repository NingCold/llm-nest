use common::SessionId;

#[derive(Debug, Clone)]
pub enum RuntimeEvent {
    SessionCreated { session_id: SessionId },
    SessionDeleted { session_id: SessionId },
    SessionChanged { session_id: SessionId },
    Error { kind: String, message: String },
}