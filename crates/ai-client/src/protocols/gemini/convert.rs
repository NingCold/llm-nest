//! Wire types and conversions for the Gemini API.

use common::{ContentPart, Message, Role, ToolCall, Usage};
use serde::{Deserialize, Serialize};

use crate::chunk::ChatChunk;
use crate::error::{AiError, Result};
use crate::reasoning::{ReasoningEffort, ReasoningFormat};
use crate::request::ChatRequest;
use crate::response::ProviderResponse;

/// One Gemini content part: text, a model `functionCall`, or a user
/// `functionResponse`. Exactly one is present on the wire.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Part {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub function_call: Option<FunctionCall>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub function_response: Option<FunctionResponse>,
}

impl Part {
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            text: Some(text.into()),
            function_call: None,
            function_response: None,
        }
    }

    pub fn function_call(name: impl Into<String>, args: serde_json::Value) -> Self {
        Self {
            text: None,
            function_call: Some(FunctionCall {
                name: name.into(),
                args,
            }),
            function_response: None,
        }
    }

    pub fn function_response(name: impl Into<String>, response: serde_json::Value) -> Self {
        Self {
            text: None,
            function_call: None,
            function_response: Some(FunctionResponse {
                name: name.into(),
                response,
            }),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FunctionCall {
    pub name: String,
    /// Arguments object; always an object on the wire.
    pub args: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FunctionResponse {
    pub name: String,
    /// Structured result object.
    pub response: serde_json::Value,
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

/// Gemini has no tool-call ids on the wire; llm-nest bookkeeping still needs
/// one per call (persisted ToolCall/ToolResult pairing). Synthesized ids are
/// never sent to the API — the pairing is positional by name/order.
static GEMINI_CALL_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

fn gemini_call_id() -> String {
    format!(
        "gemini_{}",
        GEMINI_CALL_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    )
}

/// Gemini tool declaration: `{ functionDeclarations: [...] }`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolDecl {
    pub function_declarations: Vec<FunctionDecl>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FunctionDecl {
    pub name: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub description: String,
    #[serde(skip_serializing_if = "serde_json::Value::is_null")]
    pub parameters: serde_json::Value,
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
    /// Tool declarations; only sent when non-empty.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<ToolDecl>,
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
    #[serde(default)]
    pub cached_content_token_count: Option<u32>,
}

impl From<UsageMetadata> for Usage {
    fn from(u: UsageMetadata) -> Self {
        let input = u.prompt_token_count.unwrap_or(0);
        let output = u.candidates_token_count.unwrap_or(0);
        Usage {
            prompt_tokens: input,
            completion_tokens: output,
            total_tokens: u.total_token_count.unwrap_or(input + output),
            cached_tokens: u.cached_content_token_count.unwrap_or(0),
        }
    }
}

/// Build the wire request from a routed chat request.
pub fn to_request(req: &ChatRequest) -> Request {
    let mut system = Vec::new();
    let mut contents = Vec::new();
    for message in &req.messages {
        match message.role {
            // Gemini has no system/tool roles: system and developer prompts
            // fold into `systemInstruction`; tool results ride as their own
            // user-role contents carrying only `functionResponse` parts.
            Role::System | Role::Developer => system.push(message.text()),
            Role::User => contents.push(Content {
                role: "user".into(),
                parts: vec![Part::text(message.text())],
            }),
            Role::Assistant => {
                let mut parts: Vec<Part> = message
                    .content
                    .iter()
                    .filter_map(|part| match part {
                        ContentPart::Text(t) if !t.is_empty() => Some(Part::text(t.clone())),
                        ContentPart::ToolCall(tc) => Some(Part::function_call(
                            &tc.name,
                            serde_json::from_str(&tc.arguments)
                                .unwrap_or_else(|_| serde_json::json!({})),
                        )),
                        _ => None,
                    })
                    .collect();
                if parts.is_empty() {
                    parts.push(Part::text(message.text()));
                }
                contents.push(Content {
                    role: "model".into(),
                    parts,
                });
            }
            Role::Tool => {
                let results: Vec<Part> = message
                    .content
                    .iter()
                    .filter_map(|part| match part {
                        ContentPart::ToolResult(tr) => Some(Part::function_response(
                            &tr.name,
                            serde_json::from_str(&tr.content)
                                .unwrap_or_else(|_| serde_json::json!({ "result": tr.content })),
                        )),
                        _ => None,
                    })
                    .collect();
                if !results.is_empty() {
                    contents.push(Content {
                        role: "user".into(),
                        parts: results,
                    });
                }
            }
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
            parts: vec![Part::text(system.join("\n\n"))],
        }),
        generation_config: Some(generation_config),
        // Gemini expects a single `tools` element whose functionDeclarations
        // list carries ALL declarations.
        tools: (!req.tools.is_empty()).then(|| {
            vec![ToolDecl {
                function_declarations: req
                    .tools
                    .iter()
                    .map(|t| FunctionDecl {
                        name: t.name.clone(),
                        description: t.description.clone(),
                        parameters: t.parameters.clone(),
                    })
                    .collect(),
            }]
        }).unwrap_or_default(),
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
/// candidate's text parts and map `functionCall` parts to tool-call content
/// parts, remap usage token names.
pub fn to_provider_response(resp: Response) -> Result<ProviderResponse> {
    let mut parts: Vec<ContentPart> = Vec::new();
    if let Some(content) = resp.candidates.into_iter().next().and_then(|c| c.content) {
        for part in content.parts {
            if let Some(text) = part.text {
                parts.push(ContentPart::Text(text));
            }
            if let Some(fc) = part.function_call {
                parts.push(ContentPart::ToolCall(ToolCall {
                    id: gemini_call_id(),
                    name: fc.name,
                    arguments: fc.args.to_string(),
                }));
            }
        }
    }
    let usage = resp.usage_metadata.map(Usage::from);
    Ok(ProviderResponse {
        message: Message::new(Role::Assistant, parts),
        reasoning: None,
        usage,
    })
}

/// One SSE payload → chunks. `[DONE]` closes the stream; candidate content
/// parts stream text deltas and `functionCall` parts (complete objects, one
/// chunk each in practice); the final chunk carries `usageMetadata` and
/// closes the stream; an `error` object surfaces as a stream error.
pub fn parse_event(data: &str) -> Result<Vec<ChatChunk>> {
    if data.trim() == "[DONE]" {
        return Ok(vec![ChatChunk::Done { usage: None }]);
    }
    #[derive(Deserialize)]
    struct Event {
        candidates: Option<Vec<Candidate>>,
        error: Option<EventError>,
        #[serde(rename = "usageMetadata", default)]
        usage_metadata: Option<UsageMetadata>,
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
    let mut chunks = Vec::new();
    let mut text = String::new();
    if let Some(candidate) = event.candidates.and_then(|mut c| c.drain(..).next())
        && let Some(content) = candidate.content
    {
        for part in content.parts {
            if let Some(t) = part.text {
                text.push_str(&t);
            }
            if let Some(fc) = part.function_call {
                chunks.push(ChatChunk::ToolCall {
                    id: gemini_call_id(),
                    name: fc.name,
                    arguments: fc.args.to_string(),
                });
            }
        }
    }
    if !text.is_empty() {
        chunks.push(ChatChunk::Delta { content: text });
    }
    // Terminal chunk: usageMetadata (possibly alongside final content).
    if let Some(usage) = event.usage_metadata {
        chunks.push(ChatChunk::Done {
            usage: Some(Usage::from(usage)),
        });
    }
    Ok(chunks)
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
            tools: Vec::new(),
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
            wire.system_instruction.unwrap().parts[0].text.as_deref(),
            Some("Be helpful.")
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
        let chunks = parse_event(delta).unwrap();
        assert!(matches!(&chunks[0], ChatChunk::Delta { content } if content == "hi"));
        assert!(matches!(
            parse_event("[DONE]").unwrap()[0],
            ChatChunk::Done { usage: _ }
        ));
        // empty candidate chunks are skipped
        assert!(parse_event(r#"{"candidates":[]}"#).unwrap().is_empty());
    }

    #[test]
    fn usage_metadata_chunk_closes_with_cached_tokens() {
        let json = r#"{"candidates":[],"usageMetadata":{"promptTokenCount":90,"candidatesTokenCount":30,"totalTokenCount":120,"cachedContentTokenCount":25}}"#;
        let chunks = parse_event(json).unwrap();
        assert_eq!(chunks.len(), 1);
        match &chunks[0] {
            ChatChunk::Done { usage: Some(u) } => {
                assert_eq!(u.prompt_tokens, 90);
                assert_eq!(u.completion_tokens, 30);
                assert_eq!(u.total_tokens, 120);
                assert_eq!(u.cached_tokens, 25);
            }
            other => panic!("expected Done with usage, got {other:?}"),
        }
    }

    #[test]
    fn stream_parse_function_call_parts() {
        let json = r#"{"candidates":[{"content":{"parts":[{"functionCall":{"name":"add","args":{"a":6,"b":4}}}]}}]}"#;
        let chunks = parse_event(json).unwrap();
        assert_eq!(chunks.len(), 1);
        match &chunks[0] {
            ChatChunk::ToolCall { id, name, arguments } => {
                assert!(id.starts_with("gemini_"));
                assert_eq!(name, "add");
                assert_eq!(arguments, r#"{"a":6,"b":4}"#);
            }
            other => panic!("expected ToolCall, got {other:?}"),
        }

        // parallel calls in one chunk → one ToolCall per part
        let json = r#"{"candidates":[{"content":{"parts":[{"functionCall":{"name":"a","args":{}}},{"functionCall":{"name":"b","args":{}}}]}}]}"#;
        let chunks = parse_event(json).unwrap();
        assert_eq!(chunks.len(), 2);
        assert!(matches!(&chunks[1], ChatChunk::ToolCall { name, .. } if name == "b"));
    }

    #[test]
    fn wire_declares_tools_and_maps_calls_and_results() {
        let mut req = request_with(
            vec![
                Message::user("6 + 4?"),
                Message {
                    role: Role::Assistant,
                    content: vec![
                        ContentPart::Text("calculating".into()),
                        ContentPart::ToolCall(common::ToolCall {
                            id: "gemini_7".into(),
                            name: "add".into(),
                            arguments: r#"{"a":6,"b":4}"#.into(),
                        }),
                    ],
                    reasoning: None,
                    created_at: None,
                    thinking_ms: None,
                    usage: None,
                    timings: None,
                    feedback: None,
                },
                Message {
                    role: Role::Tool,
                    content: vec![ContentPart::ToolResult(common::ToolResult {
                        id: "gemini_7".into(),
                        name: "add".into(),
                        content: "10".into(),
                        is_error: false,
                        duration_ms: None,
                    })],
                    reasoning: None,
                    created_at: None,
                    thinking_ms: None,
                    usage: None,
                    timings: None,
                    feedback: None,
                },
            ],
            None,
        );
        req.tools = vec![
            common::ToolDefinition {
                name: "add".into(),
                description: "add two numbers".into(),
                parameters: serde_json::json!({"type":"object"}),
            },
            common::ToolDefinition {
                name: "echo".into(),
                description: "echo text".into(),
                parameters: serde_json::json!({"type":"object"}),
            },
        ];
        let wire = to_request(&req);

        // tools: ONE element carrying ALL functionDeclarations
        assert_eq!(wire.tools.len(), 1);
        assert_eq!(wire.tools[0].function_declarations.len(), 2);
        let decl = &wire.tools[0].function_declarations[0];
        assert_eq!(decl.name, "add");
        assert_eq!(decl.parameters["type"], "object");

        // contents: user, model(functionCall), user(functionResponse)
        let roles: Vec<&str> = wire.contents.iter().map(|c| c.role.as_str()).collect();
        assert_eq!(roles, vec!["user", "model", "user"]);
        let model_parts = &wire.contents[1].parts;
        assert_eq!(model_parts.len(), 2);
        assert_eq!(model_parts[0].text.as_deref(), Some("calculating"));
        let fc = model_parts[1].function_call.as_ref().unwrap();
        assert_eq!(fc.name, "add");
        assert_eq!(fc.args["a"], 6);
        let fr = wire.contents[2].parts[0]
            .function_response
            .as_ref()
            .unwrap();
        assert_eq!(fr.name, "add");
        // content "10" parses as a JSON number → structured result object
        assert_eq!(fr.response, serde_json::json!(10));
    }

    #[test]
    fn non_stream_maps_function_call() {
        let json = r#"{
            "candidates": [{
                "content": {"role": "model", "parts": [
                    {"text": "calling "},
                    {"functionCall": {"name": "add", "args": {"a": 1, "b": 2}}}
                ]}
            }],
            "usageMetadata": {"promptTokenCount": 5, "candidatesTokenCount": 7, "totalTokenCount": 12}
        }"#;
        let resp: Response = serde_json::from_str(json).unwrap();
        let provider = to_provider_response(resp).unwrap();
        assert_eq!(provider.message.text(), "calling ");
        assert_eq!(provider.message.content.len(), 2);
        match &provider.message.content[1] {
            ContentPart::ToolCall(tc) => {
                assert!(tc.id.starts_with("gemini_"));
                assert_eq!(tc.name, "add");
                assert_eq!(tc.arguments, r#"{"a":1,"b":2}"#);
            }
            other => panic!("expected ToolCall, got {other:?}"),
        }
        let usage = provider.usage.unwrap();
        assert_eq!(usage.prompt_tokens, 5);
        assert_eq!(usage.completion_tokens, 7);
        assert_eq!(usage.total_tokens, 12);
    }
}
