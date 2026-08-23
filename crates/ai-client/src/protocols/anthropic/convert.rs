//! Wire types and conversions for the Anthropic Messages API.

use common::{Message, Role, Usage};
use serde::{Deserialize, Serialize};

use crate::chunk::ChatChunk;
use crate::error::{AiError, Result};
use crate::reasoning::{DEFAULT_THINKING_BUDGET_TOKENS, ReasoningEffort, ReasoningFormat};
use crate::request::ChatRequest;
use crate::response::ProviderResponse;

/// Anthropic `thinking` parameter: `{ type: "enabled", budget_tokens }` or
/// `{ type: "disabled" }`.
#[derive(Debug, Clone, Serialize)]
pub struct Thinking {
    #[serde(rename = "type")]
    pub typ: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub budget_tokens: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Request {
    pub model: String,
    /// Mandatory on the wire; sourced from options, then the model's declared
    /// output capability, then a default.
    pub max_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system: Option<String>,
    pub messages: Vec<Message>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f32>,
    #[serde(default)]
    pub stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking: Option<Thinking>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Response {
    pub id: String,
    pub content: Vec<ContentBlock>,
    pub usage: Option<ResponseUsage>,
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
}

/// `max_tokens` fallback when neither the request options nor the model's
/// declared capability provide one.
pub const DEFAULT_MAX_TOKENS: u32 = 4096;

/// Build the wire request from a routed chat request.
pub fn to_request(req: &ChatRequest) -> Request {
    let (system, messages) = split_system(&req.messages);
    Request {
        model: req.selection.model.clone(),
        max_tokens: req
            .options
            .max_tokens
            .or_else(|| req.resolved.as_ref().and_then(|r| r.spec.max_tokens))
            .unwrap_or(DEFAULT_MAX_TOKENS),
        system: (!system.is_empty()).then_some(system),
        messages,
        temperature: req.options.temperature,
        top_p: req.options.top_p,
        stream: req.options.stream,
        thinking: thinking(req),
    }
}

/// Anthropic has no system/developer/tool message roles: system and developer
/// prompts fold into the top-level `system` field, tool results are carried as
/// user text (llm-nest has no tool calling yet), and the messages array keeps
/// only user/assistant turns — the two roles the API accepts.
fn split_system(messages: &[Message]) -> (String, Vec<Message>) {
    let mut system = Vec::new();
    let mut turns = Vec::new();
    for message in messages {
        match message.role {
            Role::System | Role::Developer => system.push(message.text()),
            Role::User => turns.push(message.clone()),
            Role::Assistant => turns.push(message.clone()),
            Role::Tool => turns.push(Message::user(message.text())),
        }
    }
    (system.join("\n\n"), turns)
}

/// Anthropic `thinking.budget_tokens` per neutral level when the model's
/// capability declares no explicit `budget_tokens` — mirrors the Gemini
/// per-level table so every supported level has real wire meaning.
const LEVEL_THINKING_BUDGET: [(ReasoningEffort, u32); 3] = [
    (ReasoningEffort::Low, 1024),
    (ReasoningEffort::Medium, 4096),
    (ReasoningEffort::High, 16384),
];

/// Map the routed neutral effort to the Anthropic `thinking` block. Only
/// `AnthropicThinking`-formatted models carry it; the budget comes from the
/// model's declared `budget_tokens` when present, otherwise a per-level
/// default (1024/4096/16384). `Off` disables thinking explicitly, unset
/// leaves provider default behavior.
fn thinking(req: &ChatRequest) -> Option<Thinking> {
    let resolved = req.resolved.as_ref()?;
    let capability = resolved.spec.reasoning.as_ref()?;
    if capability.format != ReasoningFormat::AnthropicThinking {
        return None;
    }
    match req.selection.reasoning_effort? {
        ReasoningEffort::Off => Some(Thinking {
            typ: "disabled".into(),
            budget_tokens: None,
        }),
        level => Some(Thinking {
            typ: "enabled".into(),
            budget_tokens: Some(capability.budget_tokens.unwrap_or_else(|| {
                LEVEL_THINKING_BUDGET
                    .iter()
                    .find(|(l, _)| *l == level)
                    .map(|(_, b)| *b)
                    .unwrap_or(DEFAULT_THINKING_BUDGET_TOKENS)
            })),
        }),
    }
}

/// Non-streaming response → provider response: concatenate `text` blocks,
/// remap usage token names (Anthropic has no `total_tokens`; derive it).
pub fn to_provider_response(resp: Response) -> Result<ProviderResponse> {
    let text = resp
        .content
        .iter()
        .filter(|block| block.typ == "text")
        .filter_map(|block| block.text.clone())
        .collect::<String>();
    let usage = resp.usage.map(|u| {
        let input = u.input_tokens.unwrap_or(0);
        let output = u.output_tokens.unwrap_or(0);
        Usage {
            prompt_tokens: input,
            completion_tokens: output,
            total_tokens: input + output,
        }
    });
    Ok(ProviderResponse {
        message: Message::assistant(text),
        reasoning: None,
        usage,
    })
}

/// One SSE payload → optional chunk. `content_block_delta` text deltas stream
/// content; `message_stop` closes the stream; an `error` event surfaces as a
/// stream error.
pub fn parse_event(data: &str) -> Result<Option<ChatChunk>> {
    #[derive(Deserialize)]
    struct Event {
        #[serde(rename = "type")]
        typ: String,
        delta: Option<Delta>,
        error: Option<EventError>,
    }
    #[derive(Deserialize)]
    struct Delta {
        #[serde(rename = "type")]
        typ: String,
        text: Option<String>,
    }
    #[derive(Deserialize)]
    struct EventError {
        message: Option<String>,
    }
    let event: Event = serde_json::from_str(data)?;
    match event.typ.as_str() {
        "content_block_delta" => match event.delta {
            Some(delta) if delta.typ == "text_delta" => {
                Ok(delta.text.map(|t| ChatChunk::Delta { content: t }))
            }
            _ => Ok(None),
        },
        "message_stop" => Ok(Some(ChatChunk::Done)),
        "error" => Err(AiError::StreamError(
            event
                .error
                .and_then(|e| e.message)
                .unwrap_or_else(|| "unknown anthropic stream error".into()),
        )),
        _ => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use common::GenerationOptions;

    use crate::config::{ApiKey, ModelConfig, ModelId, Protocol, ProviderConfig, ProviderId};
    use crate::reasoning::ReasoningCapability;
    use crate::router::ModelRouter;

    fn request_with(messages: Vec<Message>, effort: Option<ReasoningEffort>) -> ChatRequest {
        let mut configs = std::collections::HashMap::new();
        configs.insert(
            ProviderId::new("anthropic"),
            ProviderConfig {
                protocol: Some(Protocol::Anthropic),
                api_key: ApiKey::Direct("k".into()),
                base_url: Some("https://api.anthropic.com/v1".into()),
                models: {
                    let mut m = std::collections::HashMap::new();
                    m.insert(
                        ModelId::new("m"),
                        ModelConfig {
                            model: "claude-test".into(),
                            display_name: None,
                            context_window: None,
                            max_tokens: Some(16384),
                            reasoning: Some(ReasoningCapability {
                                levels: vec![ReasoningEffort::Low, ReasoningEffort::High],
                                format: ReasoningFormat::AnthropicThinking,
                                budget_tokens: Some(2048),
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
            provider: "anthropic".into(),
            model: "claude-test".into(),
            reasoning_effort: effort,
        };
        let resolved = router.resolve(&selection).unwrap();
        let mut req = ChatRequest {
            selection,
            messages,
            options: GenerationOptions::default(),
            resolved: None,
        };
        req.resolved = Some(resolved);
        req
    }

    #[test]
    fn separates_system_and_keeps_only_turn_roles() {
        let req = request_with(
            vec![
                Message::system("You are helpful."),
                Message::user("hi"),
                Message::assistant("hello"),
                Message::developer("be concise"),
                Message::user("thanks"),
            ],
            None,
        );
        let wire = to_request(&req);
        assert_eq!(
            wire.system.as_deref(),
            Some("You are helpful.\n\nbe concise")
        );
        let roles: Vec<&str> = wire
            .messages
            .iter()
            .map(|m| match m.role {
                Role::User => "user",
                Role::Assistant => "assistant",
                _ => "other",
            })
            .collect();
        assert_eq!(roles, vec!["user", "assistant", "user"]);
    }

    #[test]
    fn max_tokens_falls_back_to_model_capability() {
        let req = request_with(vec![Message::user("hi")], None);
        let wire = to_request(&req);
        assert_eq!(wire.max_tokens, 16384);
    }

    #[test]
    fn thinking_enabled_with_budget() {
        let req = request_with(vec![Message::user("hi")], Some(ReasoningEffort::High));
        let wire = to_request(&req);
        let thinking = wire.thinking.unwrap();
        assert_eq!(thinking.typ, "enabled");
        // configured budget_tokens overrides the per-level default
        assert_eq!(thinking.budget_tokens, Some(2048));
    }

    #[test]
    fn thinking_budget_scales_with_level() {
        // no configured budget → per-level defaults (low 1024, high 16384)
        let mut req = request_with(vec![Message::user("hi")], Some(ReasoningEffort::Low));
        req.resolved
            .as_mut()
            .unwrap()
            .spec
            .reasoning
            .as_mut()
            .unwrap()
            .budget_tokens = None;
        let wire = to_request(&req);
        assert_eq!(wire.thinking.unwrap().budget_tokens, Some(1024));

        let mut req = request_with(vec![Message::user("hi")], Some(ReasoningEffort::High));
        req.resolved
            .as_mut()
            .unwrap()
            .spec
            .reasoning
            .as_mut()
            .unwrap()
            .budget_tokens = None;
        let wire = to_request(&req);
        assert_eq!(wire.thinking.unwrap().budget_tokens, Some(16384));
    }

    #[test]
    fn thinking_off_disables() {
        let req = request_with(vec![Message::user("hi")], Some(ReasoningEffort::Off));
        let wire = to_request(&req);
        assert_eq!(wire.thinking.unwrap().typ, "disabled");
    }

    #[test]
    fn unset_effort_leaves_thinking_absent() {
        let req = request_with(vec![Message::user("hi")], None);
        let wire = to_request(&req);
        assert!(wire.thinking.is_none());
    }

    #[test]
    fn parses_non_stream_response() {
        let json = r#"{
            "id": "msg_1",
            "type": "message",
            "role": "assistant",
            "content": [
                {"type": "text", "text": "hello "},
                {"type": "text", "text": "world"}
            ],
            "usage": {"input_tokens": 5, "output_tokens": 7}
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
        let delta = r#"{"type":"content_block_delta","delta":{"type":"text_delta","text":"hi"}}"#;
        assert!(matches!(
            parse_event(delta).unwrap(),
            Some(ChatChunk::Delta { ref content }) if content == "hi"
        ));
        assert!(matches!(
            parse_event(r#"{"type":"message_stop"}"#).unwrap(),
            Some(ChatChunk::Done)
        ));
        // signature_delta / thinking_delta blocks are skipped
        let thinking =
            r#"{"type":"content_block_delta","delta":{"type":"thinking_delta","thinking":"..."}}"#;
        assert!(parse_event(thinking).unwrap().is_none());
    }
}
