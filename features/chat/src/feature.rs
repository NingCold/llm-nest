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

pub struct ChatFeature {
    ctx: Arc<tokio::sync::RwLock<Option<FeatureContext>>>,
}

impl ChatFeature {
    pub fn new() -> Self {
        Self {
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
        let ctx = self.ctx.read().await.clone().unwrap();
        let message_id = MessageId::new();

        // verify session exists
        if !ctx.sessions.read().await.exists(&session_id) {
            let (tx, rx) = mpsc::channel::<ChatEvent>(64);
            let msg = format!("Session not found: {}", session_id);
            tokio::spawn(async move {
                let _ = tx
                    .send(ChatEvent::Error {
                        message_id,
                        error: msg,
                    })
                    .await;
            });
            return Ok(ReceiverStream::new(rx));
        }

        // Persist the user message before the request is built. A failed
        // write leaves the session untouched and is reported like a session
        // error, so nothing is silently lost. Attachments become multimodal
        // content parts (images/files) alongside the text.
        let mut user_msg = if attachments.is_empty() {
            Message::user(&input)
        } else {
            let mut parts = Vec::with_capacity(attachments.len() + 1);
            parts.push(ContentPart::Text(input.clone()));
            parts.extend(attachments);
            Message::new(Role::User, parts)
        };
        user_msg.created_at = Some(now_secs());
        if let Err(e) = ctx
            .sessions
            .write()
            .await
            .push_message(&session_id, user_msg)
        {
            let (tx, rx) = mpsc::channel::<ChatEvent>(64);
            let msg = format!("Failed to persist message: {e}");
            tokio::spawn(async move {
                let _ = tx
                    .send(ChatEvent::Error {
                        message_id,
                        error: msg,
                    })
                    .await;
            });
            return Ok(ReceiverStream::new(rx));
        }

        let (tx, rx) = mpsc::channel::<ChatEvent>(64);

        tokio::spawn(async move {
            if cancel.is_cancelled() {
                let _ = tx.send(ChatEvent::Cancelled { message_id }).await;
                return;
            }

            let tools = ctx.tools.definitions();

            // Agent loop: consume stream(s); if the model requested tools,
            // execute them, persist results, and request another turn until
            // the model answers without tool calls (or the iteration cap).
            let t_start = Instant::now();
            let mut t_first_token: Option<Instant> = None;
            let mut t_first_content: Option<Instant> = None;
            let mut final_usage: Option<Usage> = None;

            for _iteration in 0..MAX_TOOL_ITERATIONS {
                if cancel.is_cancelled() {
                    let _ = tx.send(ChatEvent::Cancelled { message_id }).await;
                    return;
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
                            .map(Message::to_wire)
                            .collect(),
                        options: options.clone(),
                        tools: tools.clone(),
                        resolved: None,
                    }
                };

                let mut stream = match ctx.llm.complete_stream(req).await {
                    Ok(s) => s,
                    Err(e) => {
                        let _ = tx
                            .send(ChatEvent::Error {
                                message_id,
                                error: e.to_string(),
                            })
                            .await;
                        return;
                    }
                };

                // Consume this turn's stream.
                let mut iter_content = String::new();
                let mut iter_reasoning = String::new();
                let mut iter_tool_calls: Vec<ToolCall> = Vec::new();
                let mut iter_usage: Option<Usage> = None;

                loop {
                    tokio::select! {
                        biased;
                        _ = cancel.cancelled() => {
                            let _ = tx.send(ChatEvent::Cancelled { message_id }).await;
                            return;
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
                                        return;
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
                                        return;
                                    }
                                }
                                Some(Ok(ChatChunk::ToolCall { id, name, arguments })) => {
                                    iter_tool_calls.push(ToolCall {
                                        id,
                                        name,
                                        arguments,
                                    });
                                }
                                Some(Ok(ChatChunk::Done { usage })) => {
                                    iter_usage = usage;
                                    break;
                                }
                                Some(Err(e)) => {
                                    let _ = tx
                                        .send(ChatEvent::Error {
                                            message_id,
                                            error: e.to_string(),
                                        })
                                        .await;
                                    return;
                                }
                                None => break,
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
                        .push_message(&session_id, assistant)
                    {
                        let _ = tx
                            .send(ChatEvent::Error {
                                message_id,
                                error: format!("Failed to persist assistant message: {e}"),
                            })
                            .await;
                        return;
                    }
                    let _ = tx
                        .send(ChatEvent::Finished {
                            message_id,
                            usage: final_usage,
                            timings: Some(timings),
                        })
                        .await;
                    return;
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
                    parts.extend(
                        iter_tool_calls
                            .iter()
                            .cloned()
                            .map(ContentPart::ToolCall),
                    );
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
                        let _ = tx
                            .send(ChatEvent::Error {
                                message_id,
                                error: format!("Failed to persist assistant message: {e}"),
                            })
                            .await;
                        return;
                    }
                }

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
                    let mut tool_msg =
                        Message::new(Role::Tool, vec![ContentPart::ToolResult(result.clone())]);
                    tool_msg.created_at = Some(now_secs());
                    if let Err(e) = ctx
                        .sessions
                        .write()
                        .await
                        .push_message(&session_id, tool_msg)
                    {
                        let _ = tx
                            .send(ChatEvent::Error {
                                message_id,
                                error: format!("Failed to persist tool result: {e}"),
                            })
                            .await;
                        return;
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

            // Iteration cap reached: report and stop.
            let _ = tx
                .send(ChatEvent::Error {
                    message_id,
                    error: format!(
                        "exceeded {MAX_TOOL_ITERATIONS} tool-calling iterations; stopping"
                    ),
                })
                .await;
        });

        Ok(ReceiverStream::new(rx))
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
