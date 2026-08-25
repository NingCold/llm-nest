use std::{
    collections::VecDeque,
    pin::Pin,
    task::{Context, Poll},
};

use crate::chunk::ChatChunk;
use bytes::Bytes;
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
    inner: Pin<Box<dyn Stream<Item = reqwest::Result<Bytes>> + Send>>,
    buffer: String,
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
            inner: Box::pin(response.bytes_stream()),
            buffer: String::new(),
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

    /// Parse one SSE `data:` payload into a stream event.
    fn parse_event(data: &str) -> Result<Option<ParsedEvent>> {
        if data.trim().is_empty() {
            return Ok(None);
        }
        if data.trim() == "[DONE]" {
            return Ok(Some(ParsedEvent::Done { usage: None }));
        }

        let response: StreamResponse = serde_json::from_str(data)?;
        let usage = response.usage.map(Usage::from);
        let Some(choice) = response.choices.into_iter().next() else {
            return Ok(usage.map(|usage| ParsedEvent::Done { usage: Some(usage) }));
        };

        if let Some(fragments) = choice.delta.tool_calls
            && !fragments.is_empty()
        {
            return Ok(Some(ParsedEvent::ToolFragments(fragments)));
        }
        if let Some(reasoning) = choice.delta.reasoning_content
            && !reasoning.is_empty()
        {
            return Ok(Some(ParsedEvent::Reasoning(reasoning)));
        }
        if let Some(content) = choice.delta.content
            && !content.is_empty()
        {
            return Ok(Some(ParsedEvent::Delta(content)));
        }
        if choice.finish_reason.is_some() {
            return Ok(Some(ParsedEvent::Done { usage }));
        }
        Ok(None)
    }
}

enum ParsedEvent {
    Reasoning(String),
    Delta(String),
    ToolFragments(Vec<DeltaToolCall>),
    Done { usage: Option<Usage> },
}

impl Stream for OpenAIStream {
    type Item = Result<ChatChunk>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        if self.finished {
            return Poll::Ready(None);
        }

        // Flush assembled tool calls and the terminal chunk in order.
        if let Some(chunk) = self.queued.pop_front() {
            return Poll::Ready(Some(Ok(chunk)));
        }

        loop {
            if let Some(index) = self.buffer.find("\n\n") {
                let event = self.buffer[..index].to_string();
                self.buffer = self.buffer[index + 2..].to_string();

                for line in event.lines() {
                    let Some(data) = line.strip_prefix("data:") else {
                        continue;
                    };
                    match Self::parse_event(data.trim()) {
                        Ok(Some(ParsedEvent::Reasoning(content))) => {
                            return Poll::Ready(Some(Ok(ChatChunk::ReasoningDelta { content })));
                        }
                        Ok(Some(ParsedEvent::Delta(content))) => {
                            return Poll::Ready(Some(Ok(ChatChunk::Delta { content })));
                        }
                        Ok(Some(ParsedEvent::ToolFragments(fragments))) => {
                            for fragment in &fragments {
                                self.merge_tool_call_fragment(fragment);
                            }
                            continue;
                        }
                        Ok(Some(ParsedEvent::Done { usage })) => {
                            self.finished = true;
                            // Flush assembled tool calls (skip nameless) then
                            // the terminal chunk.
                            let calls: Vec<ChatChunk> = self
                                .pending_tool_calls
                                .drain(..)
                                .filter(|c| !c.name.is_empty())
                                .map(|c| ChatChunk::ToolCall {
                                    id: c.id,
                                    name: c.name,
                                    arguments: c.arguments,
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
                        Ok(None) => continue,
                        Err(err) => return Poll::Ready(Some(Err(err))),
                    }
                }
            }

            match self.inner.as_mut().poll_next(cx) {
                Poll::Ready(Some(Ok(bytes))) => {
                    let text = String::from_utf8_lossy(&bytes);
                    self.buffer.push_str(&text);
                    continue;
                }
                Poll::Ready(Some(Err(err))) => {
                    return Poll::Ready(Some(Err(AiError::Reqwest(err))));
                }
                Poll::Ready(None) => {
                    self.finished = true;
                    return Poll::Ready(None);
                }
                Poll::Pending => return Poll::Pending,
            }
        }
    }
}
