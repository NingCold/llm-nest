//! OpenAI Responses API (`/v1/responses`) protocol implementation.
//!
//! Wire differences from chat completions: the conversation is `input` (not
//! `messages`), the output cap is `max_output_tokens`, reasoning dispatches
//! through `reasoning: { effort }`, and stream events are
//! `response.output_text.delta` / `response.completed`.

pub mod convert;
pub mod provider;
pub mod stream;
