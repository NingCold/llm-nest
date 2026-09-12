use std::{
    collections::VecDeque,
    pin::Pin,
    task::{Context, Poll},
};

use crate::chunk::ChatChunk;
use crate::protocols::sse::SseDataStream;
use futures_util::Stream;

use super::chat::{DeltaToolCall, StreamResponse};
use crate::error::{AiError, Result};
use common::Usage;

/// A tool call being assembled from streamed fragments.
struct PendingToolCall {
    index: u32,
    id: String,
    name: String,
    arguments: String,
}

pub struct OpenAIStream {
    inner: SseDataStream,
    usage: Option<Usage>,
    saw_finish: bool,
    finished: bool,
    /// Fragments of in-flight tool calls, keyed by delta `index`.
    pending_tool_calls: Vec<PendingToolCall>,
    /// Assembled chunks (tool calls, then the terminal `Done`) flushed in
    /// order after the finish event.
    queued: VecDeque<ChatChunk>,
}

impl OpenAIStream {
    pub fn new(response: reqwest::Response) -> Self {
        Self {
            inner: SseDataStream::new(response),
            usage: None,
            saw_finish: false,
            finished: false,
            pending_tool_calls: Vec::new(),
            queued: VecDeque::new(),
        }
    }

    /// Merge one fragment into the pending call with the same index.
    fn merge_tool_call_fragment(&mut self, fragment: &DeltaToolCall) {
        match self
            .pending_tool_calls
            .iter_mut()
            .find(|c| c.index == fragment.index)
        {
            Some(existing) => {
                if let Some(id) = &fragment.id {
                    if existing.id.is_empty() {
                        existing.id = id.clone();
                    }
                }
                if let Some(name) = fragment.function.as_ref().and_then(|f| f.name.as_ref()) {
                    existing.name = name.clone();
                }
                if let Some(args) = fragment
                    .function
                    .as_ref()
                    .and_then(|f| f.arguments.as_ref())
                {
                    existing.arguments.push_str(args);
                }
            }
            None => {
                self.pending_tool_calls.push(PendingToolCall {
                    index: fragment.index,
                    id: fragment.id.clone().unwrap_or_default(),
                    name: fragment
                        .function
                        .as_ref()
                        .and_then(|f| f.name.clone())
                        .unwrap_or_default(),
                    arguments: fragment
                        .function
                        .as_ref()
                        .and_then(|f| f.arguments.clone())
                        .unwrap_or_default(),
                });
            }
        }
    }

    fn finish(&mut self) -> Result<()> {
        self.finished = true;
        self.pending_tool_calls.sort_by_key(|c| c.index);
        for call in self.pending_tool_calls.drain(..) {
            if call.id.is_empty() || call.name.is_empty() {
                return Err(AiError::StreamError("incomplete tool call".into()));
            }
            self.queued.push_back(ChatChunk::ToolCall {
                id: call.id,
                name: call.name,
                arguments: call.arguments,
                thought_signature: None,
            });
        }
        self.queued.push_back(ChatChunk::Done {
            usage: self.usage.take(),
        });
        Ok(())
    }

    fn consume(&mut self, payload: &str) -> Result<()> {
        if payload.trim() == "[DONE]" {
            return self.finish();
        }
        if payload.trim().is_empty() {
            return Ok(());
        }
        let response: StreamResponse = serde_json::from_str(payload)?;
        if let Some(usage) = response.usage {
            self.usage = Some(usage.into());
        }
        if let Some(choice) = response.choices.into_iter().next() {
            if let Some(reason) = choice.finish_reason {
                match reason.as_str() {
                    "stop" | "tool_calls" | "function_call" => self.saw_finish = true,
                    _ => {
                        return Err(AiError::StreamError(format!(
                            "generation incomplete: {reason}"
                        )));
                    }
                }
            }
            if let Some(fragments) = choice.delta.tool_calls {
                for fragment in fragments {
                    self.merge_tool_call_fragment(&fragment);
                }
            }
            if let Some(content) = choice.delta.reasoning_content.filter(|s| !s.is_empty()) {
                self.queued.push_back(ChatChunk::ReasoningDelta { content });
            }
            if let Some(content) = choice.delta.content.filter(|s| !s.is_empty()) {
                self.queued.push_back(ChatChunk::Delta { content });
            }
        }
        Ok(())
    }
}

impl Stream for OpenAIStream {
    type Item = Result<ChatChunk>;
    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        loop {
            if let Some(chunk) = self.queued.pop_front() {
                return Poll::Ready(Some(Ok(chunk)));
            }
            if self.finished {
                return Poll::Ready(None);
            }
            let outcome = match Pin::new(&mut self.inner).poll_next(cx) {
                Poll::Ready(Some(Ok(payload))) => self.consume(&payload),
                Poll::Ready(Some(Err(err))) => Err(err),
                Poll::Ready(None) if self.saw_finish => self.finish(),
                Poll::Ready(None) => Err(AiError::StreamError(
                    "stream ended without a completion event".into(),
                )),
                Poll::Pending => return Poll::Pending,
            };
            if let Err(err) = outcome {
                self.finished = true;
                self.queued.clear();
                return Poll::Ready(Some(Err(err)));
            }
        }
    }
}
