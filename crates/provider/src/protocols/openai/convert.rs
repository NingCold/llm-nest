use crate::ProviderRequest;
use crate::error::ProviderError;
use super::chat;

pub fn to_real_request(
    req: ProviderRequest,
) -> chat::Request {
    chat::Request {
        model: req.model,
        messages: req.messages,
        temperature: req.parameters.temperature,
        max_tokens: req.parameters.max_tokens,
        top_p: req.parameters.top_p,
        stream: req.parameters.stream,
    }
}

impl TryFrom<chat::Response> for crate::ProviderResponse {
    type Error = ProviderError;

    fn try_from(
        resp: chat::Response,
    ) -> Result<Self, Self::Error> {
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