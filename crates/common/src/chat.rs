use serde::{Deserialize, Serialize};

use crate::{Usage, config::{ModelId, provider::ProviderId}, message::Message};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatOptions {
    pub provider: ProviderId,
    pub model: ModelId,
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
    pub top_p: Option<f32>,
    pub stream: bool,
}

impl Default for ChatOptions {
    fn default() -> Self {
        Self {
            provider: ProviderId::new("chatecnu"),
            model: ModelId::new("ecnu-max"),
            temperature: None,
            max_tokens: None,
            top_p: None,
            stream: false,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ChatRequest {
    pub messages: Vec<Message>,
    pub options: ChatOptions,
}

impl ChatRequest {
    pub fn new(
        messages: Vec<Message>,
        options: ChatOptions,
    ) -> Self {
        Self {
            messages,
            options,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct ChatResponse {
    pub message: Message,
    pub usage: Option<Usage>,
}

#[derive(Debug, Clone, Deserialize)]
pub enum ChatChunk {
    Delta {
        content: String,
    },
    Done,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chat_options_default_values() {
        let opts = ChatOptions::default();
        assert!(!opts.stream);
        assert_eq!(opts.provider.to_string(), "chatecnu");
        assert_eq!(opts.model.to_string(), "ecnu-max");
    }

    #[test]
    fn chat_request_new() {
        let opts = ChatOptions::default();
        let msgs = vec![Message::user("hi")];
        let req = ChatRequest::new(msgs.clone(), opts.clone());
        assert_eq!(req.messages.len(), 1);
        assert_eq!(req.messages[0].content, "hi");
    }

    #[test]
    fn chat_chunk_serde_delta() {
        let json = r#"{"Delta": {"content": "hello"}}"#;
        let chunk: ChatChunk = serde::Deserialize::deserialize(
            &mut serde_json::Deserializer::from_str(json),
        ).unwrap();
        match chunk {
            ChatChunk::Delta { content } => assert_eq!(content, "hello"),
            ChatChunk::Done => panic!("expected Delta"),
        }
    }

    #[test]
    fn chat_chunk_serde_done() {
        let json = r#""Done""#;
        let chunk: ChatChunk = serde::Deserialize::deserialize(
            &mut serde_json::Deserializer::from_str(json),
        ).unwrap();
        assert!(matches!(chunk, ChatChunk::Done));
    }

    #[test]
    fn chat_response_fields() {
        let msg = Message::assistant("response");
        let usage = Usage { prompt_tokens: 10, completion_tokens: 20, total_tokens: 30 };
        let resp = ChatResponse { message: msg, usage: Some(usage) };
        assert_eq!(resp.message.content, "response");
        assert_eq!(resp.usage.unwrap().total_tokens, 30);
    }
}