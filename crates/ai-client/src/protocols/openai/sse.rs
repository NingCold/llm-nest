use std::{
    pin::Pin,
    task::{Context, Poll},
};

use crate::chunk::ChatChunk;
use bytes::Bytes;
use futures_util::Stream;

use super::chat::StreamResponse;
use crate::error::{AiError, Result};

pub struct OpenAIStream {
    inner: Pin<Box<dyn Stream<Item = reqwest::Result<Bytes>> + Send>>,
    buffer: String,
    finished: bool,
}

impl OpenAIStream {
    pub fn new(response: reqwest::Response) -> Self {
        Self {
            inner: Box::pin(response.bytes_stream()),
            buffer: String::new(),
            finished: false,
        }
    }

    fn parse_event(data: &str) -> Result<Option<ChatChunk>> {
        if data.trim().is_empty() {
            return Ok(None);
        }

        if data.trim() == "[DONE]" {
            return Ok(Some(ChatChunk::Done));
        }

        let response: StreamResponse = serde_json::from_str(data)?;

        let choice = match response.choices.into_iter().next() {
            Some(c) => c,
            None => return Ok(None),
        };

        if let Some(reasoning) = choice.delta.reasoning_content
            && !reasoning.is_empty()
        {
            return Ok(Some(ChatChunk::ReasoningDelta { content: reasoning }));
        }

        if let Some(content) = choice.delta.content {
            return Ok(Some(ChatChunk::Delta { content }));
        }

        if choice.finish_reason.is_some() {
            return Ok(Some(ChatChunk::Done));
        }
        Ok(None)
    }
}

impl Stream for OpenAIStream {
    type Item = Result<ChatChunk>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        if self.finished {
            return Poll::Ready(None);
        }

        loop {
            if let Some(index) = self.buffer.find("\n\n") {
                let event = self.buffer[..index].to_string();
                self.buffer = self.buffer[index + 2..].to_string();

                for line in event.lines() {
                    if let Some(data) = line.strip_prefix("data:") {
                        match Self::parse_event(data.trim()) {
                            Ok(Some(ChatChunk::Done)) => {
                                self.finished = true;
                                return Poll::Ready(Some(Ok(ChatChunk::Done)));
                            }
                            Ok(Some(chunk)) => {
                                return Poll::Ready(Some(Ok(chunk)));
                            }
                            Ok(None) => continue,
                            Err(err) => return Poll::Ready(Some(Err(err))),
                        }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reasoning_content_becomes_reasoning_delta() {
        let data = r#"{"id":"x","choices":[{"index":0,"delta":{"role":"assistant","reasoning_content":"let me think"},"finish_reason":null}]}"#;
        let chunk = OpenAIStream::parse_event(data).unwrap().unwrap();
        match chunk {
            ChatChunk::ReasoningDelta { content } => assert_eq!(content, "let me think"),
            other => panic!("expected ReasoningDelta, got {other:?}"),
        }
    }

    #[test]
    fn reasoning_and_content_deltas_are_distinct() {
        let reasoning = r#"{"id":"x","choices":[{"index":0,"delta":{"reasoning_content":"chain"},"finish_reason":null}]}"#;
        assert!(matches!(
            OpenAIStream::parse_event(reasoning).unwrap().unwrap(),
            ChatChunk::ReasoningDelta { .. }
        ));
        let content = r#"{"id":"x","choices":[{"index":0,"delta":{"content":"answer"},"finish_reason":null}]}"#;
        assert!(matches!(
            OpenAIStream::parse_event(content).unwrap().unwrap(),
            ChatChunk::Delta { .. }
        ));
    }
}
