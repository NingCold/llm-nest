use common::{ChatRequest, ChatResponse};

use crate::error::{ProviderError, Result};

use super::chat;

pub fn to_real_request(
    req: ChatRequest,
) -> chat::Request {
    chat::Request {
        model: req.options.model.to_string(),
        messages: req.messages,
        temperature: req.options.temperature,
        max_tokens: req.options.max_tokens,
        top_p: req.options.top_p,
        stream: req.options.stream,
    }
}

impl TryFrom<chat::Response> for ChatResponse {
    type Error = ProviderError;

    fn try_from(
        resp: chat::Response,
    ) -> Result<Self> {

        let choice = resp
            .choices
            .into_iter()
            .next()
            .ok_or_else(|| ProviderError::InvalidResponse(
                "No choices returned in response".into()
            ))?;

        Ok(Self {
            message: choice.message,
            usage: resp.usage,
        })
    }
}

