use std::any::Any;
use std::sync::Arc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use ai_client::{ChatChunk, ModelSelection};
use common::{
    ContentPart, GenerationOptions, Message, MessageId, MessageTimings, Role, ToolCall, ToolResult,
    Usage,
};
use events::ChatEvent;
use futures_util::StreamExt;
use runtime::error::Result;
use runtime::feature::{BoxFuture, FeatureContext};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tokio_util::sync::CancellationToken;

/// Upper bound on tool-calling iterations per user turn (guards against
/// infinite agent loops).
const MAX_TOOL_ITERATIONS: usize = 6;

/// Unix timestamp (seconds) for message `created_at`.
fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

struct ActiveRun {
    sessions: Arc<std::sync::Mutex<std::collections::HashSet<common::SessionId>>>,
    id: common::SessionId,
    released: std::sync::atomic::AtomicBool,
}
impl ActiveRun {
    fn release(&self) {
        if !self
            .released
            .swap(true, std::sync::atomic::Ordering::SeqCst)
        {
            self.sessions.lock().unwrap().remove(&self.id);
        }
    }
}
impl Drop for ActiveRun {
    fn drop(&mut self) {
        self.release();
    }
}

pub struct ChatFeature {
    active: Arc<std::sync::Mutex<std::collections::HashSet<common::SessionId>>>,
    ctx: Arc<tokio::sync::RwLock<Option<FeatureContext>>>,
}

impl ChatFeature {
    pub fn new() -> Self {
        Self {
            active: Arc::default(),
            ctx: Arc::new(tokio::sync::RwLock::new(None)),
        }
    }

    pub async fn chat(
        &self,
        session_id: common::SessionId,
        input: String,
        attachments: Vec<common::ContentPart>,
        model: ModelSelection,
        options: GenerationOptions,
        cancel: CancellationToken,
    ) -> std::result::Result<impl futures_util::Stream<Item = ChatEvent>, ai_client::AiError> {
        self.chat_with_edit(session_id, input, attachments, model, options, cancel, None)
            .await
    }

    pub async fn chat_with_edit(
        &self,
        session_id: common::SessionId,
        input: String,
        attachments: Vec<common::ContentPart>,
        model: ModelSelection,
        options: GenerationOptions,
        cancel: CancellationToken,
        edit: Option<runtime::session_manager::ChatEdit>,
    ) -> std::result::Result<impl futures_util::Stream<Item = ChatEvent>, ai_client::AiError> {
        let ctx = self.ctx.read().await.clone().ok_or_else(|| {
            ai_client::AiError::StreamError("chat feature is not initialized".into())
        })?;
        {
            let mut active = self.active.lock().unwrap();
            if !active.insert(session_id) {
                return Err(ai_client::AiError::StreamError(
                    "a chat is already running for this session".into(),
                ));
            }
        }
        let run_guard = Arc::new(ActiveRun {
            sessions: self.active.clone(),
            id: session_id,
            released: std::sync::atomic::AtomicBool::new(false),
        });
        let message_id = MessageId::new();
        let llm = ctx.llm.snapshot().await;
        llm.resolve(&model).await?;
        if cancel.is_cancelled() {
            return Err(ai_client::AiError::StreamError("request cancelled".into()));
        }
        let mut parts = vec![ContentPart::Text(input)];
        parts.extend(attachments);
        let mut user_msg = Message::new(Role::User, parts);
        user_msg.created_at = Some(now_secs());
        ctx.sessions
            .write()
            .await
            .begin_turn(
                &session_id,
                user_msg,
                model.clone(),
                edit.as_ref(),
                Some(message_id),
            )
            .map_err(|e| ai_client::AiError::StreamError(e.to_string()))?;

        let (tx, rx) = mpsc::channel::<ChatEvent>(64);

        let worker_guard = run_guard.clone();
        tokio::spawn(async move {
            let _run_guard = worker_guard;
            let mut iter_content = String::new();
            let mut iter_reasoning = String::new();
            let mut outcome = {
                let work = async {
                    let tools = ctx.tools.definitions();
                    let mut checkpoint_timer =
                        tokio::time::interval(std::time::Duration::from_millis(500));
                    checkpoint_timer
                        .set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

                    // Agent loop: consume stream(s); if the model requested tools,
                    // execute them, persist results, and request another turn until
                    // the model answers without tool calls (or the iteration cap).
                    let t_start = Instant::now();
                    let mut t_first_token: Option<Instant> = None;
                    let mut t_first_content: Option<Instant> = None;
                    let mut final_usage: Option<Usage> = None;

                    for _iteration in 0..MAX_TOOL_ITERATIONS {
                        if cancel.is_cancelled() {
                            return ChatEvent::Cancelled { message_id };
                        }

                        let req = {
                            let sessions = ctx.sessions.read().await;
                            ai_client::ChatRequest {
                                selection: model.clone(),
                                // Wire form only: display metadata (reasoning, timings,
                                // usage, created_at) must never reach a provider request.
                                messages: sessions
                                    .get_messages(&session_id)
                                    .iter()
                                    .filter(|message| message.interruption.is_none())
                                    .map(Message::to_wire)
                                    .collect(),
                                options: options.clone(),
                                tools: tools.clone(),
                                resolved: None,
                            }
                        };

                        let mut stream = match llm.complete_stream(req).await {
                            Ok(s) => s,
                            Err(e) => {
                                return ChatEvent::Error {
                                    message_id,
                                    error: e.to_string(),
                                };
                            }
                        };

                        // Consume this turn's stream.
                        iter_content.clear();
                        iter_reasoning.clear();
                        let mut iter_tool_calls: Vec<ToolCall> = Vec::new();
                        let iter_usage: Option<Usage>;

                        loop {
                            tokio::select! {
                                biased;
                                _ = cancel.cancelled() => {
                                    return ChatEvent::Cancelled { message_id };
                                }
                                _ = checkpoint_timer.tick() => {
                                    let mut partial = Message::assistant(&iter_content);
                                    partial.reasoning = (!iter_reasoning.is_empty()).then(|| iter_reasoning.clone());
                                    if let Err(error) = ctx.sessions.write().await.checkpoint(&session_id, message_id, partial) {
                                        return ChatEvent::Error {message_id, error: format!("checkpoint failed: {error}")};
                                    }
                                }
                                chunk = stream.next() => {
                                    match chunk {
                                        Some(Ok(ChatChunk::ReasoningDelta { content: delta })) => {
                                            t_first_token.get_or_insert_with(Instant::now);
                                            iter_reasoning.push_str(&delta);
                                            if tx
                                                .send(ChatEvent::ReasoningDelta {
                                                    message_id,
                                                    content: delta,
                                                })
                                                .await
                                                .is_err()
                                            {
                                                return ChatEvent::Cancelled { message_id };
                                            }
                                        }
                                        Some(Ok(ChatChunk::Delta { content: delta })) => {
                                            t_first_token.get_or_insert_with(Instant::now);
                                            t_first_content.get_or_insert_with(Instant::now);
                                            iter_content.push_str(&delta);
                                            if tx
                                                .send(ChatEvent::Delta {
                                                    message_id,
                                                    content: delta,
                                                })
                                                .await
                                                .is_err()
                                            {
                                                return ChatEvent::Cancelled { message_id };
                                            }
                                        }
                                        Some(Ok(ChatChunk::ToolCall {
                                            id,
                                            name,
                                            arguments,
                                            thought_signature,
                                        })) => {
                                            iter_tool_calls.push(ToolCall {
                                                id,
                                                name,
                                                arguments,
                                                thought_signature,
                                            });
                                        }
                                        Some(Ok(ChatChunk::Done { usage })) => {
                                            iter_usage = usage;
                                            break;
                                        }
                                        Some(Err(e)) => {
                                            return ChatEvent::Error {
                                                    message_id,
                                                    error: e.to_string(),
                                                };
                                        }
                                        None => return ChatEvent::Error { message_id, error: "stream ended without a completion event".into() },
                                    }
                                }
                            }
                        }

                        // Accumulate usage across turns.
                        if let Some(u) = iter_usage {
                            final_usage = Some(match final_usage.take() {
                                Some(prev) => Usage {
                                    prompt_tokens: prev.prompt_tokens + u.prompt_tokens,
                                    completion_tokens: prev.completion_tokens + u.completion_tokens,
                                    total_tokens: prev.total_tokens + u.total_tokens,
                                    cached_tokens: prev.cached_tokens + u.cached_tokens,
                                },
                                None => u,
                            });
                        }

                        if iter_tool_calls.is_empty() {
                            // Final answer: persist the assistant message and finish.
                            // Note: use this iteration's content/reasoning only —
                            // `content`/`reasoning` accumulate across tool turns and
                            // must not leak into the final persisted message.
                            let t_end = Instant::now();
                            let timings = MessageTimings {
                                ttft_ms: t_first_token
                                    .map(|t| t.duration_since(t_start).as_millis() as u64),
                                reasoning_ms: t_first_content
                                    .map(|t| t.duration_since(t_start).as_millis() as u64),
                                total_ms: Some(t_end.duration_since(t_start).as_millis() as u64),
                            };
                            let mut assistant = Message::assistant(&iter_content);
                            assistant.id = Some(message_id);
                            if !iter_reasoning.is_empty() {
                                assistant.reasoning = Some(iter_reasoning.clone());
                            }
                            assistant.created_at = Some(now_secs());
                            assistant.thinking_ms = timings.reasoning_ms;
                            assistant.usage = final_usage.clone();
                            assistant.timings = Some(timings);
                            if let Err(e) = ctx
                                .sessions
                                .write()
                                .await
                                .finish_run(&session_id, assistant)
                            {
                                return ChatEvent::Error {
                                    message_id,
                                    error: format!("Failed to persist assistant message: {e}"),
                                };
                            }
                            return ChatEvent::Finished {
                                message_id,
                                usage: final_usage,
                                timings: Some(timings),
                            };
                        }

                        // The model wants tools: persist the assistant message with
                        // the tool-call blocks (also when the turn carried no text —
                        // e.g. Gemini functionCall-only turns — since the next
                        // request must include the calls alongside the tool results).
                        if !iter_content.is_empty() || !iter_tool_calls.is_empty() {
                            let mut parts = Vec::new();
                            if !iter_content.is_empty() {
                                parts.push(ContentPart::Text(iter_content.clone()));
                            }
                            parts
                                .extend(iter_tool_calls.iter().cloned().map(ContentPart::ToolCall));
                            let mut assistant = Message::new(Role::Assistant, parts);
                            if !iter_reasoning.is_empty() {
                                assistant.reasoning = Some(iter_reasoning.clone());
                            }
                            assistant.created_at = Some(now_secs());
                            if let Err(e) = ctx
                                .sessions
                                .write()
                                .await
                                .push_message(&session_id, assistant)
                            {
                                return ChatEvent::Error {
                                    message_id,
                                    error: format!("Failed to persist assistant message: {e}"),
                                };
                            }
                        }

                        iter_content.clear();
                        iter_reasoning.clear();
                        for tc in &iter_tool_calls {
                            let _ = tx
                                .send(ChatEvent::ToolCall {
                                    message_id,
                                    id: tc.id.clone(),
                                    name: tc.name.clone(),
                                    arguments: tc.arguments.clone(),
                                })
                                .await;

                            let args = serde_json::from_str(&tc.arguments).unwrap_or_default();
                            let t_tool = Instant::now();
                            let (out, is_error) = match ctx.tools.run(&tc.name, args).await {
                                Ok(v) => (v.to_string(), false),
                                Err(e) => (e.to_string(), true),
                            };
                            let duration_ms = t_tool.elapsed().as_millis() as u64;

                            let result = ToolResult {
                                id: tc.id.clone(),
                                name: tc.name.clone(),
                                content: out,
                                is_error,
                                duration_ms: Some(duration_ms),
                            };
                            let mut tool_msg = Message::new(
                                Role::Tool,
                                vec![ContentPart::ToolResult(result.clone())],
                            );
                            tool_msg.created_at = Some(now_secs());
                            if let Err(e) = ctx
                                .sessions
                                .write()
                                .await
                                .push_message(&session_id, tool_msg)
                            {
                                return ChatEvent::Error {
                                    message_id,
                                    error: format!("Failed to persist tool result: {e}"),
                                };
                            }
                            let _ = tx
                                .send(ChatEvent::ToolResult {
                                    message_id,
                                    id: result.id,
                                    name: result.name,
                                    content: result.content,
                                    is_error: result.is_error,
                                    duration_ms: result.duration_ms,
                                })
                                .await;
                        }
                    }

                    ChatEvent::Error {
                        message_id,
                        error: format!(
                            "exceeded {MAX_TOOL_ITERATIONS} tool-calling iterations; stopping"
                        ),
                    }
                };
                tokio::select! {
                    biased;
                    _ = cancel.cancelled() => ChatEvent::Cancelled { message_id },
                    _ = tx.closed() => ChatEvent::Cancelled { message_id },
                    event = work => event,
                }
            };
            if !matches!(outcome, ChatEvent::Finished { .. }) {
                let interruption = match &outcome {
                    ChatEvent::Error { error, .. } => common::Interruption::Failed(error.clone()),
                    _ => common::Interruption::Cancelled,
                };
                let mut partial = Message::assistant(&iter_content);
                partial.id = Some(message_id);
                partial.reasoning = (!iter_reasoning.is_empty()).then_some(iter_reasoning);
                partial.created_at = Some(now_secs());
                partial.interruption = Some(interruption);
                if let Err(error) = ctx
                    .sessions
                    .write()
                    .await
                    .finish_interrupted_turn(&session_id, partial)
                {
                    outcome = ChatEvent::Error {
                        message_id,
                        error: format!("Could not save interrupted turn: {error}"),
                    };
                }
            }
            let _ = tx.send(outcome).await;
        });

        Ok(ReceiverStream::new(rx).map(move |event| {
            if matches!(
                event,
                ChatEvent::Finished { .. } | ChatEvent::Error { .. } | ChatEvent::Cancelled { .. }
            ) {
                run_guard.release();
            }
            event
        }))
    }
}

impl runtime::feature::Feature for ChatFeature {
    fn id(&self) -> &'static str {
        "chat"
    }

    fn as_any(self: Arc<Self>) -> Arc<dyn Any + Send + Sync> {
        self
    }

    fn initialize(self: Arc<Self>, ctx: FeatureContext) -> BoxFuture<'static, Result<()>> {
        Box::pin(async move {
            *self.ctx.write().await = Some(ctx);
            Ok(())
        })
    }

    fn shutdown(self: Arc<Self>) -> BoxFuture<'static, Result<()>> {
        Box::pin(async move {
            *self.ctx.write().await = None;
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ai_client::{AiProvider, ChatRequest, ChatStream, ProviderId, ProviderResponse};
    use runtime::event_bus::EventBus;
    use runtime::runtime::Runtime;
    use runtime::session_manager::SessionManager;

    /// Minimal provider: answers every turn with a plain "42" and no tools.
    struct FakeProvider;

    #[async_trait::async_trait]
    impl AiProvider for FakeProvider {
        fn id(&self) -> String {
            "fake".into()
        }

        fn supported_protocols(&self) -> &[ai_client::Protocol] {
            &[]
        }

        async fn complete(&self, _req: ChatRequest) -> ai_client::Result<ProviderResponse> {
            Ok(ProviderResponse {
                message: Message::assistant("42"),
                reasoning: None,
                usage: None,
            })
        }

        async fn complete_stream(&self, _req: ChatRequest) -> ai_client::Result<ChatStream> {
            Ok(ChatStream::new(futures_util::stream::iter(vec![
                Ok(ChatChunk::Delta {
                    content: "42".into(),
                }),
                Ok(ChatChunk::Done { usage: None }),
            ])))
        }
    }

    /// Build an in-memory runtime with the fake provider pre-registered.
    /// `AiClient::register` takes a blocking lock, so it must run outside the
    /// async context (hence `block_in_place` on a multi-thread runtime).
    fn test_runtime() -> Runtime {
        tokio::task::block_in_place(|| {
            let llm = Arc::new(ai_client::AiClient::new());
            llm.register(ProviderId::new("fake"), Arc::new(FakeProvider));
            Runtime::new(SessionManager::new(), EventBus::new(), llm)
        })
    }

    fn selection() -> ModelSelection {
        ModelSelection {
            provider: "fake".into(),
            model: "m".into(),
            reasoning_effort: None,
        }
    }

    /// Run one full user turn and wait for the stream to settle.
    async fn run_turn(chat: &Arc<ChatFeature>, sid: common::SessionId, input: &str) {
        let cancel = CancellationToken::new();
        let mut stream = chat
            .chat(
                sid,
                input.to_string(),
                vec![],
                selection(),
                GenerationOptions::default(),
                cancel,
            )
            .await
            .unwrap();
        while let Some(ev) = stream.next().await {
            if matches!(
                ev,
                ChatEvent::Finished { .. } | ChatEvent::Error { .. } | ChatEvent::Cancelled { .. }
            ) {
                break;
            }
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn first_question_becomes_session_title() {
        let rt = test_runtime();
        let chat = Arc::new(ChatFeature::new());
        rt.register_feature(chat.clone()).await;
        rt.initialize_features().await.unwrap();

        let sid = rt.create_session(None).await.unwrap();
        run_turn(&chat, sid, "帮我计算 6+4 等于多少？").await;

        let session = rt.get_session(&sid).await.unwrap();
        assert_eq!(session.title(), Some("帮我计算 6+4 等于多少？"));
        // The question itself is still persisted as the first message.
        assert_eq!(session.messages().len(), 2);
        assert_eq!(session.messages()[0].text(), "帮我计算 6+4 等于多少？");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn long_first_question_is_truncated() {
        let rt = test_runtime();
        let chat = Arc::new(ChatFeature::new());
        rt.register_feature(chat.clone()).await;
        rt.initialize_features().await.unwrap();

        let long = "请帮我详细解释一下量子力学的基本原理和它在现代科技中的应用场景";
        let sid = rt.create_session(None).await.unwrap();
        run_turn(&chat, sid, long).await;

        let session = rt.get_session(&sid).await.unwrap();
        let title = session.title().expect("auto title");
        assert!(title.ends_with('…'));
        assert!(title.chars().count() <= runtime::session::AUTO_TITLE_MAX_CHARS + 1);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn existing_title_is_never_overwritten() {
        let rt = test_runtime();
        let chat = Arc::new(ChatFeature::new());
        rt.register_feature(chat.clone()).await;
        rt.initialize_features().await.unwrap();

        let sid = rt.create_session(Some("我的标题".into())).await.unwrap();
        run_turn(&chat, sid, "第一个问题").await;

        let session = rt.get_session(&sid).await.unwrap();
        assert_eq!(session.title(), Some("我的标题"));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn later_questions_do_not_retitle() {
        let rt = test_runtime();
        let chat = Arc::new(ChatFeature::new());
        rt.register_feature(chat.clone()).await;
        rt.initialize_features().await.unwrap();

        let sid = rt.create_session(None).await.unwrap();
        run_turn(&chat, sid, "第一个问题").await;
        run_turn(&chat, sid, "第二个问题，内容更长，也不应该覆盖标题").await;

        let session = rt.get_session(&sid).await.unwrap();
        assert_eq!(session.title(), Some("第一个问题"));
    }
    struct EdgeProvider {
        pending: bool,
    }
    #[async_trait::async_trait]
    impl AiProvider for EdgeProvider {
        fn id(&self) -> String {
            "fake".into()
        }
        fn supported_protocols(&self) -> &[ai_client::Protocol] {
            &[]
        }
        async fn complete(&self, req: ChatRequest) -> ai_client::Result<ProviderResponse> {
            FakeProvider.complete(req).await
        }
        async fn complete_stream(&self, _: ChatRequest) -> ai_client::Result<ChatStream> {
            if self.pending {
                std::future::pending().await
            } else {
                Ok(ChatStream::new(futures_util::stream::empty()))
            }
        }
    }
    fn edge_runtime(pending: bool) -> Runtime {
        tokio::task::block_in_place(|| {
            let llm = Arc::new(ai_client::AiClient::new());
            llm.register(ProviderId::new("fake"), Arc::new(EdgeProvider { pending }));
            Runtime::new(SessionManager::new(), EventBus::new(), llm)
        })
    }
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn unexpected_eof_persists_error_not_success() {
        let rt = edge_runtime(false);
        let chat = Arc::new(ChatFeature::new());
        rt.register_feature(chat.clone()).await;
        rt.initialize_features().await.unwrap();
        let id = rt.create_session(None).await.unwrap();
        let events: Vec<_> = chat
            .chat(
                id,
                "q".into(),
                vec![],
                selection(),
                GenerationOptions::default(),
                CancellationToken::new(),
            )
            .await
            .unwrap()
            .collect()
            .await;
        assert!(events.iter().any(|e| matches!(e, ChatEvent::Error { .. })));
        assert!(
            !events
                .iter()
                .any(|e| matches!(e, ChatEvent::Finished { .. }))
        );
        let session = rt.get_session(&id).await.unwrap();
        assert_eq!(session.messages().len(), 2);
        assert!(matches!(
            session.messages()[1].interruption,
            Some(common::Interruption::Failed(_))
        ));
    }
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn cancel_interrupts_request_setup_and_duplicate_does_not_append() {
        let rt = edge_runtime(true);
        let chat = Arc::new(ChatFeature::new());
        rt.register_feature(chat.clone()).await;
        rt.initialize_features().await.unwrap();
        let id = rt.create_session(None).await.unwrap();
        let cancel = CancellationToken::new();
        let mut stream = chat
            .chat(
                id,
                "q".into(),
                vec![],
                selection(),
                GenerationOptions::default(),
                cancel.clone(),
            )
            .await
            .unwrap();
        assert!(
            chat.chat(
                id,
                "duplicate".into(),
                vec![],
                selection(),
                GenerationOptions::default(),
                CancellationToken::new()
            )
            .await
            .is_err()
        );
        assert_eq!(rt.get_session(&id).await.unwrap().messages().len(), 1);
        cancel.cancel();
        let event = tokio::time::timeout(std::time::Duration::from_secs(1), stream.next())
            .await
            .unwrap()
            .unwrap();
        assert!(matches!(event, ChatEvent::Cancelled { .. }));
        let next = chat
            .chat(
                id,
                "next".into(),
                vec![],
                selection(),
                GenerationOptions::default(),
                CancellationToken::new(),
            )
            .await;
        assert!(next.is_ok());
    }
    struct PartialProvider {
        pending: bool,
    }
    #[async_trait::async_trait]
    impl AiProvider for PartialProvider {
        fn id(&self) -> String {
            "fake".into()
        }
        fn supported_protocols(&self) -> &[ai_client::Protocol] {
            &[]
        }
        async fn complete(&self, req: ChatRequest) -> ai_client::Result<ProviderResponse> {
            FakeProvider.complete(req).await
        }
        async fn complete_stream(&self, req: ChatRequest) -> ai_client::Result<ChatStream> {
            assert!(
                req.messages.iter().all(|m| m.text() != "partial"),
                "interrupted prose leaked into prompt"
            );
            let chunks = futures_util::stream::iter(vec![
                Ok(ChatChunk::ReasoningDelta {
                    content: "thinking".into(),
                }),
                Ok(ChatChunk::Delta {
                    content: "partial".into(),
                }),
            ]);
            if self.pending {
                Ok(ChatStream::new(
                    chunks.chain(futures_util::stream::pending()),
                ))
            } else {
                Ok(ChatStream::new(chunks))
            }
        }
    }
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn interrupted_content_saved_before_terminal_and_excluded_from_next_prompt() {
        for pending in [false, true] {
            let rt = tokio::task::block_in_place(|| {
                let llm = Arc::new(ai_client::AiClient::new());
                llm.register(
                    ProviderId::new("fake"),
                    Arc::new(PartialProvider { pending }),
                );
                Runtime::new(SessionManager::new(), EventBus::new(), llm)
            });
            let chat = Arc::new(ChatFeature::new());
            rt.register_feature(chat.clone()).await;
            rt.initialize_features().await.unwrap();
            let id = rt.create_session(None).await.unwrap();
            for _ in 0..2 {
                let cancel = CancellationToken::new();
                let mut stream = chat
                    .chat(
                        id,
                        "q".into(),
                        vec![],
                        selection(),
                        GenerationOptions::default(),
                        cancel.clone(),
                    )
                    .await
                    .unwrap();
                while let Some(event) =
                    tokio::time::timeout(std::time::Duration::from_secs(2), stream.next())
                        .await
                        .unwrap()
                {
                    if matches!(event, ChatEvent::Delta { .. }) && pending {
                        cancel.cancel();
                    }
                    if matches!(event, ChatEvent::Error { .. } | ChatEvent::Cancelled { .. }) {
                        let session = rt.get_session(&id).await.unwrap();
                        let message = session.messages().last().unwrap();
                        assert_eq!(message.text(), "partial");
                        assert_eq!(message.reasoning.as_deref(), Some("thinking"));
                        assert!(message.interruption.is_some());
                        break;
                    }
                    assert!(!matches!(event, ChatEvent::Finished { .. }));
                }
            }
        }
    }
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn dropping_receiver_saves_partial_and_releases_session() {
        let rt = tokio::task::block_in_place(|| {
            let llm = Arc::new(ai_client::AiClient::new());
            llm.register(
                ProviderId::new("fake"),
                Arc::new(PartialProvider { pending: true }),
            );
            Runtime::new(SessionManager::new(), EventBus::new(), llm)
        });
        let chat = Arc::new(ChatFeature::new());
        rt.register_feature(chat.clone()).await;
        rt.initialize_features().await.unwrap();
        let id = rt.create_session(None).await.unwrap();
        let mut stream = chat
            .chat(
                id,
                "q".into(),
                vec![],
                selection(),
                GenerationOptions::default(),
                CancellationToken::new(),
            )
            .await
            .unwrap();
        while let Some(event) = stream.next().await {
            if matches!(event, ChatEvent::Delta { .. }) {
                break;
            }
        }
        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            loop {
                if rt
                    .get_session(&id)
                    .await
                    .unwrap()
                    .run
                    .as_ref()
                    .and_then(|run| run.partial.as_ref())
                    .is_some_and(|message| message.text() == "partial")
                {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("periodic checkpoint persisted before termination");
        drop(stream);
        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            loop {
                if !chat.active.lock().unwrap().contains(&id) {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        let session = rt.get_session(&id).await.unwrap();
        assert_eq!(session.messages().last().unwrap().text(), "partial");
        assert_eq!(
            session.messages().last().unwrap().interruption,
            Some(common::Interruption::Cancelled)
        );
    }
}
