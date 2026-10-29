//! Renderer-safe access to the independent Agent's private same-device channel.
use crate::{cloud::CloudState, local_agent::LocalAgentState};
use connector::direct::{DirectView, LocalOperation, LocalReply, LocalRequest};

async fn call(
    cloud: &CloudState,
    local: &LocalAgentState,
    operation: LocalOperation,
) -> Result<LocalReply, String> {
    let (human, directory, generation) = local.direct_directory(cloud)?;
    let reply = tokio::time::timeout(
        std::time::Duration::from_secs(120),
        connector::direct::ipc::call(
            &directory,
            &LocalRequest {
                owner_id: human.to_string(),
                operation,
            },
        ),
    )
    .await
    .map_err(|_| "本机 Agent 响应超时，发送结果未知时请使用原请求手动重试")?
    .map_err(|_| "无法连接本机 Agent，请检查节点是否正在运行")?;
    // A logout or account switch while waiting must not expose the old owner's data.
    if cloud.session_generation() != generation {
        return Err("登录身份已变化".into());
    }
    match reply {
        LocalReply::Error { error } => Err(error.message),
        reply => Ok(reply),
    }
}
fn view(reply: LocalReply) -> Result<DirectView, String> {
    match reply {
        LocalReply::View { view } => Ok(*view),
        _ => Err("本机 Agent 响应类型无效".into()),
    }
}
#[tauri::command]
pub async fn local_chat_prepare(
    cloud: tauri::State<'_, CloudState>,
    local: tauri::State<'_, LocalAgentState>,
    conversation_id: String,
    ai_id: String,
) -> Result<DirectView, String> {
    let LocalReply::Status { bindings } = call(&cloud, &local, LocalOperation::Status).await?
    else {
        return Err("本机节点状态无效".into());
    };
    let binding = bindings
        .into_iter()
        .find(|b| b.ai_id == ai_id)
        .ok_or("此 AI 当前未在本机 Agent 运行")?;
    let grant = cloud.direct_grant(&conversation_id, &binding).await?;
    view(call(&cloud, &local, LocalOperation::Prepare { ai_id, grant }).await?)
}
#[tauri::command]
pub async fn local_chat_view(
    cloud: tauri::State<'_, CloudState>,
    local: tauri::State<'_, LocalAgentState>,
    conversation_id: String,
) -> Result<DirectView, String> {
    view(call(&cloud, &local, LocalOperation::View { conversation_id }).await?)
}
#[tauri::command]
pub async fn local_chat_send(
    cloud: tauri::State<'_, CloudState>,
    local: tauri::State<'_, LocalAgentState>,
    conversation_id: String,
    request_id: String,
    text: String,
    sealed: Option<connector::crypto::SealedEnvelope>,
    message_id: Option<String>,
    key_messages: Option<Vec<serde_json::Value>>,
    encrypted_attachments: Option<Vec<connector::crypto::EncryptedAttachment>>,
    reply_to: Option<String>,
    reference: Option<connector::node::ReferenceSelection>,
    expected_conversation_revision: Option<String>,
) -> Result<DirectView, String> {
    view(
        call(
            &cloud,
            &local,
            LocalOperation::Send {
                conversation_id,
                request_id,
                text,
                sealed,
                message_id,
                key_messages: key_messages.unwrap_or_default(),
                encrypted_attachments: encrypted_attachments.unwrap_or_default(),
                reply_to,
                reference,
                expected_conversation_revision,
            },
        )
        .await?,
    )
}
#[tauri::command]
pub async fn local_chat_prepare_reference(
    cloud: tauri::State<'_, CloudState>,
    local: tauri::State<'_, LocalAgentState>,
    conversation_id: String,
    selection: connector::node::ReferenceSelection,
) -> Result<connector::node::NodeReference, String> {
    match call(
        &cloud,
        &local,
        LocalOperation::PrepareReference {
            conversation_id,
            selection,
        },
    )
    .await?
    {
        LocalReply::Reference { reference } => Ok(reference),
        _ => Err("本机 Agent 响应类型无效".into()),
    }
}
#[tauri::command]
pub async fn local_chat_attachment(
    cloud: tauri::State<'_, CloudState>,
    local: tauri::State<'_, LocalAgentState>,
    conversation_id: String,
    message_id: String,
    index: usize,
    revision: String,
) -> Result<String, String> {
    match call(
        &cloud,
        &local,
        LocalOperation::Attachment {
            conversation_id,
            message_id,
            index,
            revision,
        },
    )
    .await?
    {
        LocalReply::Attachment { data } => Ok(data),
        _ => Err("本机 Agent 响应类型无效".into()),
    }
}
#[tauri::command]
pub async fn local_chat_stop(
    cloud: tauri::State<'_, CloudState>,
    local: tauri::State<'_, LocalAgentState>,
    conversation_id: String,
) -> Result<DirectView, String> {
    view(call(&cloud, &local, LocalOperation::Stop { conversation_id }).await?)
}
#[tauri::command]
pub async fn local_chat_sync(
    cloud: tauri::State<'_, CloudState>,
    local: tauri::State<'_, LocalAgentState>,
    conversation_id: String,
) -> Result<DirectView, String> {
    view(call(&cloud, &local, LocalOperation::Sync { conversation_id }).await?)
}
