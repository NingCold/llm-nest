//! Pure context and generation policy used by the bundled Agent.
pub mod attachments;
pub mod consciousness;
pub mod generation;
mod input;
pub mod summary;
pub use input::*;

#[derive(Debug, thiserror::Error)]
pub enum AgentError {
    #[error(transparent)]
    Ai(#[from] ai_client::AiError),
    #[error("{0}")]
    Invalid(String),
    #[error("{0}")]
    ModelUnavailable(String),
    #[error("本次消息、图片、角色设定或引用内容超出上下文预算")]
    BudgetExceeded,
}
pub type Result<T> = std::result::Result<T, AgentError>;

#[cfg(test)]
mod consciousness_tests;
