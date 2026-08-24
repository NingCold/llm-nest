use super::chat;
use super::chat::{FunctionDecl, Request, ThinkingParam, ToolDecl};
use crate::error::{AiError, Result};
use crate::reasoning::{ReasoningEffort, ReasoningFormat};
use crate::request::ChatRequest;
use crate::response::ProviderResponse;
use common::{ContentPart, Message, Role};

pub fn to_real_request(req: &ChatRequest) -> Request {
    let mut request = Request {
        model: req.selection.model.clone(),
        messages: req.messages.iter().map(wire_message).collect(),
        temperature: req.options.temperature,
        max_tokens: req.options.max_tokens,
        top_p: req.options.top_p,
        stream: req.options.stream,
        reasoning_effort: None,
        thinking: None,
        tools: req
            .tools
            .iter()
            .map(|t| ToolDecl {
                typ: "function".into(),
                function: FunctionDecl {
                    name: t.name.clone(),
                    description: t.description.clone(),
                    parameters: t.parameters.clone(),
                },
            })
            .collect(),
    };
    apply_reasoning(req, &mut request);
    request
}

/// Wire form of one message. Tool results are OpenAI `role: "tool"`
/// messages (`content` + `tool_call_id`); assistant messages that carried
/// tool calls get the OpenAI `tool_calls` top-level array (with text content
/// flattened to a string); everything else uses the shared wire form.
fn wire_message(m: &Message) -> serde_json::Value {
    if m.role == Role::Tool {
        let result = m.content.iter().find_map(|part| match part {
            ContentPart::ToolResult(tr) => Some(tr),
            _ => None,
        });
        return serde_json::json!({
            "role": "tool",
            "content": result.map(|r| r.content.as_str()).unwrap_or(""),
            "tool_call_id": result.map(|r| r.id.as_str()).unwrap_or(""),
        });
    }

    let calls: Vec<serde_json::Value> = m
        .content
        .iter()
        .filter_map(|part| match part {
            ContentPart::ToolCall(tc) => Some(serde_json::json!({
                "id": tc.id,
                "type": "function",
                "function": { "name": tc.name, "arguments": tc.arguments }
            })),
            _ => None,
        })
        .collect();
    if calls.is_empty() {
        return m.to_wire_value();
    }

    // OpenAI assistant messages with tool calls: string content + tool_calls.
    serde_json::json!({
        "role": "assistant",
        "content": m.text(),
        "tool_calls": calls,
    })
}

/// Map the routed reasoning effort to this protocol's wire spelling. The
/// resolved spec carries the model's format and validated levels; `Off` is
/// expressed only where the format has an explicit off switch (DeepSeek
/// thinking), otherwise it is a no-op. Without a resolved spec (legacy
/// quickstart path) nothing is emitted.
fn apply_reasoning(req: &ChatRequest, request: &mut Request) {
    let Some(resolved) = &req.resolved else {
        return;
    };
    let Some(capability) = &resolved.spec.reasoning else {
        return;
    };
    let Some(effort) = req.selection.reasoning_effort else {
        return;
    };
    match effort {
        ReasoningEffort::Off => {
            if matches!(
                capability.format,
                ReasoningFormat::DeepSeekThinking | ReasoningFormat::DeepSeekEffort
            ) {
                request.thinking = Some(ThinkingParam {
                    typ: "disabled".into(),
                });
            }
        }
        level => match capability.format {
            ReasoningFormat::OpenAIEffort => {
                request.reasoning_effort = Some(level.as_wire().to_string());
            }
            ReasoningFormat::DeepSeekThinking => {
                request.thinking = Some(ThinkingParam {
                    typ: "enabled".into(),
                });
            }
            // ECNU ecnu-max style: thinking switch + reasoning_effort strength.
            ReasoningFormat::DeepSeekEffort => {
                request.thinking = Some(ThinkingParam {
                    typ: "enabled".into(),
                });
                request.reasoning_effort = Some(level.as_wire().to_string());
            }
            // Gemini thinking dispatches through its own protocol.
            ReasoningFormat::AnthropicThinking | ReasoningFormat::GeminiThinking => {}
        },
    }
}

impl TryFrom<chat::Response> for ProviderResponse {
    type Error = AiError;

    fn try_from(resp: chat::Response) -> Result<Self> {
        let choice =
            resp.choices.into_iter().next().ok_or_else(|| {
                AiError::InvalidResponse("No choices returned in response".into())
            })?;
        let message: common::Message = serde_json::from_value(choice.message.clone())
            .map_err(|e| AiError::InvalidResponse(format!("Invalid message payload: {e}")))?;
        let reasoning = choice
            .message
            .get("reasoning_content")
            .and_then(|v| v.as_str())
            .map(String::from);

        Ok(Self {
            message,
            reasoning,
            usage: resp.usage.map(common::Usage::from),
        })
    }
}

#[cfg(test)]
mod tests {
    use common::{GenerationOptions, Message};

    use super::*;
    use crate::ModelSelection;
    use crate::config::ProviderConfig;
    use crate::reasoning::ReasoningCapability;
    use crate::router::{ModelRouter, ResolvedSelection};

    fn resolved(
        format: ReasoningFormat,
        levels: Vec<ReasoningEffort>,
        effort: Option<ReasoningEffort>,
    ) -> ResolvedSelection {
        let mut configs = std::collections::HashMap::new();
        configs.insert(
            crate::config::ProviderId::new("openai"),
            ProviderConfig {
                protocol: Some(crate::config::Protocol::OpenAIChat),
                api_key: crate::config::ApiKey::Direct("k".into()),
                base_url: Some("https://example.com/v1".into()),
                models: {
                    let mut m = std::collections::HashMap::new();
                    m.insert(
                        crate::config::ModelId::new("m"),
                        crate::config::ModelConfig {
                            model: "test-model".into(),
                            display_name: None,
                            context_window: None,
                            max_tokens: None,
                            reasoning: Some(ReasoningCapability {
                                levels,
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
        router
            .resolve(&ModelSelection {
                provider: "openai".into(),
                model: "test-model".into(),
                reasoning_effort: effort,
            })
            .unwrap()
    }

    fn request_with(effort: Option<ReasoningEffort>) -> ChatRequest {
        ChatRequest {
            selection: ModelSelection {
                provider: "openai".into(),
                model: "test-model".into(),
                reasoning_effort: effort,
            },
            messages: vec![Message::user("hi")],
            options: GenerationOptions::default(),
            tools: Vec::new(),
            resolved: None,
        }
    }

    #[test]
    fn maps_openai_effort() {
        let mut req = request_with(Some(ReasoningEffort::High));
        req.resolved = Some(resolved(
            ReasoningFormat::OpenAIEffort,
            vec![ReasoningEffort::High],
            Some(ReasoningEffort::High),
        ));
        let wire = to_real_request(&req);
        assert_eq!(wire.reasoning_effort.as_deref(), Some("high"));
        assert!(wire.thinking.is_none());
    }

    #[test]
    fn maps_deepseek_thinking_on_and_off() {
        let mut req = request_with(Some(ReasoningEffort::High));
        req.resolved = Some(resolved(
            ReasoningFormat::DeepSeekThinking,
            vec![ReasoningEffort::High],
            Some(ReasoningEffort::High),
        ));
        let wire = to_real_request(&req);
        assert!(wire.reasoning_effort.is_none());
        assert_eq!(
            wire.thinking.as_ref().map(|t| t.typ.as_str()),
            Some("enabled")
        );

        let mut req = request_with(Some(ReasoningEffort::Off));
        req.resolved = Some(resolved(
            ReasoningFormat::DeepSeekThinking,
            vec![ReasoningEffort::High],
            Some(ReasoningEffort::Off),
        ));
        let wire = to_real_request(&req);
        assert_eq!(
            wire.thinking.as_ref().map(|t| t.typ.as_str()),
            Some("disabled")
        );
    }

    #[test]
    fn off_is_a_noop_on_openai_format() {
        let mut req = request_with(Some(ReasoningEffort::Off));
        req.resolved = Some(resolved(
            ReasoningFormat::OpenAIEffort,
            vec![ReasoningEffort::High],
            Some(ReasoningEffort::Off),
        ));
        let wire = to_real_request(&req);
        assert!(wire.reasoning_effort.is_none());
        assert!(wire.thinking.is_none());
    }

    #[test]
    fn maps_deepseek_effort_on_and_off() {
        // ecnu-max style: thinking switch + reasoning_effort strength.
        for (effort, expected) in [
            (ReasoningEffort::Low, "low"),
            (ReasoningEffort::High, "high"),
            (ReasoningEffort::Max, "max"),
        ] {
            let mut req = request_with(Some(effort));
            req.resolved = Some(resolved(
                ReasoningFormat::DeepSeekEffort,
                vec![
                    ReasoningEffort::Low,
                    ReasoningEffort::High,
                    ReasoningEffort::Max,
                ],
                Some(effort),
            ));
            let wire = to_real_request(&req);
            assert_eq!(
                wire.thinking.as_ref().map(|t| t.typ.as_str()),
                Some("enabled")
            );
            assert_eq!(wire.reasoning_effort.as_deref(), Some(expected));
        }

        // Off disables thinking and emits no reasoning_effort (strength only
        // takes effect while thinking is on).
        let mut req = request_with(Some(ReasoningEffort::Off));
        req.resolved = Some(resolved(
            ReasoningFormat::DeepSeekEffort,
            vec![ReasoningEffort::Low, ReasoningEffort::Max],
            Some(ReasoningEffort::Off),
        ));
        let wire = to_real_request(&req);
        assert_eq!(
            wire.thinking.as_ref().map(|t| t.typ.as_str()),
            Some("disabled")
        );
        assert!(wire.reasoning_effort.is_none());
    }

    #[test]
    fn no_resolved_spec_emits_nothing() {
        let req = request_with(Some(ReasoningEffort::High));
        let wire = to_real_request(&req);
        assert!(wire.reasoning_effort.is_none());
        assert!(wire.thinking.is_none());
    }

    #[test]
    fn effort_without_selection_emits_nothing() {
        let mut req = request_with(None);
        req.resolved = Some(resolved(
            ReasoningFormat::OpenAIEffort,
            vec![ReasoningEffort::High],
            Some(ReasoningEffort::High),
        ));
        let wire = to_real_request(&req);
        assert!(wire.reasoning_effort.is_none());
    }

    #[test]
    fn extracts_reasoning_content_from_non_stream_response() {
        let json = r#"{"id":"x","object":"chat.completion","choices":[{"index":0,"message":{"role":"assistant","content":"answer","reasoning_content":"chain"},"finish_reason":"stop"}],"usage":null}"#;
        let resp: super::chat::Response = serde_json::from_str(json).unwrap();
        let provider: ProviderResponse = resp.try_into().unwrap();
        assert_eq!(provider.message.text(), "answer");
        assert_eq!(provider.reasoning.as_deref(), Some("chain"));

        // plain responses carry no reasoning
        let json = r#"{"id":"x","choices":[{"index":0,"message":{"role":"assistant","content":"hi"},"finish_reason":"stop"}],"usage":null}"#;
        let resp: super::chat::Response = serde_json::from_str(json).unwrap();
        let provider: ProviderResponse = resp.try_into().unwrap();
        assert_eq!(provider.reasoning, None);
    }

    #[test]
    fn non_stream_usage_normalizes_cached_tokens() {
        let json = r#"{"id":"x","choices":[{"index":0,"message":{"role":"assistant","content":"hi"},"finish_reason":"stop"}],"usage":{"prompt_tokens":10,"completion_tokens":5,"total_tokens":15,"prompt_tokens_details":{"cached_tokens":3}}}"#;
        let resp: super::chat::Response = serde_json::from_str(json).unwrap();
        let provider: ProviderResponse = resp.try_into().unwrap();
        let usage = provider.usage.unwrap();
        assert_eq!(usage.prompt_tokens, 10);
        assert_eq!(usage.completion_tokens, 5);
        assert_eq!(usage.cached_tokens, 3);
    }
}
