//! Anthropic Messages API (`POST {base}/messages`) protocol implementation.
//!
//! Wire differences from the OpenAI family: authentication is `x-api-key`
//! plus `anthropic-version` (no Bearer), `max_tokens` is mandatory, system
//! prompts live in a separate `system` field (never as a message role), and
//! streaming events carry a `type` vocabulary (`content_block_delta`,
//! `message_stop`).

pub mod convert;
pub mod provider;
