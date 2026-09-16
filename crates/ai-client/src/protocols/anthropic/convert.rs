//! Wire types and conversions for the Anthropic Messages API.

use common::{ContentPart, Message, Role, ToolCall, Usage};
use serde::{Deserialize, Serialize};

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

/// Anthropic tool declaration: `{ name, description, input_schema }`.
#[derive(Debug, Clone, Serialize)]
pub struct ToolDecl {
    pub name: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub description: String,
    #[serde(rename = "input_schema")]
    pub input_schema: serde_json::Value,
}

#[derive(Debug, Clone, Serialize)]
pub struct Request {
    pub model: String,
    /// Mandatory on the wire; sourced from options, then the model's declared
    /// output capability, then a default.
    pub max_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system: Option<String>,
    /// Wire-form user/assistant turns (role + content via [`wire_turn`];
    /// tool results ride as user messages with `tool_result` blocks).
    pub messages: Vec<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f32>,
    #[serde(default)]
    pub stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking: Option<Thinking>,
    /// Tool declarations; only sent when non-empty.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<ToolDecl>,
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
    /// `tool_use` blocks: id / name / input (arguments object).
    pub id: Option<String>,
    pub name: Option<String>,
    pub input: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ResponseUsage {
    pub input_tokens: Option<u32>,
    pub output_tokens: Option<u32>,
    #[serde(default)]
    pub cache_creation_input_tokens: Option<u32>,
    #[serde(default)]
    pub cache_read_input_tokens: Option<u32>,
}

impl From<ResponseUsage> for Usage {
    fn from(u: ResponseUsage) -> Self {
        let input = u.input_tokens.unwrap_or(0)
            + u.cache_read_input_tokens.unwrap_or(0)
            + u.cache_creation_input_tokens.unwrap_or(0);
        let output = u.output_tokens.unwrap_or(0);
        Usage {
            prompt_tokens: input,
            completion_tokens: output,
            total_tokens: input + output,
            cached_tokens: u.cache_read_input_tokens.unwrap_or(0),
        }
    }
}

/// `max_tokens` fallback when neither the request options nor the model's
/// declared capability provide one.
pub const DEFAULT_MAX_TOKENS: u32 = 4096;

/// Build the wire request from a routed chat request.
pub fn to_request(req: &ChatRequest) -> Request {
    let (system, messages) = split_system(&req.messages);
    Request {
        model: req.wire_model().to_string(),
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
        tools: req
            .tools
            .iter()
            .map(|t| ToolDecl {
                name: t.name.clone(),
                description: t.description.clone(),
                input_schema: t.parameters.clone(),
            })
            .collect(),
    }
}

/// Anthropic has no system/developer/tool message roles: system and developer
/// prompts fold into the top-level `system` field, tool results are carried
/// as user messages with `tool_result` blocks, and the messages array keeps
/// only user/assistant turns — the two roles the API accepts.
fn split_system(messages: &[Message]) -> (String, Vec<serde_json::Value>) {
    let mut system = Vec::new();
    let mut turns = Vec::new();
    for message in messages {
        match message.role {
            Role::System | Role::Developer => system.push(message.text()),
            Role::User => turns.push(message.to_wire_value()),
            Role::Assistant => turns.push(wire_turn(message)),
            Role::Tool => turns.push(wire_turn(message)),
        }
    }
    (system.join("\n\n"), turns)
}

/// Anthropic wire form of one message. Tool results become user-role
/// `tool_result` content blocks (Anthropic has no tool role; a result message
/// must contain only that block); assistant messages carrying tool calls get
/// `tool_use` blocks whose `input` is the parsed arguments object.
fn wire_turn(message: &Message) -> serde_json::Value {
    let calls: Vec<serde_json::Value> = message
        .content
        .iter()
        .filter_map(|part| match part {
            ContentPart::ToolCall(tc) => Some(serde_json::json!({
                "type": "tool_use",
                "id": tc.id,
                "name": tc.name,
                "input": serde_json::from_str(&tc.arguments)
                    .unwrap_or_else(|_| serde_json::json!({})),
            })),
            _ => None,
        })
        .collect();

    if message.role == Role::Tool {
        let result = message.content.iter().find_map(|part| match part {
            ContentPart::ToolResult(tr) => Some(tr),
            _ => None,
        });
        let mut block = serde_json::json!({
            "type": "tool_result",
            "tool_use_id": result.map(|r| r.id.as_str()).unwrap_or(""),
            "content": result.map(|r| r.content.as_str()).unwrap_or(""),
        });
        if result.is_some_and(|r| r.is_error) {
            block["is_error"] = serde_json::Value::Bool(true);
        }
        return serde_json::json!({ "role": "user", "content": vec![block] });
    }

    if calls.is_empty() {
        return message.to_wire_value();
    }

    let mut blocks: Vec<serde_json::Value> = message
        .content
        .iter()
        .filter_map(|part| match part {
            ContentPart::Text(t) if !t.is_empty() => {
                Some(serde_json::json!({ "type": "text", "text": t }))
            }
            _ => None,
        })
        .collect();
    blocks.extend(calls);
    serde_json::json!({ "role": "assistant", "content": blocks })
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

/// Non-streaming response → provider response: concatenate `text` blocks and
/// map `tool_use` blocks to tool-call content parts, remap usage token names
/// (Anthropic has no `total_tokens`; derive it).
pub fn to_provider_response(resp: Response) -> Result<ProviderResponse> {
    let mut parts: Vec<ContentPart> = Vec::new();
    for block in resp.content {
        match block.typ.as_str() {
            "text" => {
                if let Some(t) = block.text {
                    parts.push(ContentPart::Text(t));
                }
            }
            "tool_use" => {
                if let (Some(id), Some(name)) = (block.id, block.name) {
                    parts.push(ContentPart::ToolCall(ToolCall {
                        id,
                        name,
                        arguments: block
                            .input
                            .unwrap_or_else(|| serde_json::json!({}))
                            .to_string(),
                        thought_signature: None,
                    }));
                }
            }
            _ => {}
        }
    }
    let usage = resp.usage.map(Usage::from);
    Ok(ProviderResponse {
        message: Message::new(Role::Assistant, parts),
        reasoning: None,
        usage,
    })
}

/// One parsed SSE payload, before tool-call assembly ([`super::stream`]
/// merges `ToolUseStart`/`ToolUseDelta`/`ToolUseStop` into
/// [`ChatChunk::ToolCall`]).
#[derive(Debug, Clone)]
pub enum StreamEvent {
    /// `content_block_delta` text fragment.
    Delta { content: String },
    /// `content_block_start` carrying a `tool_use` block.
    ToolUseStart {
        index: u32,
        id: String,
        name: String,
    },
    /// `input_json_delta` fragment of the tool arguments.
    ToolUseDelta { index: u32, partial_json: String },
    /// `content_block_stop`: the tool block at `index` is complete.
    ToolUseStop { index: u32 },
    /// `message_delta` (carries final usage) / `message_stop`.
    Done { usage: Option<Usage> },
}

/// One SSE payload → optional stream event. `content_block_delta` text
/// deltas stream content; `message_delta` carries partial usage fields.
/// AnthropicStream merges them and waits for `message_stop`; an
/// `error` event surfaces as a stream error. `thinking_delta` /
/// `signature_delta` blocks are skipped (thinking chain is not surfaced).
pub fn parse_event(data: &str) -> Result<Option<StreamEvent>> {
    #[derive(Deserialize)]
    struct Event {
        #[serde(rename = "type")]
        typ: String,
        index: Option<u32>,
        content_block: Option<ContentBlock>,
        delta: Option<Delta>,
        error: Option<EventError>,
        #[serde(default)]
        usage: Option<ResponseUsage>,
    }
    #[derive(Deserialize)]
    struct Delta {
        /// Optional: `message_delta`'s delta object (stop_reason/sequence)
        /// has no type field.
        #[serde(rename = "type", default)]
        typ: Option<String>,
        text: Option<String>,
        #[serde(default)]
        partial_json: Option<String>,
    }
    #[derive(Deserialize)]
    struct EventError {
        message: Option<String>,
    }
    let event: Event = serde_json::from_str(data)?;
    match event.typ.as_str() {
        "content_block_start" => {
            let block = match event.content_block {
                Some(b) if b.typ == "tool_use" => b,
                _ => return Ok(None),
            };
            let (Some(id), Some(name)) = (block.id, block.name) else {
                return Ok(None);
            };
            Ok(Some(StreamEvent::ToolUseStart {
                index: event.index.unwrap_or(0),
                id,
                name,
            }))
        }
        "content_block_delta" => match event.delta {
            Some(delta) if delta.typ.as_deref() == Some("text_delta") => Ok(delta
                .text
                .filter(|t| !t.is_empty())
                .map(|t| StreamEvent::Delta { content: t })),
            Some(delta) if delta.typ.as_deref() == Some("input_json_delta") => {
                Ok(delta.partial_json.map(|json| StreamEvent::ToolUseDelta {
                    index: event.index.unwrap_or(0),
                    partial_json: json,
                }))
            }
            _ => Ok(None),
        },
        "content_block_stop" => Ok(Some(StreamEvent::ToolUseStop {
            index: event.index.unwrap_or(0),
        })),
        // AnthropicStream intercepts this update, merges usage, and waits for message_stop.
        "message_delta" => Ok(Some(StreamEvent::Done {
            usage: event.usage.map(Usage::from),
        })),
        "message_stop" => Ok(Some(StreamEvent::Done { usage: None })),
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
            tools: Vec::new(),
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
            .map(|m| match m.get("role").and_then(|r| r.as_str()) {
                Some("user") => "user",
                Some("assistant") => "assistant",
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
            Some(StreamEvent::Delta { ref content }) if content == "hi"
        ));
        assert!(matches!(
            parse_event(r#"{"type":"message_stop"}"#).unwrap(),
            Some(StreamEvent::Done { usage: _ })
        ));
        // signature_delta / thinking_delta blocks are skipped
        let thinking =
            r#"{"type":"content_block_delta","delta":{"type":"thinking_delta","thinking":"..."}}"#;
        assert!(parse_event(thinking).unwrap().is_none());
    }

    #[test]
    fn message_delta_carries_cache_usage() {
        let json = r#"{"type":"message_delta","usage":{"input_tokens":200,"output_tokens":60,"cache_creation_input_tokens":50,"cache_read_input_tokens":30}}"#;
        match parse_event(json).unwrap() {
            Some(StreamEvent::Done { usage: Some(u) }) => {
                assert_eq!(u.prompt_tokens, 280);
                assert_eq!(u.completion_tokens, 60);
                assert_eq!(u.total_tokens, 340);
                // cached = cache_read + cache_creation
                assert_eq!(u.cached_tokens, 30);
            }
            other => panic!("expected Done with usage, got {other:?}"),
        }
    }

    #[test]
    fn message_delta_with_stop_reason_delta_parses() {
        // Real proxies (e.g. chatecnu's anthropic endpoint) send message_delta
        // with a delta object that has no type field.
        let json = r#"{"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},"usage":{"input_tokens":5,"output_tokens":3}}"#;
        match parse_event(json).unwrap() {
            Some(StreamEvent::Done { usage: Some(u) }) => {
                assert_eq!(u.prompt_tokens, 5);
                assert_eq!(u.completion_tokens, 3);
            }
            other => panic!("expected Done with usage, got {other:?}"),
        }
    }

    #[test]
    fn parses_tool_use_stream_events() {
        let start = r#"{"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"toolu_1","name":"add","input":{}}}"#;
        match parse_event(start).unwrap() {
            Some(StreamEvent::ToolUseStart { index, id, name }) => {
                assert_eq!(index, 1);
                assert_eq!(id, "toolu_1");
                assert_eq!(name, "add");
            }
            other => panic!("expected ToolUseStart, got {other:?}"),
        }
        let delta = r#"{"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"{\"a\":6}"}}"#;
        match parse_event(delta).unwrap() {
            Some(StreamEvent::ToolUseDelta {
                index,
                partial_json,
            }) => {
                assert_eq!(index, 1);
                assert_eq!(partial_json, r#"{"a":6}"#);
            }
            other => panic!("expected ToolUseDelta, got {other:?}"),
        }
        assert!(matches!(
            parse_event(r#"{"type":"content_block_stop","index":1}"#).unwrap(),
            Some(StreamEvent::ToolUseStop { index: 1 })
        ));
        // non-tool content blocks are skipped
        let text_block = r#"{"type":"content_block_start","index":0,"content_block":{"type":"text","text":"hi"}}"#;
        assert!(parse_event(text_block).unwrap().is_none());
    }

    #[test]
    fn wire_declares_tools() {
        let mut req = request_with(vec![Message::user("hi")], None);
        req.tools = vec![common::ToolDefinition {
            name: "add".into(),
            description: "add two numbers".into(),
            parameters: serde_json::json!({"type":"object","properties":{"a":{"type":"number"}}}),
        }];
        let wire = to_request(&req);
        assert_eq!(wire.tools.len(), 1);
        assert_eq!(wire.tools[0].name, "add");
        assert_eq!(
            wire.tools[0].input_schema["properties"]["a"]["type"],
            "number"
        );
        // no tools → field absent
        let wire = to_request(&request_with(vec![Message::user("hi")], None));
        assert!(wire.tools.is_empty());
    }

    #[test]
    fn wire_maps_tool_calls_and_results() {
        let req = request_with(
            vec![
                Message::user("6 + 4?"),
                Message {
                    role: Role::Assistant,
                    content: vec![
                        ContentPart::Text("let me add".into()),
                        ContentPart::ToolCall(common::ToolCall {
                            id: "toolu_1".into(),
                            name: "add".into(),
                            arguments: r#"{"a":6,"b":4}"#.into(),
                            thought_signature: None,
                        }),
                    ],
                    reasoning: None,
                    created_at: None,
                    thinking_ms: None,
                    usage: None,
                    timings: None,
                    feedback: None,
                    interruption: None,
                    id: Some(common::MessageId::new()),
                },
                Message {
                    role: Role::Tool,
                    content: vec![ContentPart::ToolResult(common::ToolResult {
                        id: "toolu_1".into(),
                        name: "add".into(),
                        content: "10".into(),
                        is_error: false,
                        duration_ms: Some(3),
                    })],
                    reasoning: None,
                    created_at: None,
                    thinking_ms: None,
                    usage: None,
                    timings: None,
                    feedback: None,
                    interruption: None,
                    id: Some(common::MessageId::new()),
                },
            ],
            None,
        );
        let wire = to_request(&req);
        assert_eq!(wire.messages.len(), 3);

        // assistant: text + tool_use blocks with parsed input object
        let assistant = &wire.messages[1];
        assert_eq!(assistant["role"], "assistant");
        let blocks = assistant["content"].as_array().unwrap();
        assert_eq!(blocks[0]["type"], "text");
        assert_eq!(blocks[1]["type"], "tool_use");
        assert_eq!(blocks[1]["id"], "toolu_1");
        assert_eq!(blocks[1]["name"], "add");
        assert_eq!(blocks[1]["input"]["a"], 6);

        // tool result → user role with tool_result block
        let tool = &wire.messages[2];
        assert_eq!(tool["role"], "user");
        let tr = &tool["content"][0];
        assert_eq!(tr["type"], "tool_result");
        assert_eq!(tr["tool_use_id"], "toolu_1");
        assert_eq!(tr["content"], "10");
        assert!(tr.get("is_error").is_none());
    }

    #[test]
    fn wire_marks_tool_result_errors() {
        let req = request_with(
            vec![Message {
                role: Role::Tool,
                content: vec![ContentPart::ToolResult(common::ToolResult {
                    id: "toolu_2".into(),
                    name: "add".into(),
                    content: "boom".into(),
                    is_error: true,
                    duration_ms: None,
                })],
                reasoning: None,
                created_at: None,
                thinking_ms: None,
                usage: None,
                timings: None,
                feedback: None,
                interruption: None,
                id: Some(common::MessageId::new()),
            }],
            None,
        );
        let wire = to_request(&req);
        assert_eq!(wire.messages[0]["role"], "user");
        assert_eq!(wire.messages[0]["content"][0]["is_error"], true);
    }

    #[test]
    fn non_stream_response_maps_tool_use() {
        let json = r#"{
            "id": "msg_1",
            "type": "message",
            "role": "assistant",
            "content": [
                {"type": "text", "text": "calling "},
                {"type": "tool_use", "id": "toolu_9", "name": "add", "input": {"a": 1, "b": 2}}
            ],
            "usage": {"input_tokens": 5, "output_tokens": 7}
        }"#;
        let resp: Response = serde_json::from_str(json).unwrap();
        let provider = to_provider_response(resp).unwrap();
        assert_eq!(provider.message.text(), "calling ");
        assert_eq!(provider.message.content.len(), 2);
        match &provider.message.content[1] {
            ContentPart::ToolCall(tc) => {
                assert_eq!(tc.id, "toolu_9");
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
