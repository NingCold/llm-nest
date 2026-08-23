use std::any::Any;
use std::sync::Arc;

use ai_client::{ChatChunk, ModelSelection};
use common::{GenerationOptions, MessageId};
use events::ChatEvent;
use futures_util::StreamExt;
use runtime::error::Result;
use runtime::feature::{BoxFuture, FeatureContext};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tokio_util::sync::CancellationToken;

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
        // error, so nothing is silently lost.
        if let Err(e) = ctx
            .sessions
            .write()
            .await
            .push_message(&session_id, common::Message::user(&input))
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

        let req = {
            let sessions = ctx.sessions.read().await;
            ai_client::ChatRequest {
                selection: model,
                messages: sessions.get_messages(&session_id),
                options,
                // The client fills the resolved routing result before dispatch.
                resolved: None,
            }
        };

        let (tx, rx) = mpsc::channel::<ChatEvent>(64);

        tokio::spawn(async move {
            if cancel.is_cancelled() {
                let _ = tx.send(ChatEvent::Cancelled { message_id }).await;
                return;
            }

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

            let mut content = String::new();

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
                                // Thinking chains stream to the frontend but
                                // never enter the stored assistant message
                                // (providers allow omitting them when there
                                // are no tool calls).
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
                                content.push_str(&delta);
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
                            Some(Ok(ChatChunk::Done)) => {
                                if let Err(e) = ctx.sessions.write().await.push_message(
                                    &session_id,
                                    common::Message::assistant(&content),
                                ) {
                                    let _ = tx
                                        .send(ChatEvent::Error {
                                            message_id,
                                            error: format!(
                                                "Failed to persist assistant message: {e}"
                                            ),
                                        })
                                        .await;
                                    return;
                                }
                                let _ =
                                    tx.send(ChatEvent::Finished { message_id })
                                        .await;
                                return;
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
                            None => {
                                let _ =
                                    tx.send(ChatEvent::Finished { message_id })
                                        .await;
                                return;
                            }
                        }
                    }
                }
            }
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
