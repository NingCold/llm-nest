//! Wire types and conversions for the OpenAI Responses API.

use common::{ContentPart, Message, Role, ToolCall, Usage};
use serde::{Deserialize, Serialize};

use crate::error::{AiError, Result};
use crate::reasoning::{ReasoningEffort, ReasoningFormat};
use crate::request::ChatRequest;
use crate::response::ProviderResponse;

/// `reasoning: { effort }` — Responses API reasoning dispatch.
#[derive(Debug, Clone, Serialize)]
pub struct Reasoning {
    pub effort: String,
}

/// Responses API tool declaration — flat (unlike chat completions' nested
/// `function` object): `{ type: "function", name, description, parameters }`.
#[derive(Debug, Clone, Serialize)]
pub struct ToolDecl {
    #[serde(rename = "type")]
    pub typ: String,
    pub name: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub description: String,
    #[serde(skip_serializing_if = "serde_json::Value::is_null")]
    pub parameters: serde_json::Value,
}

#[derive(Debug, Clone, Serialize)]
pub struct Request {
    pub model: String,
    /// Wire-form input items (role + content blocks; tool calls as
    /// `function_call` / `function_call_output` items — see [`wire_input`]).
    pub input: Vec<serde_json::Value>,
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
    /// Tool declarations; only sent when non-empty.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<ToolDecl>,
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
    /// `function_call` items.
    pub call_id: Option<String>,
    pub name: Option<String>,
    pub arguments: Option<String>,
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
    #[serde(default)]
    pub input_tokens_details: Option<InputTokensDetails>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct InputTokensDetails {
    #[serde(default)]
    pub cached_tokens: Option<u32>,
}

impl From<ResponseUsage> for Usage {
    fn from(u: ResponseUsage) -> Self {
        Usage {
            prompt_tokens: u.input_tokens.unwrap_or(0),
            completion_tokens: u.output_tokens.unwrap_or(0),
            total_tokens: u.total_tokens.unwrap_or(0),
            cached_tokens: u
                .input_tokens_details
                .and_then(|d| d.cached_tokens)
                .unwrap_or(0),
        }
    }
}

/// Build the wire request from a routed chat request.
pub fn to_request(req: &ChatRequest) -> Request {
    Request {
        model: req.wire_model().to_string(),
        input: req.messages.iter().flat_map(wire_input).collect(),
        temperature: req.options.temperature,
        max_output_tokens: req.options.max_tokens,
        top_p: req.options.top_p,
        stream: req.options.stream,
        reasoning: reasoning(req),
        tools: req
            .tools
            .iter()
            .map(|t| ToolDecl {
                typ: "function".into(),
                name: t.name.clone(),
                description: t.description.clone(),
                parameters: t.parameters.clone(),
            })
            .collect(),
    }
}

/// Wire form of one message, possibly expanding to several input items:
/// tool results become `function_call_output` items, assistant messages that
/// carried tool calls become an assistant text item (when non-empty) plus one
/// `function_call` item per call; everything else stays in the plain
/// role + content form.
fn wire_input(message: &Message) -> Vec<serde_json::Value> {
    if message.role == Role::Tool {
        let result = message.content.iter().find_map(|part| match part {
            ContentPart::ToolResult(tr) => Some(tr),
            _ => None,
        });
        return vec![serde_json::json!({
            "type": "function_call_output",
            "call_id": result.map(|r| r.id.as_str()).unwrap_or(""),
            "output": result.map(|r| r.content.as_str()).unwrap_or(""),
        })];
    }

    let calls: Vec<serde_json::Value> = message
        .content
        .iter()
        .filter_map(|part| match part {
            ContentPart::ToolCall(tc) => Some(serde_json::json!({
                "type": "function_call",
                "call_id": tc.id,
                "name": tc.name,
                "arguments": tc.arguments,
            })),
            _ => None,
        })
        .collect();
    if calls.is_empty() {
        return vec![message.to_wire_value()];
    }

    let mut items = Vec::new();
    // Text rides in a plain assistant item (never alongside the raw
    // tool_call blocks); each call becomes its own function_call item.
    let text = message.text();
    if !text.is_empty() {
        items.push(serde_json::json!({ "role": "assistant", "content": text }));
    }
    items.extend(calls);
    items
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
/// blocks of message items and map `function_call` items to tool-call content
/// parts (arguments are a JSON string on this wire), remap usage token names.
pub fn to_provider_response(resp: Response) -> Result<ProviderResponse> {
    let mut parts: Vec<ContentPart> = Vec::new();
    for item in resp.output {
        match item.typ.as_str() {
            "message" => {
                if let Some(blocks) = item.content {
                    for block in blocks {
                        if block.typ == "output_text" {
                            if let Some(t) = block.text {
                                parts.push(ContentPart::Text(t));
                            }
                        }
                    }
                }
            }
            "function_call" => {
                if let (Some(call_id), Some(name)) = (item.call_id, item.name) {
                    parts.push(ContentPart::ToolCall(ToolCall {
                        id: call_id,
                        name,
                        arguments: item.arguments.unwrap_or_default(),
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
/// merges the function-call events into [`ChatChunk::ToolCall`]).
#[derive(Debug, Clone)]
pub enum StreamEvent {
    /// `response.output_text.delta`.
    Delta { content: String },
    /// `response.output_item.added` with a `function_call` item.
    FunctionCallStart {
        output_index: u32,
        call_id: String,
        name: String,
        arguments: String,
    },
    /// `response.function_call_arguments.delta` (arguments arrive as string
    /// fragments).
    FunctionCallDelta { output_index: u32, delta: String },
    /// `response.function_call_arguments.done` (complete arguments string).
    FunctionCallDone {
        output_index: u32,
        arguments: String,
    },
    /// `response.output_item.done` with the complete `function_call` item.
    FunctionCallFlush {
        output_index: u32,
        call_id: String,
        name: String,
        arguments: String,
    },
    /// Successful `response.completed` /
    /// `[DONE]` — stream end.
    Done { usage: Option<Usage> },
}

/// One SSE payload → optional stream event. `[DONE]` and the terminal
/// response events close the stream; an `error` event surfaces as a stream
/// error; irrelevant events are skipped.
pub fn parse_event(data: &str) -> Result<Option<StreamEvent>> {
    if data.trim() == "[DONE]" {
        return Ok(Some(StreamEvent::Done { usage: None }));
    }
    #[derive(Deserialize)]
    struct Event {
        #[serde(rename = "type")]
        typ: String,
        delta: Option<String>,
        error: Option<EventError>,
        #[serde(default)]
        usage: Option<ResponseUsage>,
        /// Some proxies (e.g. chatecnu) nest usage inside the `response`
        /// object instead of at the event top level.
        #[serde(default)]
        response: Option<ResponseSummary>,
        #[serde(default)]
        output_index: Option<u32>,
        #[serde(default)]
        item: Option<Item>,
        #[serde(default)]
        arguments: Option<String>,
    }
    #[derive(Deserialize)]
    struct ResponseSummary {
        error: Option<EventError>,
        incomplete_details: Option<serde_json::Value>,
        #[serde(default)]
        usage: Option<ResponseUsage>,
    }
    #[derive(Deserialize)]
    struct Item {
        #[serde(rename = "type")]
        typ: String,
        #[serde(default)]
        call_id: Option<String>,
        #[serde(default)]
        name: Option<String>,
        #[serde(default)]
        arguments: Option<String>,
    }
    #[derive(Deserialize)]
    struct EventError {
        message: Option<String>,
    }
    let event: Event = serde_json::from_str(data)?;
    match event.typ.as_str() {
        "response.output_text.delta" => Ok(event
            .delta
            .filter(|d| !d.is_empty())
            .map(|d| StreamEvent::Delta { content: d })),
        "response.output_item.added" => {
            let item = match event.item {
                Some(i) if i.typ == "function_call" => i,
                _ => return Ok(None),
            };
            let (Some(call_id), Some(name)) = (item.call_id, item.name) else {
                return Ok(None);
            };
            Ok(Some(StreamEvent::FunctionCallStart {
                output_index: event.output_index.unwrap_or(0),
                call_id,
                name,
                arguments: item.arguments.unwrap_or_default(),
            }))
        }
        "response.function_call_arguments.delta" => {
            Ok(event.delta.map(|d| StreamEvent::FunctionCallDelta {
                output_index: event.output_index.unwrap_or(0),
                delta: d,
            }))
        }
        "response.function_call_arguments.done" => {
            Ok(event.arguments.map(|a| StreamEvent::FunctionCallDone {
                output_index: event.output_index.unwrap_or(0),
                arguments: a,
            }))
        }
        "response.output_item.done" => {
            let item = match event.item {
                Some(i) if i.typ == "function_call" => i,
                _ => return Ok(None),
            };
            let (Some(call_id), Some(name)) = (item.call_id, item.name) else {
                return Ok(None);
            };
            Ok(Some(StreamEvent::FunctionCallFlush {
                output_index: event.output_index.unwrap_or(0),
                call_id,
                name,
                arguments: item.arguments.unwrap_or_default(),
            }))
        }
        "response.failed" | "response.incomplete" => Err(AiError::StreamError(format!(
            "{}: {}",
            event.typ,
            event
                .response
                .and_then(|r| r
                    .error
                    .and_then(|e| e.message)
                    .or_else(|| r.incomplete_details.map(|d| d.to_string())))
                .unwrap_or_else(|| "response did not complete".into())
        ))),
        "response.completed" => {
            let usage = event.usage.or_else(|| event.response.and_then(|r| r.usage));
            Ok(Some(StreamEvent::Done {
                usage: usage.map(Usage::from),
            }))
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
            tools: Vec::new(),
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
            Some(StreamEvent::Delta { ref content }) if content == "hi"
        ));
        assert!(matches!(
            parse_event(r#"{"type":"response.completed"}"#).unwrap(),
            Some(StreamEvent::Done { usage: _ })
        ));
        assert!(parse_event(r#"[DONE]"#).unwrap().is_some());
        // empty text deltas are skipped
        assert!(
            parse_event(r#"{"type":"response.output_text.delta","delta":""}"#)
                .unwrap()
                .is_none()
        );
        // irrelevant events are skipped
        assert!(
            parse_event(r#"{"type":"response.created"}"#)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn completed_event_carries_cached_usage() {
        let json = r#"{"type":"response.completed","usage":{"input_tokens":100,"output_tokens":50,"total_tokens":150,"input_tokens_details":{"cached_tokens":40}}}"#;
        match parse_event(json).unwrap() {
            Some(StreamEvent::Done { usage: Some(u) }) => {
                assert_eq!(u.prompt_tokens, 100);
                assert_eq!(u.completion_tokens, 50);
                assert_eq!(u.cached_tokens, 40);
            }
            other => panic!("expected Done with usage, got {other:?}"),
        }
    }

    #[test]
    fn parses_function_call_stream_events() {
        let added = r#"{"type":"response.output_item.added","output_index":1,"item":{"id":"fc_1","type":"function_call","status":"in_progress","call_id":"call_1","name":"add","arguments":""}}"#;
        match parse_event(added).unwrap() {
            Some(StreamEvent::FunctionCallStart {
                output_index,
                call_id,
                name,
                ..
            }) => {
                assert_eq!(output_index, 1);
                assert_eq!(call_id, "call_1");
                assert_eq!(name, "add");
            }
            other => panic!("expected FunctionCallStart, got {other:?}"),
        }
        let delta = r#"{"type":"response.function_call_arguments.delta","output_index":1,"delta":"{\"a\":"}"#;
        match parse_event(delta).unwrap() {
            Some(StreamEvent::FunctionCallDelta {
                output_index,
                delta,
            }) => {
                assert_eq!(output_index, 1);
                assert_eq!(delta, r#"{"a":"#);
            }
            other => panic!("expected FunctionCallDelta, got {other:?}"),
        }
        let arg_done = r#"{"type":"response.function_call_arguments.done","output_index":1,"arguments":"{\"a\":6}"}"#;
        assert!(matches!(
            parse_event(arg_done).unwrap(),
            Some(StreamEvent::FunctionCallDone {
                output_index: 1,
                ..
            })
        ));
        let item_done = r#"{"type":"response.output_item.done","output_index":1,"item":{"id":"fc_1","type":"function_call","status":"completed","call_id":"call_1","name":"add","arguments":"{\"a\":6}"}}"#;
        match parse_event(item_done).unwrap() {
            Some(StreamEvent::FunctionCallFlush {
                output_index,
                call_id,
                name,
                arguments,
            }) => {
                assert_eq!(output_index, 1);
                assert_eq!(call_id, "call_1");
                assert_eq!(name, "add");
                assert_eq!(arguments, r#"{"a":6}"#);
            }
            other => panic!("expected FunctionCallFlush, got {other:?}"),
        }
        // non-function items are skipped
        let msg_added = r#"{"type":"response.output_item.added","output_index":0,"item":{"id":"m_1","type":"message","role":"assistant","content":[{"type":"output_text","text":"","annotations":[]}]}}"#;
        assert!(parse_event(msg_added).unwrap().is_none());
    }

    #[test]
    fn wire_declares_tools() {
        let mut req = ChatRequest {
            selection: crate::request::ModelSelection {
                provider: "openai".into(),
                model: "test-model".into(),
                reasoning_effort: None,
            },
            messages: vec![Message::user("hi")],
            options: GenerationOptions::default(),
            tools: vec![common::ToolDefinition {
                name: "add".into(),
                description: "add two numbers".into(),
                parameters: serde_json::json!({"type":"object"}),
            }],
            resolved: None,
        };
        let wire = to_request(&req);
        assert_eq!(wire.tools.len(), 1);
        assert_eq!(wire.tools[0].typ, "function");
        assert_eq!(wire.tools[0].name, "add");
        assert_eq!(wire.tools[0].parameters["type"], "object");
        req.tools = Vec::new();
        assert!(to_request(&req).tools.is_empty());
    }

    #[test]
    fn wire_maps_tool_calls_and_results() {
        let req = ChatRequest {
            selection: crate::request::ModelSelection {
                provider: "openai".into(),
                model: "test-model".into(),
                reasoning_effort: None,
            },
            messages: vec![
                Message::user("6 + 4?"),
                Message {
                    role: Role::Assistant,
                    content: vec![
                        ContentPart::Text("calculating".into()),
                        ContentPart::ToolCall(common::ToolCall {
                            id: "call_1".into(),
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
                        id: "call_1".into(),
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
                    interruption: None,
                    id: Some(common::MessageId::new()),
                },
            ],
            options: GenerationOptions::default(),
            tools: Vec::new(),
            resolved: None,
        };
        let wire = to_request(&req);
        assert_eq!(wire.input.len(), 4);
        // assistant: text item + function_call item
        assert_eq!(wire.input[1]["role"], "assistant");
        assert_eq!(wire.input[1]["content"], "calculating");
        assert_eq!(wire.input[2]["type"], "function_call");
        assert_eq!(wire.input[2]["call_id"], "call_1");
        assert_eq!(wire.input[2]["name"], "add");
        assert_eq!(wire.input[2]["arguments"], r#"{"a":6,"b":4}"#);
        // tool result → function_call_output
        assert_eq!(wire.input[3]["type"], "function_call_output");
        assert_eq!(wire.input[3]["call_id"], "call_1");
        assert_eq!(wire.input[3]["output"], "10");
    }

    #[test]
    fn non_stream_response_maps_function_call() {
        let json = r#"{
            "id": "resp_1",
            "output": [
                {"type": "message", "role": "assistant", "content": [
                    {"type": "output_text", "text": "calling "}
                ]},
                {"type": "function_call", "call_id": "call_9", "name": "add", "arguments": "{\"a\":1,\"b\":2}"}
            ],
            "usage": {"input_tokens": 5, "output_tokens": 7, "total_tokens": 12}
        }"#;
        let resp: Response = serde_json::from_str(json).unwrap();
        let provider = to_provider_response(resp).unwrap();
        assert_eq!(provider.message.text(), "calling ");
        assert_eq!(provider.message.content.len(), 2);
        match &provider.message.content[1] {
            ContentPart::ToolCall(tc) => {
                assert_eq!(tc.id, "call_9");
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
