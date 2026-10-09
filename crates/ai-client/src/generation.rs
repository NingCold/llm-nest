//! Constraints of the existing protocol/reasoning adapters, independent of providers' names.
use crate::{AiError, Protocol, ReasoningEffort, ReasoningFormat, ResolvedSelection, Result};
use common::GenerationOptions;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerationLimits {
    /// None means the current adapter/mode does not offer a temperature override.
    pub temperature_max: Option<f32>,
    pub min_output_tokens: u32,
    pub max_output_tokens: Option<u32>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ChatRequest, ModelSelection, ModelSpec, ReasoningCapability, protocols};

    fn resolved(
        protocol: Protocol,
        format: Option<ReasoningFormat>,
        effort: Option<ReasoningEffort>,
    ) -> ResolvedSelection {
        ResolvedSelection {
            selection: ModelSelection {
                provider: "fixture".into(),
                model: "fixture".into(),
                reasoning_effort: effort,
            },
            spec: ModelSpec {
                id: "fixture".into(),
                wire: "wire".into(),
                display_name: "Fixture".into(),
                context_window: Some(32768),
                max_tokens: Some(24576),
                protocol,
                reasoning: format.map(|format| ReasoningCapability {
                    format,
                    levels: vec![
                        ReasoningEffort::Off,
                        ReasoningEffort::Low,
                        ReasoningEffort::Medium,
                        ReasoningEffort::High,
                    ],
                    budget_tokens: None,
                }),
            },
        }
    }

    #[test]
    fn constraints_follow_reasoning_wire_modes_and_explicit_values_fail() {
        let mut r = resolved(
            Protocol::Anthropic,
            Some(ReasoningFormat::AnthropicThinking),
            Some(ReasoningEffort::Medium),
        );
        let options = GenerationOptions {
            temperature: None,
            max_tokens: Some(4096),
            ..Default::default()
        };
        assert_eq!(r.generation_limits().min_output_tokens, 4097);
        assert!(r.validate_generation(&options).is_err());
        assert!(
            r.validate_generation(&GenerationOptions {
                temperature: Some(1.0),
                max_tokens: Some(8192),
                ..options.clone()
            })
            .is_err()
        );
        assert!(
            r.validate_generation(&GenerationOptions {
                max_tokens: Some(8192),
                ..options.clone()
            })
            .is_ok()
        );
        r.selection.reasoning_effort = Some(ReasoningEffort::Off);
        assert_eq!(r.generation_limits().temperature_max, Some(1.0));
        assert!(
            r.validate_generation(&GenerationOptions {
                temperature: Some(1.1),
                ..options.clone()
            })
            .is_err()
        );
        for bad in [f32::NAN, f32::INFINITY, -0.1, 2.1] {
            assert!(
                resolved(Protocol::Gemini, None, None)
                    .validate_generation(&GenerationOptions {
                        temperature: Some(bad),
                        ..options.clone()
                    })
                    .is_err()
            );
        }
        for format in [
            ReasoningFormat::OpenAIEffort,
            ReasoningFormat::DeepSeekThinking,
            ReasoningFormat::DeepSeekEffort,
        ] {
            let r = resolved(
                Protocol::OpenAIChat,
                Some(format),
                Some(ReasoningEffort::Low),
            );
            assert_eq!(r.generation_limits().temperature_max, None);
            assert!(
                r.validate_generation(&GenerationOptions {
                    temperature: Some(0.5),
                    ..options.clone()
                })
                .is_err()
            );
        }
        assert_eq!(
            resolved(
                Protocol::OpenAIChat,
                Some(ReasoningFormat::DeepSeekThinking),
                Some(ReasoningEffort::Off)
            )
            .generation_limits()
            .temperature_max,
            Some(2.0)
        );
        assert!(
            r.validate_generation(&GenerationOptions {
                max_tokens: Some(24577),
                ..options
            })
            .is_err()
        );
    }

    #[test]
    fn all_four_protocols_carry_account_parameters_without_remapping_in_business_code() {
        let mut request = ChatRequest {
            selection: ModelSelection {
                provider: "fixture".into(),
                model: "fixture".into(),
                reasoning_effort: None,
            },
            messages: vec![common::Message::user("test")],
            options: GenerationOptions {
                temperature: Some(0.25),
                max_tokens: Some(4321),
                stream: true,
                ..Default::default()
            },
            tools: vec![],
            resolved: None,
        };
        let chat = protocols::openai::convert::to_real_request(&request);
        assert_eq!(chat.temperature, Some(0.25));
        assert_eq!(chat.max_tokens, Some(4321));
        let responses = protocols::openai_responses::convert::to_request(&request);
        assert_eq!(responses.temperature, Some(0.25));
        assert_eq!(responses.max_output_tokens, Some(4321));
        let gemini = protocols::gemini::convert::to_request(&request)
            .generation_config
            .unwrap();
        assert_eq!(gemini.temperature, Some(0.25));
        assert_eq!(gemini.max_output_tokens, Some(4321));
        request.resolved = Some(resolved(
            Protocol::Anthropic,
            Some(ReasoningFormat::AnthropicThinking),
            Some(ReasoningEffort::Low),
        ));
        request.selection.reasoning_effort = Some(ReasoningEffort::Low);
        request.options.temperature = None;
        let anthropic = protocols::anthropic::convert::to_request(&request);
        assert_eq!(anthropic.temperature, None);
        assert_eq!(anthropic.max_tokens, 4321);
        assert_eq!(
            anthropic.thinking.unwrap().budget_tokens,
            anthropic_thinking_budget(request.resolved.as_ref().unwrap())
        );
    }
}

/// Shared by Anthropic conversion and parameter validation. Unset effort emits no thinking block.
pub fn anthropic_thinking_budget(resolved: &ResolvedSelection) -> Option<u32> {
    if resolved.spec.protocol != Protocol::Anthropic {
        return None;
    }
    let capability = resolved.spec.reasoning.as_ref()?;
    if capability.format != ReasoningFormat::AnthropicThinking {
        return None;
    }
    let level = resolved.selection.reasoning_effort?;
    if level == ReasoningEffort::Off {
        return None;
    }
    Some(capability.budget_tokens.unwrap_or(match level {
        ReasoningEffort::Low => 1024,
        ReasoningEffort::Medium => 4096,
        ReasoningEffort::High => 16384,
        _ => crate::DEFAULT_THINKING_BUDGET_TOKENS,
    }))
}

impl ResolvedSelection {
    pub fn generation_limits(&self) -> GenerationLimits {
        let format = self.spec.reasoning.as_ref().map(|r| r.format);
        let temperature_supported = match (self.spec.protocol, format) {
            // Our effort adapters cannot emit an explicit OpenAI reasoning-none switch.
            (
                Protocol::OpenAIChat
                | Protocol::OpenAI
                | Protocol::OpenAIResponses
                | Protocol::Ollama,
                Some(ReasoningFormat::OpenAIEffort),
            ) => false,
            (
                Protocol::OpenAIChat | Protocol::OpenAI | Protocol::Ollama,
                Some(ReasoningFormat::DeepSeekThinking | ReasoningFormat::DeepSeekEffort),
            ) => self.selection.reasoning_effort == Some(ReasoningEffort::Off),
            (Protocol::Anthropic, Some(ReasoningFormat::AnthropicThinking)) => {
                anthropic_thinking_budget(self).is_none()
            }
            _ => true,
        };
        GenerationLimits {
            temperature_max: temperature_supported.then_some(
                if self.spec.protocol == Protocol::Anthropic {
                    1.0
                } else {
                    2.0
                },
            ),
            min_output_tokens: if self.spec.protocol == Protocol::Anthropic {
                anthropic_thinking_budget(self).map_or(1, |budget| budget.saturating_add(1))
            } else {
                1
            },
            max_output_tokens: self.spec.max_tokens,
        }
    }

    /// Called by catalog-backed social use cases before saving or dispatching parameters.
    /// Explicit values are rejected, never clamped or dropped to fit a different mode.
    pub fn validate_generation(&self, options: &GenerationOptions) -> Result<()> {
        let limits = self.generation_limits();
        let invalid = |message: &str| {
            AiError::ConfigError(format!(
                "{}/{}：{message}",
                self.selection.provider, self.selection.model
            ))
        };
        if let Some(temperature) = options.temperature {
            let max = limits
                .temperature_max
                .ok_or_else(|| invalid("当前模型/思考模式不支持调整温度，请留空"))?;
            if !temperature.is_finite() || !(0.0..=max).contains(&temperature) {
                return Err(invalid(&format!("温度必须在 0 到 {max} 之间")));
            }
        }
        if let Some(max) = options.max_tokens {
            if max < limits.min_output_tokens
                || limits.max_output_tokens.is_some_and(|limit| max > limit)
            {
                return Err(invalid(&format!(
                    "输出上限必须至少为 {}{}",
                    limits.min_output_tokens,
                    limits
                        .max_output_tokens
                        .map(|limit| format!("，且不超过 {limit}"))
                        .unwrap_or_default()
                )));
            }
        }
        Ok(())
    }
}
