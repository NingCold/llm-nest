mod account_control;
mod accounts;
mod consciousness;
mod controls;
mod execution;
mod feature;
mod generation;
mod memory;
mod reference;
mod subscription;
mod summary;
pub mod wire;

pub use accounts::{AccountInput, AdministratorInput, ContactInput as SetContactInput};
pub use feature::SocialFeature;
pub use subscription::SocialSubscription;

#[derive(Debug, thiserror::Error)]
pub enum SocialError {
    #[error(transparent)]
    Storage(#[from] storage::social::SocialStorageError),
    #[error(transparent)]
    Ai(#[from] ai_client::AiError),
    #[error("{0}")]
    Invalid(String),
    #[error("{0}")]
    ModelUnavailable(String),
    #[error("本次消息、角色设定或引用内容超出上下文预算")]
    BudgetExceeded,
    #[error("社交服务尚未初始化或已关闭")]
    Unavailable,
}

pub type Result<T> = std::result::Result<T, SocialError>;

#[cfg(test)]
mod consciousness_tests;
