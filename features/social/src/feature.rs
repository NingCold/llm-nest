use crate::{Result, SocialError, wire};
use ai_client::{AiClient, ModelSelection};
use runtime::feature::{BoxFuture, Feature, FeatureContext, SocialResources};
use social::*;
use std::{
    any::Any,
    collections::HashMap,
    sync::{Arc, Mutex as StdMutex},
};
use storage::social::*;
use tokio::sync::{Mutex, OnceCell, broadcast};
use tokio_util::{sync::CancellationToken, task::TaskTracker};

pub(crate) struct Services {
    pub resources: Arc<SocialResources>,
    pub llm: Arc<AiClient>,
    pub human: ActorId,
}
#[derive(Clone, Default)]
pub(crate) struct ConversationLocks {
    pub mutation: Arc<Mutex<()>>,
    pub execution: Arc<Mutex<()>>,
}

pub struct SocialFeature {
    services: OnceCell<Services>,
    locks: StdMutex<HashMap<ConversationId, ConversationLocks>>,
    pub(crate) cancellations: Mutex<HashMap<RunId, CancellationToken>>,
    pub(crate) stop: CancellationToken,
    pub(crate) tasks: TaskTracker,
    pub(crate) live: broadcast::Sender<wire::LiveEvent>,
    pub(crate) background_failures: StdMutex<Vec<SocialError>>,
}
impl Default for SocialFeature {
    fn default() -> Self {
        Self::new()
    }
}
impl SocialFeature {
    pub fn new() -> Self {
        let (live, _) = broadcast::channel(256);
        Self {
            services: OnceCell::new(),
            locks: StdMutex::new(HashMap::new()),
            cancellations: Mutex::new(HashMap::new()),
            stop: CancellationToken::new(),
            tasks: TaskTracker::new(),
            live,
            background_failures: StdMutex::new(Vec::new()),
        }
    }
    pub(crate) fn services(&self) -> Result<&Services> {
        self.services.get().ok_or(SocialError::Unavailable)
    }
    pub(crate) fn conversation_locks(&self, id: ConversationId) -> ConversationLocks {
        self.locks
            .lock()
            .expect("conversation locks")
            .entry(id)
            .or_default()
            .clone()
    }
    pub(crate) fn context(&self, request_id: CommandId) -> Result<CommandContext> {
        Ok(CommandContext {
            operator: self.services()?.human,
            request_id,
        })
    }
    pub fn subscribe_live(&self) -> broadcast::Receiver<wire::LiveEvent> {
        self.live.subscribe()
    }

    pub async fn init(&self) -> Result<wire::Init> {
        let s = self.services()?;
        let overview = s.resources.store.overview(s.human).await?;
        let models = s
            .llm
            .list_models()
            .await
            .into_iter()
            .map(|m| wire::ModelOption {
                provider_id: m.provider,
                model_id: m.spec.id,
                display_name: m.spec.display_name,
                reasoning_levels: m
                    .spec
                    .reasoning
                    .map(|r| {
                        r.levels
                            .into_iter()
                            .map(|e| e.as_wire().to_owned())
                            .collect()
                    })
                    .unwrap_or_default(),
            })
            .collect();
        Ok(wire::Init {
            self_id: overview.human.id,
            self_name: overview.human.display_name,
            contacts: overview.contacts.into_iter().map(Into::into).collect(),
            conversations: overview.conversations.into_iter().map(Into::into).collect(),
            models,
            cursor: overview.cursor,
        })
    }
    async fn validate_binding(&self, binding: &Option<wire::Binding>) -> Result<()> {
        if let Some(binding) = binding {
            let selection = selection(binding)?;
            if self.services()?.llm.resolve(&selection).await?.is_none() {
                return Err(SocialError::ModelUnavailable("尚未配置模型目录".into()));
            }
        }
        Ok(())
    }
    pub async fn create_contact(&self, input: wire::ContactInput) -> Result<wire::Contact> {
        self.validate_binding(&input.binding).await?;
        let s = self.services()?;
        let actor = s
            .resources
            .store
            .create_ai(
                self.context(input.request_id)?,
                CreateAi {
                    display_name: input.name,
                    persona: input.persona,
                    binding: input.binding.map(Into::into),
                },
            )
            .await?;
        Ok(s.resources.store.contact(s.human, actor.id).await?.into())
    }
    pub async fn update_contact(
        &self,
        id: ActorId,
        input: wire::UpdateContactInput,
    ) -> Result<wire::Contact> {
        self.validate_binding(&input.binding).await?;
        Ok(self
            .services()?
            .resources
            .store
            .update_ai(
                self.context(input.request_id)?,
                UpdateAi {
                    display_name: input.name,
                    persona: input.persona,
                    binding: input.binding.map(Into::into),
                    expected_revision: input.expected_revision,
                },
                id,
            )
            .await?
            .into())
    }
    pub async fn open_direct(&self, id: ActorId, command: wire::Command) -> Result<ConversationId> {
        Ok(self
            .services()?
            .resources
            .store
            .open_direct(self.context(command.request_id)?, id)
            .await?
            .id)
    }
    pub async fn conversation(&self, id: ConversationId) -> Result<wire::Conversation> {
        let s = self.services()?;
        Ok(s.resources
            .store
            .conversation_record(s.human, id)
            .await?
            .into())
    }
    pub async fn messages(
        &self,
        id: ConversationId,
        before: Option<Sequence>,
    ) -> Result<wire::MessagePage> {
        let s = self.services()?;
        let (messages, has_more) = s.resources.store.message_page(s.human, id, before).await?;
        Ok(wire::MessagePage {
            messages: messages.into_iter().map(Into::into).collect(),
            has_more,
        })
    }
    pub async fn events(&self, after: EventCursor) -> Result<Vec<events::SocialEvent>> {
        let s = self.services()?;
        Ok(s.resources.store.events(s.human, after, 500).await?)
    }
    pub async fn run_details(&self, id: RunId) -> Result<Option<RunDetails>> {
        let s = self.services()?;
        Ok(s.resources.store.run_details(s.human, id).await?)
    }

    pub async fn send(
        self: &Arc<Self>,
        id: ConversationId,
        input: wire::SendInput,
    ) -> Result<wire::SendReceipt> {
        let s = self.services()?;
        let locks = self.conversation_locks(id);
        let _mutation = locks.mutation.lock().await;
        if self.stop.is_cancelled() {
            return Err(SocialError::Unavailable);
        }
        let record = s.resources.store.conversation_record(s.human, id).await?;
        let c = record.conversation;
        if c.kind != ConversationKind::Direct {
            return Err(SocialError::Invalid(
                "本批次先开放长期私聊，群聊将在下一批接入".into(),
            ));
        }
        let ai = record
            .members
            .iter()
            .find(|m| m.actor_id != s.human)
            .expect("direct has its AI member")
            .actor_id;
        let contact = s.resources.store.contact(s.human, ai).await?;
        let binding = contact.binding.map(|b| wire::Binding {
            provider_id: b.provider_id,
            model_id: b.model_id,
            reasoning_effort: b.reasoning_effort,
        });
        let availability = self.validate_binding(&binding).await;
        let message = s
            .resources
            .store
            .append_message(
                self.context(input.request_id)?,
                AppendMessage {
                    conversation_id: id,
                    body: MessageBody::text(input.text),
                    reply_to: input.reply_to,
                    expected_revision: None,
                },
            )
            .await?;
        let execution_notice = if c.paused {
            Some(RunFailure {
                code: "paused".into(),
                message: "消息已保存，AI 当前暂停接话".into(),
            })
        } else if binding.is_none() {
            Some(RunFailure {
                code: "model_unavailable".into(),
                message: "消息已保存，请在联系人资料中配置模型".into(),
            })
        } else if let Err(error) = availability {
            let error = error.wire();
            Some(RunFailure {
                code: error.code.into(),
                message: format!("消息已保存；{}", error.message),
            })
        } else {
            None
        };
        if execution_notice.is_none() {
            let feature = self.clone();
            let trigger = message.id;
            let execution_locks = locks.clone();
            self.tasks.spawn(async move {
                if let Err(error) = feature
                    .execute_message(id, ai, trigger, execution_locks)
                    .await
                {
                    let wire_error = error.wire();
                    let _ = feature.live.send(wire::LiveEvent::Notice {
                        conversation_id: id,
                        error: RunFailure {
                            code: wire_error.code.into(),
                            message: wire_error.message,
                        },
                    });
                    if matches!(error, SocialError::Storage(_)) {
                        feature
                            .background_failures
                            .lock()
                            .expect("background failures")
                            .push(error);
                    }
                }
            });
        }
        Ok(wire::SendReceipt {
            message: message.into(),
            execution_notice,
        })
    }
    pub async fn cancel_run(&self, id: RunId) -> Result<()> {
        let s = self.services()?;
        let run = s.resources.store.run(s.human, id).await?;
        if run.status == RunStatus::Running
            && let Some(cancel) = self.cancellations.lock().await.get(&id)
        {
            cancel.cancel();
        }
        Ok(())
    }
}

pub(crate) fn selection(binding: &wire::Binding) -> Result<ModelSelection> {
    let reasoning_effort = binding
        .reasoning_effort
        .as_ref()
        .map(|effort| {
            serde_json::from_value(serde_json::Value::String(effort.clone()))
                .map_err(|_| SocialError::Invalid(format!("无效思考强度：{effort}")))
        })
        .transpose()?;
    Ok(ModelSelection {
        provider: binding.provider_id.clone(),
        model: binding.model_id.clone(),
        reasoning_effort,
    })
}

impl Feature for SocialFeature {
    fn id(&self) -> &'static str {
        "social"
    }
    fn as_any(self: Arc<Self>) -> Arc<dyn Any + Send + Sync> {
        self
    }
    fn initialize(
        self: Arc<Self>,
        ctx: FeatureContext,
    ) -> BoxFuture<'static, runtime::error::Result<()>> {
        Box::pin(async move {
            let resources = ctx.social.ok_or_else(|| {
                runtime::error::RuntimeError::ConfigError(
                    "SocialFeature requires social resources".into(),
                )
            })?;
            let human = resources.store.local_human("我".into()).await?;
            self.services
                .set(Services {
                    resources,
                    llm: ctx.llm,
                    human: human.id,
                })
                .map_err(|_| {
                    runtime::error::RuntimeError::ConfigError(
                        "SocialFeature already initialized".into(),
                    )
                })?;
            Ok(())
        })
    }
    fn shutdown(self: Arc<Self>) -> BoxFuture<'static, runtime::error::Result<()>> {
        Box::pin(async move {
            self.stop.cancel();
            self.tasks.close();
            self.tasks.wait().await;
            if let Some(error) = self
                .background_failures
                .lock()
                .expect("background failures")
                .pop()
            {
                return Err(runtime::error::RuntimeError::ConfigError(error.to_string()));
            }
            Ok(())
        })
    }
}
