use std::pin::Pin;
use std::task::{Context, Poll};

use futures_util::Stream;

use crate::chunk::ChatChunk;
use crate::error::Result;

type Inner = Pin<Box<dyn Stream<Item = Result<ChatChunk>> + Send + 'static>>;

pub struct ChatStream {
    inner: Inner,
}

impl ChatStream {
    pub fn new<S>(stream: S) -> Self
    where
        S: Stream<Item = Result<ChatChunk>> + Send + 'static,
    {
        Self {
            inner: Box::pin(stream),
        }
    }
}

impl Stream for ChatStream {
    type Item = Result<ChatChunk>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.inner.as_mut().poll_next(cx)
    }
}
