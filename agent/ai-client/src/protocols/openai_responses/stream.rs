//! Stateful OpenAI Responses SSE stream.
//!
//! Tool calls stream as `response.output_item.added` (call_id/name),
//! `response.function_call_arguments.delta` (string fragments), and finally
//! `response.function_call_arguments.done` / `response.output_item.done`
//! (complete arguments). This wrapper holds per-`output_index` state between
//! payloads and flushes assembled calls as [`ChatChunk::ToolCall`] before the
//! terminal `Done`.

use std::collections::VecDeque;
use std::pin::Pin;
use std::task::{Context, Poll};

use futures_util::Stream;

use super::convert::{self, StreamEvent};
use crate::chunk::ChatChunk;
use crate::error::Result;
use crate::protocols::sse::SseDataStream;

/// A `function_call` item being assembled from streamed argument fragments,
/// keyed by the response `output_index`.
struct PendingCall {
    output_index: u32,
    call_id: String,
    name: String,
    /// Accumulated raw JSON arguments (may end partial if the stream cuts
    /// mid-call).
    arguments: String,
}

pub struct ResponsesStream {
    inner: Pin<Box<dyn Stream<Item = Result<String>> + Send>>,
    pending: Vec<PendingCall>,
    /// Flushed tool calls waiting to be yielded.
    queued: VecDeque<ChatChunk>,
    finished: bool,
}

impl ResponsesStream {
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

    fn take_pending(&mut self, output_index: u32) -> Option<PendingCall> {
        let pos = self
            .pending
            .iter()
            .position(|p| p.output_index == output_index)?;
        Some(self.pending.remove(pos))
    }
}

impl Stream for ResponsesStream {
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
                        Some(StreamEvent::FunctionCallStart {
                            output_index,
                            call_id,
                            name,
                            arguments,
                        }) => {
                            self.pending.retain(|p| p.output_index != output_index);
                            self.pending.push(PendingCall {
                                output_index,
                                call_id,
                                name,
                                arguments,
                            });
                            continue;
                        }
                        Some(StreamEvent::FunctionCallDelta {
                            output_index,
                            delta,
                        }) => {
                            if let Some(p) = self
                                .pending
                                .iter_mut()
                                .find(|p| p.output_index == output_index)
                            {
                                p.arguments.push_str(&delta);
                            }
                            continue;
                        }
                        Some(StreamEvent::FunctionCallDone {
                            output_index,
                            arguments,
                        }) => {
                            if let Some(p) = self
                                .pending
                                .iter_mut()
                                .find(|p| p.output_index == output_index)
                                && !arguments.is_empty()
                            {
                                // done carries the complete arguments string.
                                p.arguments = arguments;
                            }
                            continue;
                        }
                        Some(StreamEvent::FunctionCallFlush {
                            output_index,
                            call_id,
                            name,
                            arguments,
                        }) => {
                            let call = match self.take_pending(output_index) {
                                Some(mut p) => {
                                    if !arguments.is_empty() {
                                        p.arguments = arguments;
                                    }
                                    p
                                }
                                // no start seen (proxy omitted it): synthesize
                                None => PendingCall {
                                    output_index,
                                    call_id,
                                    name,
                                    arguments,
                                },
                            };
                            return Poll::Ready(Some(Ok(ChatChunk::ToolCall {
                                id: call.call_id,
                                name: call.name,
                                arguments: call.arguments,
                                thought_signature: None,
                            })));
                        }
                        Some(StreamEvent::Done { usage }) => {
                            self.finished = true;
                            // Flush any calls that ended without
                            // output_item.done, then the terminal chunk.
                            let calls: Vec<ChatChunk> = self
                                .pending
                                .drain(..)
                                .filter(|p| !p.name.is_empty())
                                .map(|p| ChatChunk::ToolCall {
                                    id: p.call_id,
                                    name: p.name,
                                    arguments: p.arguments,
                                    thought_signature: None,
                                })
                                .collect();
                            self.queued.extend(calls);
                            self.queued.push_back(ChatChunk::Done {
                                usage: usage.clone(),
                            });
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

    fn stream_from(payloads: Vec<&str>) -> ResponsesStream {
        let owned: Vec<String> = payloads.into_iter().map(String::from).collect();
        ResponsesStream::from_stream(futures_util::stream::iter(owned.into_iter().map(|p| Ok(p))))
    }

    fn start(index: u32, call_id: &str, name: &str) -> String {
        serde_json::json!({
            "type": "response.output_item.added",
            "output_index": index,
            "item": {
                "id": format!("fc_{call_id}"),
                "type": "function_call",
                "status": "in_progress",
                "call_id": call_id,
                "name": name,
                "arguments": "",
            },
        })
        .to_string()
    }

    fn delta(index: u32, fragment: &str) -> String {
        serde_json::json!({
            "type": "response.function_call_arguments.delta",
            "output_index": index,
            "delta": fragment,
        })
        .to_string()
    }

    fn done(index: u32, call_id: &str, name: &str, arguments: &str) -> String {
        serde_json::json!({
            "type": "response.output_item.done",
            "output_index": index,
            "item": {
                "id": format!("fc_{call_id}"),
                "type": "function_call",
                "status": "completed",
                "call_id": call_id,
                "name": name,
                "arguments": arguments,
            },
        })
        .to_string()
    }

    #[tokio::test]
    async fn assembles_function_call_from_fragments() {
        let stream = stream_from(vec![
            &start(1, "call_1", "add"),
            &delta(1, r#"{"a":"#),
            &delta(1, r#"6,"b":4}"#),
            &r#"{"type":"response.function_call_arguments.done","output_index":1,"arguments":"{\"a\": 6, \"b\": 4}"}"#,
            &done(1, "call_1", "add", r#"{"a": 6, "b": 4}"#),
            &r#"{"type":"response.completed","usage":{"input_tokens":10,"output_tokens":3,"total_tokens":13}}"#,
        ]);
        let chunks: Vec<ChatChunk> = stream.map(|r| r.unwrap()).collect().await;
        assert_eq!(chunks.len(), 2);
        match &chunks[0] {
            ChatChunk::ToolCall {
                id,
                name,
                arguments,
                ..
            } => {
                assert_eq!(id, "call_1");
                assert_eq!(name, "add");
                // done carries the canonical (spaced) arguments
                assert_eq!(arguments, r#"{"a": 6, "b": 4}"#);
            }
            other => panic!("expected ToolCall, got {other:?}"),
        }
        match &chunks[1] {
            ChatChunk::Done { usage: Some(u) } => {
                assert_eq!(u.prompt_tokens, 10);
                assert_eq!(u.completion_tokens, 3);
                assert_eq!(u.total_tokens, 13);
            }
            other => panic!("expected Done with usage, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn streams_text_around_tool_calls() {
        let stream = stream_from(vec![
            &r#"{"type":"response.output_text.delta","delta":"hi"}"#,
            &start(0, "call_2", "add"),
            &delta(0, "{}"),
            &done(0, "call_2", "add", "{}"),
            &r#"{"type":"response.output_text.delta","delta":"done"}"#,
            &r#"{"type":"response.completed"}"#,
        ]);
        let chunks: Vec<ChatChunk> = stream.map(|r| r.unwrap()).collect().await;
        assert!(matches!(&chunks[0], ChatChunk::Delta { content } if content == "hi"));
        assert!(matches!(
            &chunks[1],
            ChatChunk::ToolCall { id, name, .. } if id == "call_2" && name == "add"
        ));
        assert!(matches!(&chunks[2], ChatChunk::Delta { content } if content == "done"));
        assert!(matches!(&chunks[3], ChatChunk::Done { usage: None }));
    }

    #[tokio::test]
    async fn flushes_pending_calls_when_stream_ends_without_item_done() {
        let stream = stream_from(vec![
            &start(0, "call_3", "add"),
            &delta(0, r#"{"a":1}"#),
            &r#"{"type":"response.completed"}"#,
        ]);
        let chunks: Vec<ChatChunk> = stream.map(|r| r.unwrap()).collect().await;
        assert_eq!(chunks.len(), 2);
        assert!(matches!(&chunks[0], ChatChunk::ToolCall { name, .. } if name == "add"));
    }

    #[tokio::test]
    async fn synthesizes_call_when_start_was_omitted() {
        let stream = stream_from(vec![
            &done(3, "call_9", "add", r#"{"a":1}"#),
            &r#"{"type":"response.completed"}"#,
        ]);
        let chunks: Vec<ChatChunk> = stream.map(|r| r.unwrap()).collect().await;
        assert!(matches!(
            &chunks[0],
            ChatChunk::ToolCall { id, name, arguments, .. } if id == "call_9" && name == "add" && arguments == r#"{"a":1}"#
        ));
    }
}
