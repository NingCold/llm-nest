//! Builtin provider directory.
//!
//! Mirrors Pi's `defaultModelPerProvider` + builtin catalog in reduced form.
//! Builtins never *register* a provider — a provider still requires a
//! configuration (it carries the API key) — they only supply defaults that a
//! minimal config can lean on:
//!
//! 1. the wire protocol and base_url of a known provider (config may omit
//!    both when the provider id matches the directory);
//! 2. a suggested API key environment variable name (documented, not read
//!    implicitly);
//! 3. the default model id (used when the config names no `default_model`
//!    and the effective catalog contains it);
//! 4. the model list with capability hints, merged field-by-field under a
//!    configured model entry (config wins, builtin fills gaps) and appended
//!    for models the config does not declare at all.
//!
//! Model-level protocol overrides are expressible in the directory too
//! ([`BuiltinModelEntry::protocol`]) — e.g. one gateway exposing several
//! protocols across its models.

use crate::config::{Protocol, ProviderConfig};
use crate::error::{AiError, Result};
use crate::reasoning::{ReasoningCapability, ReasoningEffort, ReasoningFormat};

/// Static reasoning capability of a builtin model.
#[derive(Debug, Clone, Copy)]
pub struct BuiltinReasoning {
    pub levels: &'static [ReasoningEffort],
    pub format: ReasoningFormat,
    pub budget_tokens: Option<u32>,
}

/// One builtin model: wire id plus informational capability hints.
/// These numbers are hints only and are never enforced on requests;
/// `None` means the vendor does not publish the value.
#[derive(Debug, Clone, Copy)]
pub struct BuiltinModelEntry {
    pub id: &'static str,
    pub display_name: &'static str,
    pub context_window: Option<u32>,
    pub max_tokens: Option<u32>,
    pub reasoning: Option<BuiltinReasoning>,
    /// Wire protocol override for this model (see [`BuiltinProviderEntry`]
    /// for the default). `None` inherits the provider's protocol.
    pub protocol: Option<Protocol>,
}

/// One builtin provider directory entry.
#[derive(Debug, Clone, Copy)]
pub struct BuiltinProviderEntry {
    pub id: &'static str,
    pub display_name: &'static str,
    pub protocol: Protocol,
    pub base_url: &'static str,
    pub api_key_env: &'static str,
    pub default_model: &'static str,
    pub models: &'static [BuiltinModelEntry],
}

const DS_K: &[BuiltinModelEntry] = &[
    BuiltinModelEntry {
        id: "deepseek-chat",
        display_name: "DeepSeek Chat",
        context_window: Some(64 * 1024),
        max_tokens: Some(8192),
        reasoning: None,
        protocol: None,
    },
    BuiltinModelEntry {
        id: "deepseek-reasoner",
        display_name: "DeepSeek Reasoner",
        context_window: Some(64 * 1024),
        max_tokens: Some(8192),
        reasoning: Some(BuiltinReasoning {
            levels: &[ReasoningEffort::Low, ReasoningEffort::High],
            format: ReasoningFormat::DeepSeekThinking,
            budget_tokens: None,
        }),
        protocol: None,
    },
];

const OA: &[BuiltinModelEntry] = &[
    BuiltinModelEntry {
        id: "gpt-4o",
        display_name: "GPT-4o",
        context_window: Some(128 * 1024),
        max_tokens: Some(16 * 1024),
        reasoning: None,
        protocol: None,
    },
    BuiltinModelEntry {
        id: "gpt-5.6-sol",
        display_name: "GPT-5.6 Sol",
        context_window: Some(1101000),
        max_tokens: Some(131072),
        reasoning: Some(BuiltinReasoning {
            levels: &[
                ReasoningEffort::Off,
                ReasoningEffort::Low,
                ReasoningEffort::Medium,
                ReasoningEffort::High,
                ReasoningEffort::Max,
            ],
            format: ReasoningFormat::OpenAIEffort,
            budget_tokens: None,
        }),
        protocol: None,
    },
    BuiltinModelEntry {
        id: "gpt-5.6-terra",
        display_name: "GPT-5.6 Terra",
        context_window: Some(1101000),
        max_tokens: Some(131072),
        reasoning: Some(BuiltinReasoning {
            levels: &[
                ReasoningEffort::Off,
                ReasoningEffort::Low,
                ReasoningEffort::Medium,
                ReasoningEffort::High,
                ReasoningEffort::Max,
            ],
            format: ReasoningFormat::OpenAIEffort,
            budget_tokens: None,
        }),
        protocol: None,
    },
    BuiltinModelEntry {
        id: "gpt-5.6-luna",
        display_name: "GPT-5.6 Luna",
        context_window: Some(1101000),
        max_tokens: Some(131072),
        reasoning: Some(BuiltinReasoning {
            levels: &[
                ReasoningEffort::Off,
                ReasoningEffort::Low,
                ReasoningEffort::Medium,
                ReasoningEffort::High,
                ReasoningEffort::Max,
            ],
            format: ReasoningFormat::OpenAIEffort,
            budget_tokens: None,
        }),
        protocol: None,
    },
];

const GE: &[BuiltinModelEntry] = &[
    // Gemini 3.x reasons via thinkingLevel (minimal/low/medium/high), which
    // our gemini-thinking adapter (integer thinkingBudget) cannot express
    // yet — no reasoning declaration until the adapter speaks thinkingLevel.
    BuiltinModelEntry {
        id: "gemini-3.7-flash",
        display_name: "Gemini 3.7 Flash",
        context_window: None,
        max_tokens: None,
        reasoning: None,
        protocol: None,
    },
    BuiltinModelEntry {
        id: "gemini-2.5-pro",
        display_name: "Gemini 2.5 Pro",
        context_window: Some(1024 * 1024),
        max_tokens: Some(65536),
        reasoning: Some(BuiltinReasoning {
            levels: &[
                ReasoningEffort::Off,
                ReasoningEffort::Low,
                ReasoningEffort::Medium,
                ReasoningEffort::High,
            ],
            format: ReasoningFormat::GeminiThinking,
            budget_tokens: None,
        }),
        protocol: None,
    },
    BuiltinModelEntry {
        id: "gemini-2.5-flash",
        display_name: "Gemini 2.5 Flash",
        context_window: Some(1024 * 1024),
        max_tokens: Some(65536),
        reasoning: Some(BuiltinReasoning {
            levels: &[
                ReasoningEffort::Off,
                ReasoningEffort::Low,
                ReasoningEffort::Medium,
                ReasoningEffort::High,
            ],
            format: ReasoningFormat::GeminiThinking,
            budget_tokens: None,
        }),
        protocol: None,
    },
    BuiltinModelEntry {
        id: "gemini-2.5-flash-lite",
        display_name: "Gemini 2.5 Flash Lite",
        context_window: Some(1024 * 1024),
        max_tokens: Some(65536),
        reasoning: Some(BuiltinReasoning {
            levels: &[
                ReasoningEffort::Off,
                ReasoningEffort::Low,
                ReasoningEffort::Medium,
                ReasoningEffort::High,
            ],
            format: ReasoningFormat::GeminiThinking,
            budget_tokens: None,
        }),
        protocol: None,
    },
];

const AN: &[BuiltinModelEntry] = &[
    // Claude 5 series reasons via adaptive thinking + effort, not the
    // budget_tokens extended-thinking block (rejected on 4.7+), so no
    // reasoning declaration until the anthropic adapter speaks effort.
    BuiltinModelEntry {
        id: "claude-opus-5",
        display_name: "Claude Opus 5",
        context_window: Some(1000 * 1024),
        max_tokens: Some(131072),
        reasoning: None,
        protocol: None,
    },
    BuiltinModelEntry {
        id: "claude-sonnet-5",
        display_name: "Claude Sonnet 5",
        context_window: Some(1000 * 1024),
        max_tokens: Some(131072),
        reasoning: None,
        protocol: None,
    },
    BuiltinModelEntry {
        id: "claude-sonnet-4-5",
        display_name: "Claude Sonnet 4.5",
        context_window: Some(200 * 1024),
        max_tokens: Some(64 * 1024),
        reasoning: Some(BuiltinReasoning {
            levels: &[
                ReasoningEffort::Off,
                ReasoningEffort::Low,
                ReasoningEffort::Medium,
                ReasoningEffort::High,
            ],
            format: ReasoningFormat::AnthropicThinking,
            budget_tokens: None,
        }),
        protocol: None,
    },
    BuiltinModelEntry {
        id: "claude-opus-4-5",
        display_name: "Claude Opus 4.5",
        context_window: Some(200 * 1024),
        max_tokens: Some(128 * 1024),
        reasoning: Some(BuiltinReasoning {
            levels: &[
                ReasoningEffort::Off,
                ReasoningEffort::Low,
                ReasoningEffort::Medium,
                ReasoningEffort::High,
            ],
            format: ReasoningFormat::AnthropicThinking,
            budget_tokens: None,
        }),
        protocol: None,
    },
    BuiltinModelEntry {
        id: "claude-haiku-4-5",
        display_name: "Claude Haiku 4.5",
        context_window: Some(200 * 1024),
        max_tokens: Some(65536),
        reasoning: Some(BuiltinReasoning {
            levels: &[
                ReasoningEffort::Off,
                ReasoningEffort::Low,
                ReasoningEffort::Medium,
                ReasoningEffort::High,
            ],
            format: ReasoningFormat::AnthropicThinking,
            budget_tokens: None,
        }),
        protocol: None,
    },
];

const XAI: &[BuiltinModelEntry] = &[
    BuiltinModelEntry {
        id: "grok-4.6",
        display_name: "Grok 4.6",
        context_window: Some(500 * 1024),
        max_tokens: None,
        reasoning: Some(BuiltinReasoning {
            levels: &[
                ReasoningEffort::Low,
                ReasoningEffort::Medium,
                ReasoningEffort::High,
            ],
            format: ReasoningFormat::OpenAIEffort,
            budget_tokens: None,
        }),
        protocol: None,
    },
    BuiltinModelEntry {
        id: "grok-4.5",
        display_name: "Grok 4.5",
        context_window: Some(500 * 1024),
        max_tokens: None,
        reasoning: Some(BuiltinReasoning {
            levels: &[
                ReasoningEffort::Low,
                ReasoningEffort::Medium,
                ReasoningEffort::High,
            ],
            format: ReasoningFormat::OpenAIEffort,
            budget_tokens: None,
        }),
        protocol: None,
    },
    BuiltinModelEntry {
        id: "grok-4.3",
        display_name: "Grok 4.3",
        context_window: Some(1000 * 1024),
        max_tokens: None,
        reasoning: Some(BuiltinReasoning {
            levels: &[
                ReasoningEffort::Off,
                ReasoningEffort::Low,
                ReasoningEffort::Medium,
                ReasoningEffort::High,
            ],
            format: ReasoningFormat::OpenAIEffort,
            budget_tokens: None,
        }),
        protocol: None,
    },
    // grok-4.20 reasons by model-name suffix (`-reasoning`); no effort param.
    BuiltinModelEntry {
        id: "grok-4.20-reasoning",
        display_name: "Grok 4.20 (reasoning)",
        context_window: Some(1000 * 1024),
        max_tokens: None,
        reasoning: None,
        protocol: None,
    },
    BuiltinModelEntry {
        id: "grok-4.20-non-reasoning",
        display_name: "Grok 4.20 (non-reasoning)",
        context_window: Some(1000 * 1024),
        max_tokens: None,
        reasoning: None,
        protocol: None,
    },
];

const ECNU: &[BuiltinModelEntry] = &[
    BuiltinModelEntry {
        id: "ecnu-max",
        display_name: "DeepSeek-V4-Flash",
        context_window: Some(1000 * 1024),
        max_tokens: Some(384 * 1024),
        reasoning: Some(BuiltinReasoning {
            levels: &[
                ReasoningEffort::Off,
                ReasoningEffort::Low,
                ReasoningEffort::High,
                ReasoningEffort::Max,
            ],
            format: ReasoningFormat::DeepSeekEffort,
            budget_tokens: None,
        }),
        protocol: None,
    },
    BuiltinModelEntry {
        id: "ecnu-plus",
        display_name: "ECNU Plus",
        context_window: Some(1000 * 1024),
        max_tokens: Some(384 * 1024),
        reasoning: None,
        protocol: None,
    },
];

const OCG: &[BuiltinModelEntry] = &[
    BuiltinModelEntry {
        id: "deepseek-v4-flash",
        display_name: "DeepSeek V4 Flash",
        context_window: Some(1000 * 1024),
        max_tokens: Some(384 * 1024),
        reasoning: Some(BuiltinReasoning {
            levels: &[
                ReasoningEffort::Off,
                ReasoningEffort::Low,
                ReasoningEffort::High,
                ReasoningEffort::Max,
            ],
            format: ReasoningFormat::DeepSeekEffort,
            budget_tokens: None,
        }),
        protocol: None,
    },
    BuiltinModelEntry {
        id: "deepseek-v4-pro",
        display_name: "DeepSeek V4 Pro",
        context_window: Some(1000 * 1024),
        max_tokens: Some(384 * 1024),
        reasoning: Some(BuiltinReasoning {
            levels: &[
                ReasoningEffort::Off,
                ReasoningEffort::Low,
                ReasoningEffort::High,
                ReasoningEffort::Max,
            ],
            format: ReasoningFormat::DeepSeekEffort,
            budget_tokens: None,
        }),
        protocol: None,
    },
    BuiltinModelEntry {
        id: "mimo-v2.5",
        display_name: "MiMo V2.5",
        context_window: Some(1000 * 1024),
        max_tokens: Some(128 * 1024),
        reasoning: None,
        protocol: None,
    },
    BuiltinModelEntry {
        id: "ox-alpha-free",
        display_name: "Ox Alpha Free",
        context_window: Some(1000 * 1024),
        max_tokens: Some(128 * 1024),
        reasoning: Some(BuiltinReasoning {
            levels: &[
                ReasoningEffort::Off,
                ReasoningEffort::Low,
                ReasoningEffort::High,
                ReasoningEffort::Max,
            ],
            format: ReasoningFormat::OpenAIEffort,
            budget_tokens: None,
        }),
        protocol: None,
    },
];

const SF: &[BuiltinModelEntry] = &[
    BuiltinModelEntry {
        id: "deepseek-ai/DeepSeek-V4-Flash",
        display_name: "DeepSeek V4 Flash",
        context_window: Some(1000 * 1024),
        max_tokens: Some(384 * 1024),
        reasoning: Some(BuiltinReasoning {
            levels: &[
                ReasoningEffort::Off,
                ReasoningEffort::Low,
                ReasoningEffort::High,
                ReasoningEffort::Max,
            ],
            format: ReasoningFormat::DeepSeekEffort,
            budget_tokens: None,
        }),
        protocol: None,
    },
    BuiltinModelEntry {
        id: "Pro/deepseek-ai/DeepSeek-V4",
        display_name: "DeepSeek V4 (Pro)",
        context_window: None,
        max_tokens: None,
        reasoning: Some(BuiltinReasoning {
            levels: &[
                ReasoningEffort::Off,
                ReasoningEffort::Low,
                ReasoningEffort::High,
                ReasoningEffort::Max,
            ],
            format: ReasoningFormat::DeepSeekEffort,
            budget_tokens: None,
        }),
        protocol: None,
    },
    BuiltinModelEntry {
        id: "Pro/deepseek-ai/DeepSeek-R1",
        display_name: "DeepSeek R1 (Pro)",
        context_window: None,
        max_tokens: None,
        reasoning: Some(BuiltinReasoning {
            levels: &[ReasoningEffort::Low, ReasoningEffort::High],
            format: ReasoningFormat::DeepSeekThinking,
            budget_tokens: None,
        }),
        protocol: None,
    },
    BuiltinModelEntry {
        id: "deepseek-ai/DeepSeek-V3",
        display_name: "DeepSeek V3",
        context_window: None,
        max_tokens: None,
        reasoning: Some(BuiltinReasoning {
            levels: &[ReasoningEffort::Low, ReasoningEffort::High],
            format: ReasoningFormat::DeepSeekThinking,
            budget_tokens: None,
        }),
        protocol: None,
    },
    BuiltinModelEntry {
        id: "Pro/zai-org/GLM-5.2",
        display_name: "GLM 5.2 (Pro)",
        context_window: None,
        max_tokens: None,
        reasoning: Some(BuiltinReasoning {
            levels: &[
                ReasoningEffort::Off,
                ReasoningEffort::Low,
                ReasoningEffort::High,
                ReasoningEffort::Max,
            ],
            format: ReasoningFormat::DeepSeekEffort,
            budget_tokens: None,
        }),
        protocol: None,
    },
];

const TR: &[BuiltinModelEntry] = &[
    BuiltinModelEntry {
        id: "deepseek-v4-flash",
        display_name: "DeepSeek V4 Flash",
        context_window: Some(1024 * 1024),
        max_tokens: Some(384 * 1024),
        reasoning: Some(BuiltinReasoning {
            levels: &[
                ReasoningEffort::Off,
                ReasoningEffort::Low,
                ReasoningEffort::High,
                ReasoningEffort::Max,
            ],
            format: ReasoningFormat::DeepSeekEffort,
            budget_tokens: None,
        }),
        protocol: None,
    },
    BuiltinModelEntry {
        id: "deepseek-v4-pro",
        display_name: "DeepSeek V4 Pro",
        context_window: Some(1024 * 1024),
        max_tokens: None,
        reasoning: Some(BuiltinReasoning {
            levels: &[
                ReasoningEffort::Off,
                ReasoningEffort::Low,
                ReasoningEffort::High,
                ReasoningEffort::Max,
            ],
            format: ReasoningFormat::DeepSeekEffort,
            budget_tokens: None,
        }),
        protocol: None,
    },
    BuiltinModelEntry {
        id: "deepseek-v4-flash-0731",
        display_name: "DeepSeek V4 Flash 0731",
        context_window: Some(1024 * 1024),
        max_tokens: Some(384 * 1024),
        reasoning: Some(BuiltinReasoning {
            levels: &[
                ReasoningEffort::Off,
                ReasoningEffort::Low,
                ReasoningEffort::High,
                ReasoningEffort::Max,
            ],
            format: ReasoningFormat::DeepSeekEffort,
            budget_tokens: None,
        }),
        protocol: None,
    },
    BuiltinModelEntry {
        id: "glm-5.2",
        display_name: "GLM 5.2",
        context_window: Some(1024 * 1024),
        max_tokens: Some(131072),
        reasoning: Some(BuiltinReasoning {
            levels: &[
                ReasoningEffort::Low,
                ReasoningEffort::Medium,
                ReasoningEffort::High,
                ReasoningEffort::Max,
            ],
            format: ReasoningFormat::OpenAIEffort,
            budget_tokens: None,
        }),
        protocol: None,
    },
    BuiltinModelEntry {
        id: "kimi-k2.6",
        display_name: "Kimi K2.6",
        context_window: Some(256 * 1024),
        max_tokens: None,
        reasoning: Some(BuiltinReasoning {
            levels: &[ReasoningEffort::Low, ReasoningEffort::High],
            format: ReasoningFormat::DeepSeekThinking,
            budget_tokens: None,
        }),
        protocol: None,
    },
];

/// OpenCode Zen: one gateway exposing several protocols across its models —
/// the flagship case for model-level protocol override. The base URL is
/// shared (`https://opencode.ai/zen/v1`); each adapter appends its own path
/// (`/responses`, `/messages`, `/models/{id}`, `/chat/completions`).
const OZ: &[BuiltinModelEntry] = &[
    // OpenAI Responses API family
    BuiltinModelEntry {
        id: "gpt-5.6-sol",
        display_name: "GPT 5.6 Sol",
        context_window: None,
        max_tokens: None,
        reasoning: Some(BuiltinReasoning {
            levels: &[
                ReasoningEffort::Off,
                ReasoningEffort::Low,
                ReasoningEffort::Medium,
                ReasoningEffort::High,
                ReasoningEffort::Max,
            ],
            format: ReasoningFormat::OpenAIEffort,
            budget_tokens: None,
        }),
        protocol: Some(Protocol::OpenAIResponses),
    },
    BuiltinModelEntry {
        id: "gpt-5.6-terra",
        display_name: "GPT 5.6 Terra",
        context_window: None,
        max_tokens: None,
        reasoning: Some(BuiltinReasoning {
            levels: &[
                ReasoningEffort::Off,
                ReasoningEffort::Low,
                ReasoningEffort::Medium,
                ReasoningEffort::High,
                ReasoningEffort::Max,
            ],
            format: ReasoningFormat::OpenAIEffort,
            budget_tokens: None,
        }),
        protocol: Some(Protocol::OpenAIResponses),
    },
    BuiltinModelEntry {
        id: "gpt-5.6-luna",
        display_name: "GPT 5.6 Luna",
        context_window: None,
        max_tokens: None,
        reasoning: Some(BuiltinReasoning {
            levels: &[
                ReasoningEffort::Off,
                ReasoningEffort::Low,
                ReasoningEffort::Medium,
                ReasoningEffort::High,
                ReasoningEffort::Max,
            ],
            format: ReasoningFormat::OpenAIEffort,
            budget_tokens: None,
        }),
        protocol: Some(Protocol::OpenAIResponses),
    },
    BuiltinModelEntry {
        id: "grok-4.6",
        display_name: "Grok 4.6",
        context_window: None,
        max_tokens: None,
        reasoning: None,
        protocol: Some(Protocol::OpenAIResponses),
    },
    // Anthropic Messages family
    BuiltinModelEntry {
        id: "claude-opus-5",
        display_name: "Claude Opus 5",
        context_window: None,
        max_tokens: None,
        reasoning: Some(BuiltinReasoning {
            levels: &[
                ReasoningEffort::Off,
                ReasoningEffort::Low,
                ReasoningEffort::Medium,
                ReasoningEffort::High,
            ],
            format: ReasoningFormat::AnthropicThinking,
            budget_tokens: None,
        }),
        protocol: Some(Protocol::Anthropic),
    },
    BuiltinModelEntry {
        id: "claude-sonnet-5",
        display_name: "Claude Sonnet 5",
        context_window: None,
        max_tokens: None,
        reasoning: Some(BuiltinReasoning {
            levels: &[
                ReasoningEffort::Off,
                ReasoningEffort::Low,
                ReasoningEffort::Medium,
                ReasoningEffort::High,
            ],
            format: ReasoningFormat::AnthropicThinking,
            budget_tokens: None,
        }),
        protocol: Some(Protocol::Anthropic),
    },
    BuiltinModelEntry {
        id: "qwen3.7-max",
        display_name: "Qwen3.7 Max",
        context_window: None,
        max_tokens: None,
        reasoning: Some(BuiltinReasoning {
            levels: &[
                ReasoningEffort::Off,
                ReasoningEffort::Low,
                ReasoningEffort::Medium,
                ReasoningEffort::High,
            ],
            format: ReasoningFormat::AnthropicThinking,
            budget_tokens: None,
        }),
        protocol: Some(Protocol::Anthropic),
    },
    // Gemini native family (model id in the URL path)
    BuiltinModelEntry {
        id: "gemini-3.7-flash",
        display_name: "Gemini 3.7 Flash",
        context_window: None,
        max_tokens: None,
        reasoning: Some(BuiltinReasoning {
            levels: &[
                ReasoningEffort::Off,
                ReasoningEffort::Low,
                ReasoningEffort::Medium,
                ReasoningEffort::High,
            ],
            format: ReasoningFormat::GeminiThinking,
            budget_tokens: None,
        }),
        protocol: Some(Protocol::Gemini),
    },
    // OpenAI chat completions family (inherits the provider default protocol)
    BuiltinModelEntry {
        id: "deepseek-v4-pro",
        display_name: "DeepSeek V4 Pro",
        context_window: None,
        max_tokens: None,
        reasoning: Some(BuiltinReasoning {
            levels: &[
                ReasoningEffort::Off,
                ReasoningEffort::Low,
                ReasoningEffort::High,
                ReasoningEffort::Max,
            ],
            format: ReasoningFormat::DeepSeekEffort,
            budget_tokens: None,
        }),
        protocol: None,
    },
    BuiltinModelEntry {
        id: "kimi-k2.7-code",
        display_name: "Kimi K2.7 Code",
        context_window: None,
        max_tokens: None,
        reasoning: Some(BuiltinReasoning {
            levels: &[ReasoningEffort::Low, ReasoningEffort::High],
            format: ReasoningFormat::DeepSeekThinking,
            budget_tokens: None,
        }),
        protocol: None,
    },
];

const KIMI: &[BuiltinModelEntry] = &[
    BuiltinModelEntry {
        id: "kimi-k3",
        display_name: "Kimi K3",
        context_window: Some(1000 * 1024),
        max_tokens: Some(131072),
        reasoning: Some(BuiltinReasoning {
            levels: &[
                ReasoningEffort::Low,
                ReasoningEffort::High,
                ReasoningEffort::Max,
            ],
            format: ReasoningFormat::OpenAIEffort,
            budget_tokens: None,
        }),
        protocol: None,
    },
    BuiltinModelEntry {
        id: "kimi-k2.6",
        display_name: "Kimi K2.6",
        context_window: Some(256 * 1024),
        max_tokens: Some(32 * 1024),
        reasoning: Some(BuiltinReasoning {
            levels: &[ReasoningEffort::Low, ReasoningEffort::High],
            format: ReasoningFormat::DeepSeekThinking,
            budget_tokens: None,
        }),
        protocol: None,
    },
    BuiltinModelEntry {
        id: "kimi-k2.5",
        display_name: "Kimi K2.5",
        context_window: Some(256 * 1024),
        max_tokens: Some(8192),
        reasoning: Some(BuiltinReasoning {
            levels: &[ReasoningEffort::Low, ReasoningEffort::High],
            format: ReasoningFormat::DeepSeekThinking,
            budget_tokens: None,
        }),
        protocol: None,
    },
];

const ZHIPU: &[BuiltinModelEntry] = &[
    BuiltinModelEntry {
        id: "glm-5.3",
        display_name: "GLM-5.3",
        context_window: Some(1048576),
        max_tokens: Some(131072),
        reasoning: Some(BuiltinReasoning {
            levels: &[
                ReasoningEffort::Low,
                ReasoningEffort::High,
                ReasoningEffort::Max,
            ],
            format: ReasoningFormat::OpenAIEffort,
            budget_tokens: None,
        }),
        protocol: None,
    },
    BuiltinModelEntry {
        id: "glm-5.2",
        display_name: "GLM-5.2",
        context_window: Some(1048576),
        max_tokens: Some(131072),
        reasoning: Some(BuiltinReasoning {
            levels: &[
                ReasoningEffort::Low,
                ReasoningEffort::Medium,
                ReasoningEffort::High,
                ReasoningEffort::Max,
            ],
            format: ReasoningFormat::OpenAIEffort,
            budget_tokens: None,
        }),
        protocol: None,
    },
    BuiltinModelEntry {
        id: "glm-4.7",
        display_name: "GLM-4.7",
        context_window: Some(204800),
        max_tokens: Some(131072),
        reasoning: Some(BuiltinReasoning {
            levels: &[ReasoningEffort::Low, ReasoningEffort::High],
            format: ReasoningFormat::DeepSeekThinking,
            budget_tokens: None,
        }),
        protocol: None,
    },
    BuiltinModelEntry {
        id: "glm-4.6",
        display_name: "GLM-4.6",
        context_window: Some(204800),
        max_tokens: Some(131072),
        reasoning: Some(BuiltinReasoning {
            levels: &[ReasoningEffort::Low, ReasoningEffort::High],
            format: ReasoningFormat::DeepSeekThinking,
            budget_tokens: None,
        }),
        protocol: None,
    },
    BuiltinModelEntry {
        id: "glm-4.7-flash",
        display_name: "GLM-4.7-Flash",
        context_window: Some(204800),
        max_tokens: Some(131072),
        reasoning: Some(BuiltinReasoning {
            levels: &[ReasoningEffort::Low, ReasoningEffort::High],
            format: ReasoningFormat::DeepSeekThinking,
            budget_tokens: None,
        }),
        protocol: None,
    },
];

const MM: &[BuiltinModelEntry] = &[
    BuiltinModelEntry {
        id: "MiniMax-M3",
        display_name: "MiniMax M3",
        context_window: Some(1000 * 1024),
        max_tokens: Some(131072),
        reasoning: None,
        protocol: None,
    },
    BuiltinModelEntry {
        id: "MiniMax-M2.7",
        display_name: "MiniMax M2.7",
        context_window: Some(204800),
        max_tokens: Some(65536),
        reasoning: None,
        protocol: None,
    },
    BuiltinModelEntry {
        id: "MiniMax-M2.5",
        display_name: "MiniMax M2.5",
        context_window: Some(204800),
        max_tokens: Some(65536),
        reasoning: None,
        protocol: None,
    },
];

const MIMO: &[BuiltinModelEntry] = &[
    BuiltinModelEntry {
        id: "mimo-v2.5-pro",
        display_name: "MiMo-V2.5 Pro",
        context_window: Some(1000 * 1024),
        max_tokens: Some(131072),
        reasoning: Some(BuiltinReasoning {
            levels: &[
                ReasoningEffort::Off,
                ReasoningEffort::Low,
                ReasoningEffort::High,
            ],
            format: ReasoningFormat::DeepSeekThinking,
            budget_tokens: None,
        }),
        protocol: None,
    },
    BuiltinModelEntry {
        id: "mimo-v2.5",
        display_name: "MiMo-V2.5",
        context_window: Some(1000 * 1024),
        max_tokens: Some(131072),
        reasoning: Some(BuiltinReasoning {
            levels: &[
                ReasoningEffort::Off,
                ReasoningEffort::Low,
                ReasoningEffort::High,
            ],
            format: ReasoningFormat::DeepSeekThinking,
            budget_tokens: None,
        }),
        protocol: None,
    },
];

const OR: &[BuiltinModelEntry] = &[
    BuiltinModelEntry {
        id: "openrouter/auto",
        display_name: "Auto Router",
        context_window: Some(2000 * 1024),
        max_tokens: Some(128 * 1024),
        reasoning: None,
        protocol: None,
    },
    BuiltinModelEntry {
        id: "openrouter/fusion",
        display_name: "OpenRouter: Fusion",
        context_window: Some(1000 * 1024),
        max_tokens: Some(128 * 1024),
        reasoning: None,
        protocol: None,
    },
    BuiltinModelEntry {
        id: "deepseek/deepseek-v4-flash-0731",
        display_name: "DeepSeek V4 Flash 0731",
        context_window: Some(1024 * 1024),
        max_tokens: Some(64 * 1024),
        reasoning: Some(BuiltinReasoning {
            levels: &[
                ReasoningEffort::Off,
                ReasoningEffort::Low,
                ReasoningEffort::High,
                ReasoningEffort::Max,
            ],
            format: ReasoningFormat::DeepSeekEffort,
            budget_tokens: None,
        }),
        protocol: None,
    },
    BuiltinModelEntry {
        id: "google/gemini-3.7-flash",
        display_name: "Gemini 3.7 Flash",
        context_window: Some(1024 * 1024),
        max_tokens: Some(65536),
        reasoning: Some(BuiltinReasoning {
            levels: &[
                ReasoningEffort::Off,
                ReasoningEffort::Low,
                ReasoningEffort::Medium,
                ReasoningEffort::High,
            ],
            format: ReasoningFormat::OpenAIEffort,
            budget_tokens: None,
        }),
        protocol: None,
    },
    BuiltinModelEntry {
        id: "qwen/qwen3.8-max",
        display_name: "Qwen3.8 Max",
        context_window: Some(1000 * 1024),
        max_tokens: Some(131072),
        reasoning: Some(BuiltinReasoning {
            levels: &[
                ReasoningEffort::Off,
                ReasoningEffort::Low,
                ReasoningEffort::Medium,
                ReasoningEffort::High,
                ReasoningEffort::Max,
            ],
            format: ReasoningFormat::OpenAIEffort,
            budget_tokens: None,
        }),
        protocol: None,
    },
];

/// Builtin provider directory. `id` is the provider key users write in
/// `[providers.<id>]`; a config matching the id may omit `protocol`,
/// `base_url` and `models`, and gets the default model for free.
pub const BUILTIN_PROVIDERS: &[BuiltinProviderEntry] = &[
    BuiltinProviderEntry {
        id: "anthropic",
        display_name: "Anthropic",
        protocol: Protocol::Anthropic,
        base_url: "https://api.anthropic.com/v1",
        api_key_env: "ANTHROPIC_API_KEY",
        default_model: "claude-sonnet-4-5",
        models: AN,
    },
    BuiltinProviderEntry {
        id: "chatecnu",
        display_name: "ChatECNU",
        protocol: Protocol::OpenAI,
        base_url: "https://chat.ecnu.edu.cn/open/api/v1/",
        api_key_env: "CHATECNU_API_KEY",
        default_model: "ecnu-max",
        models: ECNU,
    },
    BuiltinProviderEntry {
        id: "deepseek",
        display_name: "DeepSeek",
        protocol: Protocol::OpenAI,
        base_url: "https://api.deepseek.com",
        api_key_env: "DEEPSEEK_API_KEY",
        default_model: "deepseek-chat",
        models: DS_K,
    },
    BuiltinProviderEntry {
        id: "gemini",
        display_name: "Gemini",
        protocol: Protocol::Gemini,
        base_url: "https://generativelanguage.googleapis.com/v1beta",
        api_key_env: "GEMINI_API_KEY",
        default_model: "gemini-3.7-flash",
        models: GE,
    },
    BuiltinProviderEntry {
        id: "kimi",
        display_name: "月之暗面 (Kimi)",
        protocol: Protocol::OpenAI,
        base_url: "https://api.moonshot.cn/v1",
        api_key_env: "MOONSHOT_API_KEY",
        default_model: "kimi-k3",
        models: KIMI,
    },
    BuiltinProviderEntry {
        id: "minimax",
        display_name: "MiniMax",
        protocol: Protocol::OpenAI,
        base_url: "https://api.minimaxi.com/v1",
        api_key_env: "MINIMAX_API_KEY",
        default_model: "MiniMax-M3",
        models: MM,
    },
    BuiltinProviderEntry {
        id: "mimo",
        display_name: "小米 MiMo",
        protocol: Protocol::OpenAI,
        base_url: "https://api.xiaomimimo.com/v1",
        api_key_env: "MIMO_API_KEY",
        default_model: "mimo-v2.5-pro",
        models: MIMO,
    },
    BuiltinProviderEntry {
        id: "opencode-go",
        display_name: "OpenCode Go",
        protocol: Protocol::OpenAI,
        base_url: "https://opencode.ai/zen/go/v1",
        api_key_env: "OPENCODE_API_KEY",
        default_model: "deepseek-v4-flash",
        models: OCG,
    },
    BuiltinProviderEntry {
        id: "opencode-zen",
        display_name: "OpenCode Zen",
        protocol: Protocol::OpenAI,
        base_url: "https://opencode.ai/zen/v1",
        api_key_env: "OPENCODE_ZEN_API_KEY",
        default_model: "gpt-5.6-sol",
        models: OZ,
    },
    BuiltinProviderEntry {
        id: "openai",
        display_name: "OpenAI",
        protocol: Protocol::OpenAI,
        base_url: "https://api.openai.com/v1",
        api_key_env: "OPENAI_API_KEY",
        default_model: "gpt-5.6-sol",
        models: OA,
    },
    BuiltinProviderEntry {
        id: "openrouter",
        display_name: "OpenRouter",
        protocol: Protocol::OpenAI,
        base_url: "https://openrouter.ai/api/v1",
        api_key_env: "OPENROUTER_API_KEY",
        default_model: "openrouter/auto",
        models: OR,
    },
    BuiltinProviderEntry {
        id: "xai",
        display_name: "xAI (Grok)",
        protocol: Protocol::OpenAI,
        base_url: "https://api.x.ai/v1",
        api_key_env: "XAI_API_KEY",
        default_model: "grok-4.6",
        models: XAI,
    },
    BuiltinProviderEntry {
        id: "siliconflow",
        display_name: "硅基流动",
        protocol: Protocol::OpenAI,
        base_url: "https://api.siliconflow.cn/v1",
        api_key_env: "SILICONFLOW_API_KEY",
        default_model: "deepseek-ai/DeepSeek-V4-Flash",
        models: SF,
    },
    BuiltinProviderEntry {
        id: "tokenrhythm",
        display_name: "基元律动",
        protocol: Protocol::OpenAI,
        base_url: "https://tokenrhythm.studio/v1",
        api_key_env: "TOKENRHYTHM_API_KEY",
        default_model: "deepseek-v4-flash",
        models: TR,
    },
    BuiltinProviderEntry {
        id: "zhipu",
        display_name: "智谱清言 (BigModel)",
        protocol: Protocol::OpenAI,
        base_url: "https://open.bigmodel.cn/api/paas/v4",
        api_key_env: "ZHIPU_API_KEY",
        default_model: "glm-5.3",
        models: ZHIPU,
    },
];

/// Builtin directory entry for `provider`, if known.
pub fn builtin_provider(provider: &str) -> Option<&'static BuiltinProviderEntry> {
    BUILTIN_PROVIDERS.iter().find(|p| p.id == provider)
}

/// Builtin default model id for `provider`, if known.
pub fn builtin_default_model(provider: &str) -> Option<&'static str> {
    builtin_provider(provider).map(|p| p.default_model)
}

/// Capability hints for `(provider, wire model id)`, if known.
pub fn builtin_model(provider: &str, wire: &str) -> Option<&'static BuiltinModelEntry> {
    builtin_provider(provider).and_then(|p| p.models.iter().find(|m| m.id == wire))
}

/// Effective wire protocol for a provider route: config override, else the
/// builtin directory, else an error naming the provider (fail-fast at
/// startup).
pub fn effective_protocol(provider: &str, cfg: &ProviderConfig) -> Result<Protocol> {
    cfg.protocol
        .or_else(|| builtin_provider(provider).map(|p| p.protocol))
        .ok_or_else(|| {
            AiError::ConfigError(format!(
                "provider '{provider}': no protocol configured and no builtin directory \
                 entry; set protocol = \"...\""
            ))
        })
}

/// Effective base_url for a provider route: config override, else the builtin
/// directory, else an error naming the provider (fail-fast at startup).
pub fn effective_base_url(provider: &str, cfg: &ProviderConfig) -> Result<String> {
    cfg.base_url
        .clone()
        .or_else(|| builtin_provider(provider).map(|p| p.base_url.to_string()))
        .ok_or_else(|| {
            AiError::ConfigError(format!(
                "provider '{provider}': no base_url configured and no builtin directory \
                 entry; set base_url = \"...\""
            ))
        })
}

/// Convert a static builtin reasoning declaration into the owned capability
/// used by the router.
pub fn to_reasoning_capability(r: &BuiltinReasoning) -> ReasoningCapability {
    ReasoningCapability {
        levels: r.levels.to_vec(),
        format: r.format,
        budget_tokens: r.budget_tokens,
    }
}
