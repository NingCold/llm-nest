use crate::{Result, SocialError, wire};
use ai_client::{AiClient, ModelSelection};
use runtime::feature::{BoxFuture, Feature, FeatureContext, SocialResources};
use social::*;
use std::{
    any::Any,
    collections::HashMap,
    sync::{
        Arc, Mutex as StdMutex,
        atomic::{AtomicBool, Ordering},
    },
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
    pub wake: Arc<tokio::sync::Notify>,
    pub scheduled: Arc<AtomicBool>,
    pub dirty: Arc<AtomicBool>,
    pub summary: Arc<Mutex<()>>,
    pub mutation: Arc<Mutex<()>>,
    pub execution: Arc<Mutex<()>>,
}

pub struct SocialFeature {
    pub(crate) execution_slots: tokio::sync::Semaphore,
    pub(crate) summary_slots: tokio::sync::Semaphore,
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
            execution_slots: tokio::sync::Semaphore::new(4),
            summary_slots: tokio::sync::Semaphore::new(1),
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
            .map(|m| {
                let mut levels = vec![None];
                levels.extend(
                    m.spec
                        .reasoning
                        .iter()
                        .flat_map(|r| r.levels.iter().copied().map(Some)),
                );
                let generation_limits = levels
                    .into_iter()
                    .map(|effort| {
                        let resolved = ai_client::ResolvedSelection {
                            selection: ModelSelection {
                                provider: m.provider.clone(),
                                model: m.spec.id.clone(),
                                reasoning_effort: effort,
                            },
                            spec: m.spec.clone(),
                        };
                        (
                            effort.map_or("default", |e| e.as_wire()).to_owned(),
                            crate::generation::limits(&resolved),
                        )
                    })
                    .collect();
                wire::ModelOption {
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
                    generation_limits,
                }
            })
            .collect();
        Ok(wire::Init {
            self_id: overview.account.id,
            self_name: overview.account.display_name.clone(),
            self_account: overview.account.clone().into(),
            accounts: overview.accounts.into_iter().map(Into::into).collect(),
            address_book: overview.address_book,
            conversations: overview.conversations.into_iter().map(Into::into).collect(),
            models,
            cursor: overview.cursor,
        })
    }
    async fn validate_binding(&self, binding: &Option<wire::Binding>) -> Result<()> {
        if let Some(binding) = binding {
            let selection = selection(binding)?;
            let client = self.services()?.llm.snapshot().await;
            let resolved = client
                .resolve(&selection)
                .await?
                .ok_or_else(|| SocialError::ModelUnavailable("尚未配置模型目录".into()))?;
            crate::generation::options(binding, &resolved)?;
        }
        Ok(())
    }
    pub async fn create_ai(&self, input: wire::AiInput) -> Result<wire::ManagedAi> {
        self.validate_binding(&input.binding).await?;
        let s = self.services()?;
        let actor = s
            .resources
            .store
            .create_ai(
                self.context(input.request_id)?,
                CreateAi {
                    display_name: input.name,
                    bio: input.bio,
                    avatar_ref: None,
                    persona: input.persona,
                    binding: input.binding.map(Into::into),
                },
            )
            .await?;
        Ok(s.resources
            .store
            .managed_account(s.human, actor.id)
            .await?
            .into())
    }
    pub async fn update_ai(
        &self,
        id: ActorId,
        input: wire::UpdateAiInput,
    ) -> Result<wire::ManagedAi> {
        let current = self
            .services()?
            .resources
            .store
            .managed_account(self.services()?.human, id)
            .await?;
        self.validate_binding(&input.binding).await?;
        self.services()?
            .resources
            .store
            .update_ai(
                self.context(input.request_id)?,
                UpdateAi {
                    display_name: input.name,
                    bio: input.bio,
                    avatar_ref: current.contact.actor.avatar_ref,
                    persona: input.persona,
                    binding: input.binding.map(Into::into),
                    expected_revision: input.expected_revision,
                },
                id,
            )
            .await?;
        Ok(self
            .services()?
            .resources
            .store
            .managed_account(self.services()?.human, id)
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
        let saved = s
            .resources
            .store
            .send_message(
                self.context(input.request_id)?,
                SendMessage {
                    conversation_id: id,
                    body: MessageBody::text(input.text),
                    reply_to: input.reply_to,
                    mentions: input.mentions,
                    reference: input.reference,
                },
            )
            .await?;
        let record = s.resources.store.conversation_record(s.human, id).await?;
        let mut execution_notice = if record.conversation.paused {
            Some(RunFailure {
                code: "paused".into(),
                message: "消息已保存，AI 当前暂停接话".into(),
            })
        } else {
            None
        };
        for target in &saved.response_targets {
            let contact = s.resources.store.contact(s.human, *target).await?;
            if contact.actor.status == EntityStatus::Archived {
                execution_notice = Some(RunFailure {
                    code: "account_disabled".into(),
                    message: format!(
                        "消息已保存；{} 的账号已停用，管理员恢复账号后只有新消息才会触发回复",
                        contact.actor.display_name
                    ),
                });
                break;
            }
        }
        for plan in &saved.plans {
            let contact = s.resources.store.contact(s.human, plan.actor_id).await?;
            let binding = contact.binding.map(Into::into);
            if binding.is_none() {
                execution_notice = Some(RunFailure {
                    code: "model_unavailable".into(),
                    message: format!("消息已保存；请为 {} 配置模型", contact.actor.display_name),
                });
                break;
            }
            if let Err(error) = self.validate_binding(&binding).await {
                let error = error.wire();
                execution_notice = Some(RunFailure {
                    code: error.code.into(),
                    message: format!("消息已保存；{}", error.message),
                });
                break;
            }
        }
        self.schedule_queue(id);
        Ok(wire::SendReceipt {
            message: saved.message.into(),
            execution_notice,
        })
    }
    pub(crate) fn schedule_queue(self: &Arc<Self>, id: ConversationId) {
        if self.stop.is_cancelled() {
            return;
        }
        let feature = self.clone();
        let locks = self.conversation_locks(id);
        locks.dirty.store(true, Ordering::SeqCst);
        locks.wake.notify_one();
        if locks.scheduled.swap(true, Ordering::SeqCst) {
            return;
        }
        self.tasks.spawn(async move {
            locks.dirty.store(false, Ordering::SeqCst);
            if let Err(error) = feature.drain_queue(id, locks.clone()).await {
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
            locks.scheduled.store(false, Ordering::SeqCst);
            if locks.dirty.swap(false, Ordering::SeqCst) {
                feature.schedule_queue(id);
            }
        });
    }
    pub async fn create_group(&self, input: wire::CreateGroupInput) -> Result<ConversationId> {
        Ok(self
            .services()?
            .resources
            .store
            .create_group(
                self.context(input.request_id)?,
                input.title,
                input.member_ids,
            )
            .await?
            .id)
    }
    pub async fn change_membership(
        self: &Arc<Self>,
        id: ConversationId,
        input: wire::MembershipInput,
    ) -> Result<wire::Conversation> {
        let s = self.services()?;
        let locks = self.conversation_locks(id);
        let _mutation = locks.mutation.lock().await;
        let active = s
            .resources
            .store
            .conversation_record(s.human, id)
            .await?
            .active_run;
        s.resources
            .store
            .change_membership(
                self.context(input.request_id)?,
                ChangeMembership {
                    conversation_id: id,
                    actor_id: input.actor_id,
                    expected_revision: input.expected_revision,
                    action: input.action,
                },
            )
            .await?;
        if let Some(run) = active
            && let Some(cancel) = self.cancellations.lock().await.get(&run.id)
        {
            cancel.cancel();
        }
        self.stop_terminal_executions().await?;
        self.schedule_queue(id);
        self.conversation(id).await
    }
    pub async fn plans(&self, id: ConversationId) -> Result<Vec<wire::Plan>> {
        let s = self.services()?;
        Ok(s.resources
            .store
            .plans(s.human, id)
            .await?
            .into_iter()
            .map(Into::into)
            .collect())
    }
    pub async fn cancel_run(&self, id: RunId) -> Result<()> {
        let s = self.services()?;
        s.resources.store.cancel_run(s.human, id).await?;
        self.stop_terminal_executions().await?;
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
            let resources = ctx.social;
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
            for id in self
                .services()
                .expect("initialized")
                .resources
                .store
                .queued_conversations(human.id)
                .await?
            {
                self.schedule_queue(id);
            }
            Ok(())
        })
    }
    fn shutdown(self: Arc<Self>) -> BoxFuture<'static, runtime::error::Result<()>> {
        Box::pin(async move {
            self.stop.cancel();
            self.tasks.close();
            self.tasks.wait().await;
            self.services()
                .expect("initialized")
                .resources
                .store
                .interrupt_summary_tasks()
                .await?;
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

#[cfg(test)]
mod rhythm_tests {
    use super::*;
    #[tokio::test]
    async fn manual_clock_wakes_one_scheduler_and_shutdown_cancels_wait_without_dispatch() {
        let directory = tempfile::tempdir().unwrap();
        let clock = Arc::new(ManualClock::new("2026-10-13T00:00:00Z".parse().unwrap()));
        let store = Arc::new(
            SqliteSocialStore::open(
                storage::DataDirectoryGuard::acquire(directory.path()).unwrap(),
                clock.clone(),
            )
            .await
            .unwrap(),
        );
        let runtime = runtime::runtime::Runtime::with_resources(
            &runtime::config::RuntimeConfig::default(),
            SocialResources {
                store: store.clone(),
                clock: clock.clone(),
            },
        )
        .unwrap();
        let feature = Arc::new(SocialFeature::new());
        runtime.register_feature(feature.clone()).await;
        runtime.initialize_features().await.unwrap();
        let ai = feature
            .create_ai(wire::AiInput {
                request_id: CommandId::new(),
                name: "test".into(),
                bio: String::new(),
                persona: String::new(),
                binding: None,
            })
            .await
            .unwrap()
            .account;
        let cid = feature
            .open_direct(
                ai.id,
                wire::Command {
                    request_id: CommandId::new(),
                },
            )
            .await
            .unwrap();
        let input = || wire::SendInput {
            request_id: CommandId::new(),
            text: "hello".into(),
            mentions: vec![],
            reply_to: None,
            reference: None,
        };
        feature.send(cid, input()).await.unwrap();
        for _ in 0..100 {
            feature.schedule_queue(cid);
        }
        let s = feature.services().unwrap();
        let plan = store.next_plan(s.human, cid).await.unwrap().unwrap();
        assert_eq!(store.plans(s.human, cid).await.unwrap().len(), 1);
        clock.set(plan.not_before.unwrap());
        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            while store.plans(s.human, cid).await.unwrap()[0].status == PlanStatus::Queued {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        assert_eq!(
            store.plans(s.human, cid).await.unwrap()[0].status,
            PlanStatus::Failed
        ); // no model configured
        feature.send(cid, input()).await.unwrap();
        tokio::time::timeout(
            std::time::Duration::from_secs(2),
            runtime.shutdown_features(),
        )
        .await
        .unwrap()
        .unwrap();
        assert!(feature.stop.is_cancelled());
        assert!(
            !feature
                .conversation_locks(cid)
                .scheduled
                .load(Ordering::SeqCst)
        );
    }
}
