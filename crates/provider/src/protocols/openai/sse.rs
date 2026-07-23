use std::{pin::Pin, task::{Context, Poll}};

use bytes::Bytes;
use common::ChatChunk;
use futures_util::Stream;


use crate::{error::{ProviderError, Result}, protocols::openai::chat::StreamResponse};

pub struct OpenAIStream {
    inner: Pin<Box<dyn Stream<Item = reqwest::Result<Bytes>> + Send>>,

    buffer: String,

    finished: bool,
}

impl OpenAIStream {
    pub fn new(
        response: reqwest::Response,
    ) -> Self {
        Self {
            inner: Box::pin(
                response.bytes_stream()
            ),

            buffer: String::new(),

            finished: false,
        }
    }

    fn parse_event(
        data: &str,
    ) -> Result<Option<ChatChunk>> {
        if data.trim().is_empty() {
            return Ok(None);
        }

        if data.trim() == "[DONE]" {
            return Ok(Some(ChatChunk::Done));
        }

        let response: StreamResponse = serde_json::from_str(data)?;

        let choice = response
            .choices
            .into_iter()
            .next()
            .ok_or_else(|| {
                ProviderError::InvalidResponse(
                    "No choices returned in stream response".into()
                )
            })?;

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

    fn poll_next(
        mut self: Pin<&mut Self>,
        cx : &mut Context<'_>,
    ) -> Poll<Option<Self::Item>> {
        if self.finished {
            return Poll::Ready(None);
        }

        loop {
            if let Some(index) = self.buffer.find("\n\n") {
                let event = self.buffer[..index].to_string();
                self.buffer = self.buffer[index + 2..].to_string();

                for line in event.lines() {
                    if let Some(data) = line.strip_prefix("data:") {
                        match Self::parse_event(
                            data.trim(),
                        ) {
                            Ok(Some(
                                ChatChunk::Done
                            )) => {
                                self.finished = true;
                                return Poll::Ready(
                                    Some(Ok(
                                        ChatChunk::Done
                                    ))
                                );
                            }

                            Ok(Some(chunk)) => {
                                return Poll::Ready(
                                    Some(Ok(chunk))
                                );
                            }

                            Ok(None) => {
                                continue;
                            }

                            Err(err) => {
                                return Poll::Ready(
                                    Some(Err(err))
                                )
                            }
                        }
                    }
                }


            }

            match self.inner.as_mut().poll_next(cx) {
                Poll::Ready(Some(Ok(bytes))) => {
                    let text = String::from_utf8_lossy(
                        &bytes
                    );
                    self.buffer.push_str(&text);
                    continue;
                }

                Poll::Ready(Some(Err(err))) => {
                    return Poll::Ready(
                        Some(
                            Err(
                                ProviderError::Reqwest(err)
                            )
                        )
                    );
                }

                Poll::Ready(None) => {
                    self.finished = true;
                    return Poll::Ready(None);
                }

                Poll::Pending => {
                    return Poll::Pending;
                }
            }
        }
    }
}