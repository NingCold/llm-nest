//! Stateful Anthropic SSE stream.
//!
//! Anthropic streams a `tool_use` block across three event kinds —
//! `content_block_start` (id/name), `input_json_delta` fragments, and
//! `content_block_stop` — so the stream must hold per-index state between
//! payloads. This wrapper consumes the shared [`SseDataStream`] frame stream,
//! assembles pending tool calls, and flushes them as
//! [`ChatChunk::ToolCall`] before the terminal `Done`.

use std::collections::VecDeque;
use std::pin::Pin;
use std::task::{Context, Poll};

use futures_util::Stream;

use super::convert::{self, StreamEvent};
use crate::chunk::ChatChunk;
use crate::error::Result;
use crate::protocols::sse::SseDataStream;

/// A `tool_use` block being assembled from `content_block_start` +
/// `input_json_delta` fragments, keyed by the content block index.
struct PendingToolUse {
    index: u32,
    id: String,
    name: String,
    /// Accumulated raw JSON arguments (may end partial if the stream cuts
    /// mid-call).
    input: String,
}

pub struct AnthropicStream {
    inner: Pin<Box<dyn Stream<Item = Result<String>> + Send>>,
    pending: Vec<PendingToolUse>,
    /// Flushed tool calls waiting to be yielded.
    queued: VecDeque<ChatChunk>,
    finished: bool,
}

impl AnthropicStream {
    /// Wrap a raw HTTP response (frames extracted by the shared SSE stream).
    pub fn new(response: reqwest::Response) -> Self {
        Self::from_stream(SseDataStream::new(response))
    }

    /// Wrap any `Result<String>` stream (SSE payloads) — used by tests.
    pub fn from_stream<S>(stream: S) -> Self
    where
        S: Stream<Item = Result<String>> + Send + 'static,
    {
        Self {
            inner: Box::pin(stream),
            pending: Vec::new(),
            queued: VecDeque::new(),
            finished: false,
        }
    }

    fn take_pending(&mut self, index: u32) -> Option<PendingToolUse> {
        let pos = self.pending.iter().position(|p| p.index == index)?;
        Some(self.pending.remove(pos))
    }
}

impl Stream for AnthropicStream {
    type Item = Result<ChatChunk>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        // Drain flushed chunks (terminal Done included) before checking
        // `finished`, so a flush triggered by the terminal event is not lost.
        if let Some(chunk) = self.queued.pop_front() {
            return Poll::Ready(Some(Ok(chunk)));
        }
        if self.finished {
            return Poll::Ready(None);
        }

        loop {
            match self.inner.as_mut().poll_next(cx) {
                Poll::Ready(Some(Ok(payload))) => {
                    let parsed = match convert::parse_event(&payload) {
                        Ok(p) => p,
                        Err(err) => return Poll::Ready(Some(Err(err))),
                    };
                    match parsed {
                        Some(StreamEvent::Delta { content }) => {
                            return Poll::Ready(Some(Ok(ChatChunk::Delta { content })));
                        }
                        Some(StreamEvent::ToolUseStart { index, id, name }) => {
                            self.pending.retain(|p| p.index != index);
                            self.pending.push(PendingToolUse {
                                index,
                                id,
                                name,
                                input: String::new(),
                            });
                            continue;
                        }
                        Some(StreamEvent::ToolUseDelta { index, partial_json }) => {
                            if let Some(p) = self.pending.iter_mut().find(|p| p.index == index) {
                                p.input.push_str(&partial_json);
                            }
                            continue;
                        }
                        Some(StreamEvent::ToolUseStop { index }) => {
                            if let Some(p) = self.take_pending(index) {
                                return Poll::Ready(Some(Ok(ChatChunk::ToolCall {
                                    id: p.id,
                                    name: p.name,
                                    arguments: p.input,
                                })));
                            }
                            continue;
                        }
                        Some(StreamEvent::Done { usage }) => {
                            self.finished = true;
                            // Flush any tool calls that ended without a
                            // content_block_stop, then the terminal chunk.
                            let calls: Vec<ChatChunk> = self
                                .pending
                                .drain(..)
                                .filter(|p| !p.name.is_empty())
                                .map(|p| ChatChunk::ToolCall {
                                    id: p.id,
                                    name: p.name,
                                    arguments: p.input,
                                })
                                .collect();
                            self.queued.extend(calls);
                            self.queued
                                .push_back(ChatChunk::Done { usage: usage.clone() });
                            if let Some(chunk) = self.queued.pop_front() {
                                return Poll::Ready(Some(Ok(chunk)));
                            }
                            return Poll::Ready(Some(Ok(ChatChunk::Done { usage })));
                        }
                        None => continue,
                    }
                }
                Poll::Ready(Some(Err(err))) => return Poll::Ready(Some(Err(err))),
                Poll::Ready(None) => {
                    self.finished = true;
                    return Poll::Ready(None);
                }
                Poll::Pending => return Poll::Pending,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::StreamExt;

    fn stream_from(payloads: Vec<&str>) -> AnthropicStream {
        let owned: Vec<String> = payloads.into_iter().map(String::from).collect();
        AnthropicStream::from_stream(futures_util::stream::iter(owned.into_iter().map(|p| Ok(p))))
    }

    #[tokio::test]
    async fn assembles_tool_use_from_fragments() {
        let stream = stream_from(vec![
            r#"{"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"toolu_1","name":"add","input":{}}}"#,
            r#"{"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":"{\"a\":"}}"#,
            r#"{"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":"6,\"b\":4}"}}"#,
            r#"{"type":"content_block_stop","index":0}"#,
            r#"{"type":"message_delta","usage":{"input_tokens":10,"output_tokens":3}}"#,
        ]);
        let chunks: Vec<ChatChunk> = stream.map(|r| r.unwrap()).collect().await;
        assert_eq!(chunks.len(), 2);
        match &chunks[0] {
            ChatChunk::ToolCall { id, name, arguments } => {
                assert_eq!(id, "toolu_1");
                assert_eq!(name, "add");
                assert_eq!(arguments, r#"{"a":6,"b":4}"#);
            }
            other => panic!("expected ToolCall, got {other:?}"),
        }
        match &chunks[1] {
            ChatChunk::Done { usage: Some(u) } => {
                assert_eq!(u.prompt_tokens, 10);
                assert_eq!(u.completion_tokens, 3);
            }
            other => panic!("expected Done with usage, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn streams_text_before_and_after_tool_calls() {
        let stream = stream_from(vec![
            r#"{"type":"content_block_start","index":0,"content_block":{"type":"text","text":"hi"}}"#,
            r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"hi"}}"#,
            r#"{"type":"content_block_stop","index":0}"#,
            r#"{"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"toolu_2","name":"add","input":{}}}"#,
            r#"{"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"{}"}}"#,
            r#"{"type":"content_block_stop","index":1}"#,
            r#"{"type":"message_stop"}"#,
        ]);
        let chunks: Vec<ChatChunk> = stream.map(|r| r.unwrap()).collect().await;
        assert!(matches!(&chunks[0], ChatChunk::Delta { content } if content == "hi"));
        assert!(matches!(
            &chunks[1],
            ChatChunk::ToolCall { id, name, .. } if id == "toolu_2" && name == "add"
        ));
        assert!(matches!(&chunks[2], ChatChunk::Done { usage: None }));
    }

    #[tokio::test]
    async fn flushes_pending_calls_when_stream_ends_without_stop() {
        let stream = stream_from(vec![
            r#"{"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"toolu_3","name":"add","input":{}}}"#,
            r#"{"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":"{\"a\":1}"}}"#,
            r#"{"type":"message_delta","usage":{"input_tokens":4,"output_tokens":1}}"#,
        ]);
        let chunks: Vec<ChatChunk> = stream.map(|r| r.unwrap()).collect().await;
        assert_eq!(chunks.len(), 2);
        assert!(matches!(&chunks[0], ChatChunk::ToolCall { name, .. } if name == "add"));
    }
}
