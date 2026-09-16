//! Shared SSE frame extraction for streaming responses.
//!
//! Both OpenAI-style (`data: {...}\n\n`) and Anthropic-style (`event: ...\n
//! data: {...}\n\n`) streams separate events with a blank line and carry the
//! payload on `data:` lines. This module frames a byte stream into those
//! payloads; each protocol parses them with its own event vocabulary.
//! `event:` lines (Anthropic) and empty events are dropped here, so a parser
//! only ever sees payload JSON (or `[DONE]`).

use std::pin::Pin;
use std::task::{Context, Poll};

use bytes::Bytes;
use futures_util::Stream;

use crate::error::Result;

/// Position and separator length of the next frame boundary in `buf`.
///
/// Most servers separate events with a blank line `\n\n`; Gemini's
/// `streamGenerateContent?alt=sse` uses CRLF (`\r\n\r\n`). Both are accepted;
/// the earliest boundary wins.
fn find_frame_boundary(buf: &[u8]) -> Option<(usize, usize)> {
    let lf = buf.windows(2).position(|w| w == b"\n\n").map(|i| (i, 2));
    let crlf = buf
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .map(|i| (i, 4));
    match (lf, crlf) {
        (Some(a), Some(b)) => Some(if a.0 <= b.0 { a } else { b }),
        (a, b) => a.or(b),
    }
}

/// One `data:` payload extracted from an SSE byte stream.
pub struct SseDataStream {
    inner: Pin<Box<dyn Stream<Item = reqwest::Result<Bytes>> + Send>>,
    buffer: Vec<u8>,
    finished: bool,
}

impl SseDataStream {
    pub fn new(response: reqwest::Response) -> Self {
        Self::from_stream(response.bytes_stream())
    }

    pub fn from_stream(
        stream: impl Stream<Item = reqwest::Result<Bytes>> + Send + 'static,
    ) -> Self {
        Self {
            inner: Box::pin(stream),
            buffer: Vec::new(),
            finished: false,
        }
    }
}

impl Stream for SseDataStream {
    /// Each item is one trimmed `data:` payload.
    type Item = Result<String>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        if self.finished {
            return Poll::Ready(None);
        }
        loop {
            if let Some((index, sep_len)) = find_frame_boundary(&self.buffer) {
                let bytes: Vec<_> = self.buffer.drain(..index + sep_len).collect();
                let event = match std::str::from_utf8(&bytes[..index]) {
                    Ok(event) => event,
                    Err(e) => {
                        self.finished = true;
                        return Poll::Ready(Some(Err(crate::error::AiError::StreamError(
                            e.to_string(),
                        ))));
                    }
                };
                let data: Vec<_> = event
                    .lines()
                    .filter_map(|line| {
                        line.strip_prefix("data:")
                            .map(|s| s.strip_prefix(' ').unwrap_or(s))
                    })
                    .collect();
                if !data.is_empty() {
                    return Poll::Ready(Some(Ok(data.join("\n"))));
                }
                continue;
            }

            match self.inner.as_mut().poll_next(cx) {
                Poll::Ready(Some(Ok(bytes))) => {
                    self.buffer.extend_from_slice(&bytes);
                    continue;
                }
                Poll::Ready(Some(Err(err))) => {
                    self.finished = true;
                    return Poll::Ready(Some(Err(crate::error::AiError::Reqwest(err))));
                }
                Poll::Ready(None) => {
                    self.finished = true;
                    if !self.buffer.is_empty() {
                        return Poll::Ready(Some(Err(crate::error::AiError::StreamError(
                            "truncated SSE frame".into(),
                        ))));
                    }
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

    #[tokio::test]
    async fn frames_data_payloads() {
        // OpenAI style
        let body = "data: {\"a\":1}\n\ndata: [DONE]\n\n";
        let stream = SseDataStream {
            inner: Box::pin(futures_util::stream::iter(vec![Ok(bytes::Bytes::from(
                body.as_bytes().to_vec(),
            ))])),
            buffer: Vec::new(),
            finished: false,
        };
        let items: Vec<String> = stream.map(|r| r.unwrap()).collect().await;
        assert_eq!(items, vec!["{\"a\":1}".to_string(), "[DONE]".to_string()]);
    }

    #[tokio::test]
    async fn drops_event_lines_and_split_frames() {
        // Anthropic style with event: lines, split across two byte chunks
        let part1 = "event: message_start\ndata: {\"type\":\"message_start\"}\n\nevent: content_block_delta\n";
        let part2 = "data: {\"type\":\"content_block_delta\",\"delta\":{\"type\":\"text_delta\",\"text\":\"hi\"}}\n\n";
        let stream = SseDataStream {
            inner: Box::pin(futures_util::stream::iter(vec![
                Ok(bytes::Bytes::from(part1.as_bytes().to_vec())),
                Ok(bytes::Bytes::from(part2.as_bytes().to_vec())),
            ])),
            buffer: Vec::new(),
            finished: false,
        };
        let items: Vec<String> = stream.map(|r| r.unwrap()).collect().await;
        assert_eq!(items.len(), 2);
        assert!(items[0].contains("message_start"));
        assert!(items[1].contains("text_delta"));
    }

    #[tokio::test]
    async fn frames_crlf_separated_payloads() {
        // Gemini's streamGenerateContent uses \r\n\r\n between events.
        let body = "data: {\"a\":1}\r\n\r\ndata: {\"b\":2}\r\n\r\n";
        let stream = SseDataStream {
            inner: Box::pin(futures_util::stream::iter(vec![Ok(bytes::Bytes::from(
                body.as_bytes().to_vec(),
            ))])),
            buffer: Vec::new(),
            finished: false,
        };
        let items: Vec<String> = stream.map(|r| r.unwrap()).collect().await;
        assert_eq!(
            items,
            vec!["{\"a\":1}".to_string(), "{\"b\":2}".to_string()]
        );
    }
}
