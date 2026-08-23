//! Google Gemini API (`generativelanguage.googleapis.com`) protocol
//! implementation.
//!
//! Wire differences from the OpenAI family: the model name lives in the URL
//! path (`/models/{model}:generateContent` / `:streamGenerateContent`),
//! messages are `contents` with `parts`, assistant turns use the role `model`,
//! system prompts go to `systemInstruction`, and output caps live in
//! `generationConfig.maxOutputTokens`. Reasoning dispatches through
//! `generationConfig.thinkingConfig.thinkingBudget`.

pub mod convert;
pub mod provider;
