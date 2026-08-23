//! Wire types and conversions for the OpenAI Responses API.

use common::{Message, Usage};
use serde::{Deserialize, Serialize};

use crate::chunk::ChatChunk;
use crate::error::{AiError, Result};
use crate::reasoning::{ReasoningEffort, ReasoningFormat};
use crate::request::ChatRequest;
use crate::response::ProviderResponse;

/// `reasoning: { effort }` — Responses API reasoning dispatch.
#[derive(Debug, Clone, Serialize)]
pub struct Reasoning {
    pub effort: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Request {
    pub model: String,
    pub input: Vec<Message>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f32>,
    #[serde(default)]
    pub stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<Reasoning>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Response {
    pub id: String,
    pub output: Vec<OutputItem>,
    pub usage: Option<ResponseUsage>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct OutputItem {
    #[serde(rename = "type")]
    pub typ: String,
    pub content: Option<Vec<ContentBlock>>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ContentBlock {
    #[serde(rename = "type")]
    pub typ: String,
    pub text: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ResponseUsage {
    pub input_tokens: Option<u32>,
    pub output_tokens: Option<u32>,
    pub total_tokens: Option<u32>,
}

/// Build the wire request from a routed chat request.
pub fn to_request(req: &ChatRequest) -> Request {
    Request {
        model: req.selection.model.clone(),
        input: req.messages.clone(),
        temperature: req.options.temperature,
        max_output_tokens: req.options.max_tokens,
        top_p: req.options.top_p,
        stream: req.options.stream,
        reasoning: reasoning(req),
    }
}

/// Map the routed neutral effort to the Responses `reasoning: { effort }`
/// shape. Only `OpenAIEffort`-formatted models carry it; `Off` and unset leave
/// the field absent, and a legacy (unresolved) request emits nothing.
fn reasoning(req: &ChatRequest) -> Option<Reasoning> {
    let resolved = req.resolved.as_ref()?;
    let capability = resolved.spec.reasoning.as_ref()?;
    if capability.format != ReasoningFormat::OpenAIEffort {
        return None;
    }
    match req.selection.reasoning_effort? {
        ReasoningEffort::Off => None,
        level => Some(Reasoning {
            effort: level.as_wire().to_string(),
        }),
    }
}

/// Non-streaming response → provider response: concatenate `output_text`
/// blocks of message items, remap usage token names.
pub fn to_provider_response(resp: Response) -> Result<ProviderResponse> {
    let text = resp
        .output
        .iter()
        .filter(|item| item.typ == "message")
        .filter_map(|item| item.content.as_ref())
        .flatten()
        .filter(|block| block.typ == "output_text")
        .filter_map(|block| block.text.clone())
        .collect::<String>();
    let usage = resp.usage.map(|u| Usage {
        prompt_tokens: u.input_tokens.unwrap_or(0),
        completion_tokens: u.output_tokens.unwrap_or(0),
        total_tokens: u.total_tokens.unwrap_or(0),
    });
    Ok(ProviderResponse {
        message: Message::assistant(text),
        reasoning: None,
        usage,
    })
}

/// One SSE payload → optional chunk. `[DONE]` and the terminal response
/// events close the stream; an `error` event surfaces as a stream error.
pub fn parse_event(data: &str) -> Result<Option<ChatChunk>> {
    if data.trim() == "[DONE]" {
        return Ok(Some(ChatChunk::Done));
    }
    #[derive(Deserialize)]
    struct Event {
        #[serde(rename = "type")]
        typ: String,
        delta: Option<String>,
        error: Option<EventError>,
    }
    #[derive(Deserialize)]
    struct EventError {
        message: Option<String>,
    }
    let event: Event = serde_json::from_str(data)?;
    match event.typ.as_str() {
        "response.output_text.delta" => Ok(event.delta.map(|d| ChatChunk::Delta { content: d })),
        "response.completed" | "response.incomplete" | "response.failed" => {
            Ok(Some(ChatChunk::Done))
        }
        "error" => Err(AiError::StreamError(
            event
                .error
                .and_then(|e| e.message)
                .unwrap_or_else(|| "unknown responses stream error".into()),
        )),
        _ => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use common::{GenerationOptions, Message};

    use crate::config::{ApiKey, ModelConfig, ModelId, Protocol, ProviderConfig, ProviderId};
    use crate::reasoning::ReasoningCapability;
    use crate::router::ModelRouter;

    fn resolved_with(
        effort: Option<ReasoningEffort>,
        format: ReasoningFormat,
    ) -> (ChatRequest, Option<Reasoning>) {
        let mut configs = std::collections::HashMap::new();
        configs.insert(
            ProviderId::new("openai"),
            ProviderConfig {
                protocol: Some(Protocol::OpenAIResponses),
                api_key: ApiKey::Direct("k".into()),
                base_url: Some("https://example.com/v1".into()),
                models: {
                    let mut m = std::collections::HashMap::new();
                    m.insert(
                        ModelId::new("m"),
                        ModelConfig {
                            model: "test-model".into(),
                            display_name: None,
                            context_window: None,
                            max_tokens: None,
                            reasoning: Some(ReasoningCapability {
                                levels: vec![ReasoningEffort::Low, ReasoningEffort::High],
                                format,
                                budget_tokens: None,
                            }),
                            protocol: None,
                        },
                    );
                    m
                },
                default_model: None,
                headers: std::collections::HashMap::new(),
                timeout_ms: None,
            },
        );
        let router = ModelRouter::new(&configs).unwrap();
        let selection = crate::request::ModelSelection {
            provider: "openai".into(),
            model: "test-model".into(),
            reasoning_effort: effort,
        };
        let resolved = router.resolve(&selection).unwrap();
        let mut req = ChatRequest {
            selection,
            messages: vec![Message::user("hi")],
            options: GenerationOptions::default(),
            resolved: None,
        };
        let reasoning = to_request(&{
            req.resolved = Some(resolved);
            req.clone()
        })
        .reasoning;
        (req, reasoning)
    }

    #[test]
    fn maps_effort_to_reasoning_object() {
        let (_, reasoning) =
            resolved_with(Some(ReasoningEffort::High), ReasoningFormat::OpenAIEffort);
        assert_eq!(reasoning.map(|r| r.effort).as_deref(), Some("high"));
    }

    #[test]
    fn off_and_unset_omit_reasoning() {
        let (_, off) = resolved_with(Some(ReasoningEffort::Off), ReasoningFormat::OpenAIEffort);
        assert!(off.is_none());
        let (_, unset) = resolved_with(None, ReasoningFormat::OpenAIEffort);
        assert!(unset.is_none());
    }

    #[test]
    fn non_openai_format_omits_reasoning() {
        let (_, thinking) = resolved_with(
            Some(ReasoningEffort::High),
            ReasoningFormat::DeepSeekThinking,
        );
        assert!(thinking.is_none());
    }

    #[test]
    fn uses_max_output_tokens() {
        let (req, _) = resolved_with(None, ReasoningFormat::OpenAIEffort);
        let mut req = req;
        req.options.max_tokens = Some(2048);
        let wire = to_request(&req);
        assert_eq!(wire.max_output_tokens, Some(2048));
    }

    #[test]
    fn parses_non_stream_response() {
        let json = r#"{
            "id": "resp_1",
            "output": [
                {"type": "message", "role": "assistant", "content": [
                    {"type": "output_text", "text": "hello "},
                    {"type": "output_text", "text": "world"}
                ]}
            ],
            "usage": {"input_tokens": 5, "output_tokens": 7, "total_tokens": 12}
        }"#;
        let resp: Response = serde_json::from_str(json).unwrap();
        let provider = to_provider_response(resp).unwrap();
        assert_eq!(provider.message.text(), "hello world");
        let usage = provider.usage.unwrap();
        assert_eq!(usage.prompt_tokens, 5);
        assert_eq!(usage.completion_tokens, 7);
        assert_eq!(usage.total_tokens, 12);
    }

    #[test]
    fn parses_stream_events() {
        assert!(matches!(
            parse_event(r#"{"type":"response.output_text.delta","delta":"hi"}"#).unwrap(),
            Some(ChatChunk::Delta { ref content }) if content == "hi"
        ));
        assert!(matches!(
            parse_event(r#"{"type":"response.completed"}"#).unwrap(),
            Some(ChatChunk::Done)
        ));
        assert!(parse_event(r#"[DONE]"#).unwrap().is_some());
        // irrelevant events are skipped
        assert!(
            parse_event(r#"{"type":"response.created"}"#)
                .unwrap()
                .is_none()
        );
    }
}
