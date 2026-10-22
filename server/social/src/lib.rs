mod account_control;
mod accounts;
mod consciousness;
mod contact_intent;
mod controls;
mod execution;
mod export;
pub use export::{ConversationArchive, ConversationExport, ExportFormat, export_file_name};
mod files;
pub use files::{DownloadFile, FileInput};
mod feature;
mod generation;
mod images;
pub use images::{ImageInput, ImagePayload};
mod legacy;
mod memory;
mod message_list;
mod reference;
mod subscription;
mod summary;
pub mod wire;

pub use accounts::{
    AccountInput, AdministratorInput, ContactBlockInput, ContactInput as SetContactInput,
};
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
    #[error("文件导出失败：{0}")]
    FileExport(String),
    #[error("本次消息、图片、角色设定或引用内容超出上下文预算")]
    BudgetExceeded,
    #[error("社交服务尚未初始化或已关闭")]
    Unavailable,
}

pub type Result<T> = std::result::Result<T, SocialError>;

impl From<builtin_agent::AgentError> for SocialError {
    fn from(error: builtin_agent::AgentError) -> Self {
        match error {
            builtin_agent::AgentError::Ai(error) => Self::Ai(error),
            builtin_agent::AgentError::Invalid(message) => Self::Invalid(message),
            builtin_agent::AgentError::ModelUnavailable(message) => Self::ModelUnavailable(message),
            builtin_agent::AgentError::BudgetExceeded => Self::BudgetExceeded,
        }
    }
}
