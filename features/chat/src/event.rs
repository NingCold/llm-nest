use std::fmt;

#[derive(Debug, Clone)]
pub enum ChatEvent {
    ResponseStarted,
    ResponseDelta { content: String },
    ResponseFinished,
    Info(String),
    Error { message: String },
}

impl fmt::Display for ChatEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ChatEvent::ResponseDelta { content } => write!(f, "{}", content),
            ChatEvent::Info(msg) => write!(f, "{}", msg),
            ChatEvent::Error { message } => write!(f, "Error: {}", message),
            _ => Ok(()),
        }
    }
}