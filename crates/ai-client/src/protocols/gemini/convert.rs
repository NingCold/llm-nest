//! Wire types and conversions for the Gemini API.

use common::{Message, Role, Usage};
use serde::{Deserialize, Serialize};

use crate::chunk::ChatChunk;
use crate::error::{AiError, Result};
use crate::reasoning::{ReasoningEffort, ReasoningFormat};
use crate::request::ChatRequest;
use crate::response::ProviderResponse;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Part {
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Content {
    /// Absent in streamed candidates; defaulted for deserialization.
    #[serde(default)]
    pub role: String,
    pub parts: Vec<Part>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SystemInstruction {
    pub parts: Vec<Part>,
}

/// Gemini `thinkingBudget` for each neutral level when the model's capability
/// declares no explicit `budget_tokens`.
const LEVEL_THINKING_BUDGET: [(ReasoningEffort, u32); 3] = [
    (ReasoningEffort::Low, 1024),
    (ReasoningEffort::Medium, 4096),
    (ReasoningEffort::High, 16384),
];

#[derive(Debug, Clone, Serialize)]
pub struct ThinkingConfig {
    pub thinking_budget: u32,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct GenerationConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking_config: Option<ThinkingConfig>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Request {
    pub contents: Vec<Content>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system_instruction: Option<SystemInstruction>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub generation_config: Option<GenerationConfig>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Response {
    pub candidates: Vec<Candidate>,
    #[serde(rename = "usageMetadata")]
    pub usage_metadata: Option<UsageMetadata>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Candidate {
    pub content: Option<Content>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageMetadata {
    pub prompt_token_count: Option<u32>,
    pub candidates_token_count: Option<u32>,
    pub total_token_count: Option<u32>,
}

/// Build the wire request from a routed chat request.
pub fn to_request(req: &ChatRequest) -> Request {
    let mut system = Vec::new();
    let mut contents = Vec::new();
    for message in &req.messages {
        match message.role {
            // Gemini has no system/tool roles: system and developer prompts
            // fold into `systemInstruction`; tool results ride as user text.
            Role::System | Role::Developer => system.push(message.text()),
            Role::User => contents.push(Content {
                role: "user".into(),
                parts: vec![Part {
                    text: message.text(),
                }],
            }),
            Role::Assistant => contents.push(Content {
                role: "model".into(),
                parts: vec![Part {
                    text: message.text(),
                }],
            }),
            Role::Tool => contents.push(Content {
                role: "user".into(),
                parts: vec![Part {
                    text: message.text(),
                }],
            }),
        }
    }

    let thinking_config = thinking(req);
    let generation_config = GenerationConfig {
        temperature: req.options.temperature,
        max_output_tokens: req.options.max_tokens,
        top_p: req.options.top_p,
        thinking_config,
    };
    Request {
        contents,
        system_instruction: (!system.is_empty()).then(|| SystemInstruction {
            parts: vec![Part {
                text: system.join("\n\n"),
            }],
        }),
        generation_config: Some(generation_config),
    }
}

/// Map the routed neutral effort to the Gemini `thinkingBudget`. Only
/// `GeminiThinking`-formatted models carry it; the budget is the model's
/// declared `budget_tokens` when present, otherwise a per-level default
/// (`off` → 0, `low`/`medium`/`high` → 1024/4096/16384).
fn thinking(req: &ChatRequest) -> Option<ThinkingConfig> {
    let resolved = req.resolved.as_ref()?;
    let capability = resolved.spec.reasoning.as_ref()?;
    if capability.format != ReasoningFormat::GeminiThinking {
        return None;
    }
    let effort = req.selection.reasoning_effort?;
    let budget = match effort {
        ReasoningEffort::Off => 0,
        level => capability.budget_tokens.unwrap_or_else(|| {
            LEVEL_THINKING_BUDGET
                .iter()
                .find(|(l, _)| *l == level)
                .map(|(_, b)| *b)
                .unwrap_or(4096)
        }),
    };
    Some(ThinkingConfig {
        thinking_budget: budget,
    })
}

/// Non-streaming response → provider response: concatenate the first
/// candidate's text parts, remap usage token names.
pub fn to_provider_response(resp: Response) -> Result<ProviderResponse> {
    let text = resp
        .candidates
        .into_iter()
        .next()
        .and_then(|c| c.content)
        .map(|content| {
            content
                .parts
                .into_iter()
                .map(|p| p.text)
                .collect::<String>()
        })
        .unwrap_or_default();
    let usage = resp.usage_metadata.map(|u| {
        let prompt = u.prompt_token_count.unwrap_or(0);
        let completion = u.candidates_token_count.unwrap_or(0);
        Usage {
            prompt_tokens: prompt,
            completion_tokens: completion,
            total_tokens: u.total_token_count.unwrap_or(prompt + completion),
        }
    });
    Ok(ProviderResponse {
        message: Message::assistant(text),
        reasoning: None,
        usage,
    })
}

/// One SSE payload → optional chunk. `[DONE]` closes the stream; candidate
/// content text streams deltas; an `error` object surfaces as a stream error.
pub fn parse_event(data: &str) -> Result<Option<ChatChunk>> {
    if data.trim() == "[DONE]" {
        return Ok(Some(ChatChunk::Done));
    }
    #[derive(Deserialize)]
    struct Event {
        candidates: Option<Vec<Candidate>>,
        error: Option<EventError>,
    }
    #[derive(Deserialize)]
    struct EventError {
        message: Option<String>,
    }
    let event: Event = serde_json::from_str(data)?;
    if let Some(error) = event.error {
        return Err(AiError::StreamError(
            error
                .message
                .unwrap_or_else(|| "unknown gemini stream error".into()),
        ));
    }
    let text = event
        .candidates
        .and_then(|mut c| c.drain(..).next())
        .and_then(|c| c.content)
        .map(|content| {
            content
                .parts
                .into_iter()
                .map(|p| p.text)
                .collect::<String>()
        })
        .filter(|t| !t.is_empty());
    Ok(text.map(|t| ChatChunk::Delta { content: t }))
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
            ProviderId::new("gemini"),
            ProviderConfig {
                protocol: Some(Protocol::Gemini),
                api_key: ApiKey::Direct("k".into()),
                base_url: Some("https://generativelanguage.googleapis.com/v1beta".into()),
                models: {
                    let mut m = std::collections::HashMap::new();
                    m.insert(
                        ModelId::new("m"),
                        ModelConfig {
                            model: "gemini-2.5-pro".into(),
                            display_name: None,
                            context_window: None,
                            max_tokens: None,
                            reasoning: Some(ReasoningCapability {
                                levels: vec![ReasoningEffort::Low, ReasoningEffort::High],
                                format: ReasoningFormat::GeminiThinking,
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
            provider: "gemini".into(),
            model: "gemini-2.5-pro".into(),
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
    fn maps_roles_and_system_instruction() {
        let req = request_with(
            vec![
                Message::system("Be helpful."),
                Message::user("hi"),
                Message::assistant("hello"),
            ],
            None,
        );
        let wire = to_request(&req);
        assert_eq!(
            wire.system_instruction.unwrap().parts[0].text,
            "Be helpful."
        );
        let roles: Vec<&str> = wire.contents.iter().map(|c| c.role.as_str()).collect();
        assert_eq!(roles, vec!["user", "model"]);
    }

    #[test]
    fn thinking_budget_by_level() {
        let req = request_with(vec![Message::user("hi")], Some(ReasoningEffort::High));
        let wire = to_request(&req);
        assert_eq!(
            wire.generation_config
                .as_ref()
                .and_then(|g| g.thinking_config.as_ref())
                .map(|t| t.thinking_budget),
            Some(16384)
        );
    }

    #[test]
    fn thinking_off_zeroes_budget() {
        let req = request_with(vec![Message::user("hi")], Some(ReasoningEffort::Off));
        let wire = to_request(&req);
        assert_eq!(
            wire.generation_config
                .as_ref()
                .and_then(|g| g.thinking_config.as_ref())
                .map(|t| t.thinking_budget),
            Some(0)
        );
    }

    #[test]
    fn unset_effort_omits_thinking() {
        let req = request_with(vec![Message::user("hi")], None);
        let wire = to_request(&req);
        assert!(
            wire.generation_config
                .as_ref()
                .and_then(|g| g.thinking_config.as_ref())
                .is_none()
        );
    }

    #[test]
    fn parses_non_stream_response() {
        let json = r#"{
            "candidates": [{
                "content": {"role": "model", "parts": [{"text": "hello "}, {"text": "world"}]}
            }],
            "usageMetadata": {"promptTokenCount": 5, "candidatesTokenCount": 7, "totalTokenCount": 12}
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
        let delta = r#"{"candidates":[{"content":{"parts":[{"text":"hi"}]}}]}"#;
        assert!(matches!(
            parse_event(delta).unwrap(),
            Some(ChatChunk::Delta { ref content }) if content == "hi"
        ));
        assert!(matches!(
            parse_event("[DONE]").unwrap(),
            Some(ChatChunk::Done)
        ));
        // empty candidate chunks are skipped
        assert!(parse_event(r#"{"candidates":[]}"#).unwrap().is_none());
    }
}
