use crate::{
    Result, SocialError, SocialFeature,
    consciousness::ConsciousnessService,
    feature::{ConversationLocks, selection},
    wire,
};
use ai_client::{ChatChunk, ChatRequest};
use common::MessageTimings;
use futures_util::StreamExt;
use social::*;
use std::{sync::Arc, time::Instant};
use storage::social::{BeginRun, FinishRun, RunDetails, RunFailure, SocialStorageError};
use tokio_util::sync::CancellationToken;

impl SocialFeature {
    pub(crate) async fn drain_queue(
        self: &Arc<Self>,
        conversation_id: ConversationId,
        locks: ConversationLocks,
    ) -> Result<()> {
        let _execution = tokio::select! { biased; _=self.stop.cancelled()=>return Ok(()), guard=locks.execution.lock()=>guard };
        let s = self.services()?;
        loop {
            locks
                .dirty
                .store(false, std::sync::atomic::Ordering::SeqCst);
            if self.stop.is_cancelled() {
                return Ok(());
            }
            let tick = s
                .resources
                .store
                .participation_tick(s.human, conversation_id)
                .await?;
            let Some(plan) = tick.plan else {
                self.schedule_summaries(conversation_id, None);
                if let Some(deadline) = tick.wake_at {
                    tokio::select! {
                        _ = self.stop.cancelled() => return Ok(()),
                        _ = locks.wake.notified() => {},
                        _ = s.resources.clock.wait_until(deadline) => {},
                    }
                    continue;
                }
                return Ok(());
            };
            let _slot = tokio::select! {
                _ = self.stop.cancelled() => return Ok(()),
                slot = self.execution_slots.acquire() => slot.expect("execution slots remain open"),
            };
            match self.execute_plan(&plan, &locks).await {
                Ok(()) => {}
                Err(error) => {
                    if matches!(
                        error,
                        SocialError::Storage(
                            SocialStorageError::Sqlite { .. }
                                | SocialStorageError::Io { .. }
                                | SocialStorageError::Json(_)
                                | SocialStorageError::Counter(_)
                                | SocialStorageError::Schema { .. }
                                | SocialStorageError::Legacy { .. }
                                | SocialStorageError::WorkerClosed
                        )
                    ) {
                        return Err(error);
                    }
                    let failure = error.wire();
                    let reason = match &error {
                        SocialError::Storage(SocialStorageError::ParticipationLimit {
                            reason,
                            ..
                        }) => Some(*reason),
                        _ => None,
                    };
                    s.resources
                        .store
                        .fail_plan(s.human, plan.id, failure.message.clone(), reason)
                        .await?;
                    let _ = self.live.send(wire::LiveEvent::Notice {
                        conversation_id,
                        error: RunFailure {
                            code: failure.code.into(),
                            message: failure.message,
                        },
                    });
                }
            }
        }
    }
    async fn execute_plan(
        self: &Arc<Self>,
        plan: &ParticipationPlan,
        locks: &ConversationLocks,
    ) -> Result<()> {
        let conversation_id = plan.conversation_id;
        let s = self.services()?;
        let (run, client, request, details, cancel) =
            {
                let _mutation = locks.mutation.lock().await;
                if self.stop.is_cancelled() {
                    return Ok(());
                }
                // A newer input may have extended or replaced this plan while awaiting a slot.
                let tick = s
                    .resources
                    .store
                    .participation_tick(s.human, conversation_id)
                    .await?;
                if !tick.plan.is_some_and(|current| {
                    current.id == plan.id && current.revision == plan.revision
                }) {
                    return Ok(());
                }
                let material = s.resources.store.plan_material(s.human, plan.id).await?;
                let binding =
                    material.contact.binding.as_ref().ok_or_else(|| {
                        SocialError::ModelUnavailable("该 AI 尚未配置模型".into())
                    })?;
                let binding: wire::Binding = binding.clone().into();
                let selection = selection(&binding)?;
                let client = s.llm.snapshot().await;
                let resolved = client
                    .resolve(&selection)
                    .await?
                    .ok_or_else(|| SocialError::ModelUnavailable("尚未配置模型目录".into()))?;
                let details = RunDetails {
                    provider: resolved.selection.provider.clone(),
                    model: resolved.selection.model.clone(),
                    wire_model: resolved.spec.wire.clone(),
                    protocol: serde_json::to_value(resolved.spec.protocol)
                        .expect("protocol is serializable")
                        .as_str()
                        .expect("protocol is a string")
                        .to_owned(),
                    ..Default::default()
                };
                let options = crate::generation::options(&binding, &resolved)?;
                let prepared = ConsciousnessService::prepare(
                    material,
                    &resolved.spec,
                    options.max_tokens.expect("allocated output budget"),
                )?;
                // Contact controls span spaces. Keep run creation and token registration together
                // so a concurrent archive cannot miss the new execution. No model call holds this lock.
                let mut cancellations = self.cancellations.lock().await;
                let run = s
                    .resources
                    .store
                    .begin_plan(
                        s.human,
                        plan.id,
                        BeginRun {
                            snapshot: prepared.snapshot,
                            expected_revision: prepared.expected_revision,
                            details: Some(details.clone()),
                        },
                    )
                    .await?;
                let cancel = CancellationToken::new();
                cancellations.insert(run.id, cancel.clone());
                let request = ChatRequest {
                    selection,
                    messages: prepared.messages,
                    options,
                    tools: vec![],
                    resolved: None,
                };
                (run, client, request, details, cancel)
            };
        let deadline = plan.contact_intent_id.and(plan.expires_at);
        let outcome = self
            .consume(&run, &client, request, details, &cancel, deadline)
            .await;
        self.cancellations.lock().await.remove(&run.id);
        outcome
    }

    async fn consume(
        &self,
        run: &AiRun,
        client: &ai_client::AiClient,
        request: ChatRequest,
        mut details: RunDetails,
        cancel: &CancellationToken,
        deadline: Option<chrono::DateTime<chrono::Utc>>,
    ) -> Result<()> {
        let s = self.services()?;
        let started = Instant::now();
        let mut text = String::new();
        let mut ttft = None;
        let mut first_answer = None;
        let mut delta_seq = 0;
        let mut checkpoint = tokio::time::interval(std::time::Duration::from_millis(500));
        checkpoint.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let mut dirty = false;
        let expiry = async {
            match deadline {
                Some(t) => s.resources.clock.wait_until(t).await,
                None => std::future::pending::<()>().await,
            }
        };
        tokio::pin!(expiry);
        let opened = tokio::select! { biased;
            _=self.stop.cancelled()=>return self.finish(run,RunStatus::Interrupted,text,details).await,
            _=cancel.cancelled()=>return self.finish(run,RunStatus::Cancelled,text,details).await,
            _=&mut expiry=>{ details.error=Some(failure("contact_expired","约定已过期，停止生成")); return self.finish(run,RunStatus::Interrupted,text,details).await; },
            opened=client.complete_stream(request)=>opened,
        };
        let mut stream = match opened {
            Ok(stream) => stream,
            Err(_) => {
                details.error = Some(failure(
                    "provider_error",
                    "模型请求失败，请检查模型服务与配置",
                ));
                return self.finish(run, RunStatus::Failed, text, details).await;
            }
        };
        let status = loop {
            tokio::select! { biased;
                _=self.stop.cancelled()=>break RunStatus::Interrupted,
                _=cancel.cancelled()=>break RunStatus::Cancelled,
                _=&mut expiry=>{ details.error=Some(failure("contact_expired","约定已过期，停止生成")); break RunStatus::Interrupted; },
                _=checkpoint.tick(), if dirty => {
                    match s.resources.store.checkpoint_run(s.human,run.id,MessageBody::text(&text),Some(details.clone())).await {
                        Ok(())=>{},
                        Err(SocialStorageError::Conflict(_)) if s.resources.store.run(s.human,run.id).await?.status.is_terminal()=>return Ok(()),
                        Err(error)=>return Err(error.into()),
                    }
                    dirty=false;
                }
                chunk=stream.next()=>match chunk {
                    Some(Ok(ChatChunk::Delta { content }))=> {
                        let elapsed=started.elapsed().as_millis() as u64;
                        ttft.get_or_insert(elapsed); first_answer.get_or_insert(elapsed);
                        text.push_str(&content); dirty=true; delta_seq+=1;
                        let _=self.live.send(wire::LiveEvent::Delta { run_id:run.id,message_id:run.message_id,conversation_id:run.snapshot.scope.conversation_id,actor_id:run.snapshot.scope.actor_id,delta_seq,text:content });
                    }
                    Some(Ok(ChatChunk::ReasoningDelta { content }))=> {
                        ttft.get_or_insert(started.elapsed().as_millis() as u64);
                        details.reasoning.push_str(&content); dirty=true; delta_seq+=1;
                        let _=self.live.send(wire::LiveEvent::ReasoningDelta { run_id:run.id,message_id:run.message_id,conversation_id:run.snapshot.scope.conversation_id,actor_id:run.snapshot.scope.actor_id,delta_seq,text:content });
                    }
                    Some(Ok(ChatChunk::Done { usage }))=> {
                        details.usage=usage;
                        if text.trim().is_empty() { details.error=Some(failure("empty_answer","模型没有返回正文")); break RunStatus::Failed; }
                        break RunStatus::Succeeded;
                    }
                    Some(Ok(ChatChunk::ToolCall { .. }))=> { details.error=Some(failure("tool_not_allowed","该联系人尚未获准使用工具")); break RunStatus::Failed; }
                    Some(Err(ai_client::AiError::UnexpectedEof))=> { details.error=Some(failure("unexpected_eof","连接在模型确认完成之前结束")); break RunStatus::Interrupted; }
                    Some(Err(_))=> { details.error=Some(failure("provider_error","模型回复中断，请检查模型服务")); break RunStatus::Failed; }
                    None=> { details.error=Some(failure("unexpected_eof","连接在模型确认完成之前结束")); break RunStatus::Interrupted; }
                }
            }
        };
        details.timings = Some(MessageTimings {
            ttft_ms: ttft,
            reasoning_ms: first_answer,
            total_ms: Some(started.elapsed().as_millis() as u64),
        });
        self.finish(run, status, text, details).await
    }

    async fn finish(
        &self,
        run: &AiRun,
        status: RunStatus,
        text: String,
        mut details: RunDetails,
    ) -> Result<()> {
        let s = self.services()?;
        let input = FinishRun {
            run_id: run.id,
            expected_revision: run.revision,
            status,
            body: MessageBody::text(&text),
            details: Some(details.clone()),
        };
        let result = s
            .resources
            .store
            .finish_run(self.context(CommandId::new())?, input)
            .await;
        match result {
            Ok(_) => Ok(()),
            Err(SocialStorageError::Conflict(_)) => {
                let current = s.resources.store.run(s.human, run.id).await?;
                if current.status.is_terminal() {
                    return Ok(());
                }
                if status != RunStatus::Succeeded {
                    return Err(SocialStorageError::Conflict(
                        "run changed before termination".into(),
                    )
                    .into());
                }
                details.error = Some(failure(
                    "context_changed",
                    "本次使用的消息来源或参与状态已变化，回复保留为中断记录",
                ));
                s.resources
                    .store
                    .finish_run(
                        self.context(CommandId::new())?,
                        FinishRun {
                            run_id: run.id,
                            expected_revision: current.revision,
                            status: RunStatus::Interrupted,
                            body: MessageBody::text(text),
                            details: Some(details),
                        },
                    )
                    .await?;
                Ok(())
            }
            Err(error) => Err(error.into()),
        }
    }
}
fn failure(code: &str, message: &str) -> RunFailure {
    RunFailure {
        code: code.into(),
        message: message.into(),
    }
}
