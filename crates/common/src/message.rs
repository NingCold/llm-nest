use serde::de::Error as DeError;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::role::Role;
use crate::timings::MessageTimings;
use crate::usage::Usage;

use base64::Engine as _;

/// User feedback on an assistant message (thumbs up/down). Persisted with
/// the message; `None` means no feedback yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Feedback {
    Up,
    Down,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    /// JSON arguments for the call.
    pub arguments: String,
    /// Gemini thought signature (`thoughtSignature`, 3.x / Gemma 4): the API
    /// REQUIRES echoing it back on the assistant functionCall part in the next
    /// request, otherwise the tool round-trip is rejected. Other protocols
    /// never set it (serde default keeps legacy records loadable).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thought_signature: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolResult {
    pub id: String,
    pub name: String,
    pub content: String,
    #[serde(default)]
    pub is_error: bool,
    /// Tool execution duration in milliseconds (measured by the chat feature
    /// around `ToolRegistry::run`). `None` for legacy records / demo paths.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ContentPart {
    Text(String),
    Image {
        mime: Option<String>,
        data: Vec<u8>,
    },
    File {
        mime: String,
        data: Vec<u8>,
    },
    /// Assistant request to run a tool. Persisted as part of the message;
    /// mapping to each protocol's wire form is the tool execution layer's job.
    ToolCall(ToolCall),
    /// Tool execution result, paired with its call by id.
    ToolResult(ToolResult),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Interruption {
    Cancelled,
    Failed(String),
}

/// Common content format used by OpenAI-compatible APIs:
///
/// Text-only message: `{ "role": ..., "content": "the text" }`
/// Multimodal message: `{ "role": ..., "content": [{ "type": "text", "text": ... }, ...] }`
#[derive(Debug, Clone, PartialEq)]
pub struct Message {
    pub id: Option<crate::MessageId>,
    /// Display-only interruption marker; incomplete answers are excluded from future prompts.
    pub interruption: Option<Interruption>,
    pub role: Role,
    pub content: Vec<ContentPart>,
    /// The model's thinking chain for this message. Rendered distinctly by
    /// frontends and persisted with the message, but **never sent to
    /// providers** (historical display only). Serde handled by the manual
    /// `Serialize`/`Deserialize` impls below.
    pub reasoning: Option<String>,
    /// Unix timestamp (seconds) when the message was created; persisted and
    /// shown by frontends. `None` for legacy records.
    pub created_at: Option<i64>,
    /// Thinking phase duration in milliseconds (request start → first final
    /// answer delta). Persisted per message; `None` when unknown.
    pub thinking_ms: Option<u64>,
    /// Token usage of the assistant turn. Persisted per message so frontends
    /// can show consumption without a separate store.
    pub usage: Option<Usage>,
    /// Timing statistics of the assistant turn (ttft / total).
    pub timings: Option<MessageTimings>,
    /// User feedback (up/down), persisted so history keeps it across reloads.
    pub feedback: Option<Feedback>,
}

impl Message {
    pub fn new(role: Role, content: impl Into<Vec<ContentPart>>) -> Self {
        Self {
            role,
            content: content.into(),
            reasoning: None,
            created_at: None,
            thinking_ms: None,
            usage: None,
            timings: None,
            feedback: None,
            interruption: None,
            id: Some(crate::MessageId::new()),
        }
    }

    pub fn text(&self) -> String {
        self.content
            .iter()
            .filter_map(|part| match part {
                ContentPart::Text(t) => Some(t.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("")
    }

    pub fn reasoning(&self) -> Option<&str> {
        self.reasoning.as_deref()
    }

    pub fn system(text: impl Into<String>) -> Self {
        Self::new(Role::System, vec![ContentPart::Text(text.into())])
    }

    pub fn user(text: impl Into<String>) -> Self {
        Self::new(Role::User, vec![ContentPart::Text(text.into())])
    }

    pub fn assistant(text: impl Into<String>) -> Self {
        Self::new(Role::Assistant, vec![ContentPart::Text(text.into())])
    }

    /// Assistant message carrying both the answer and the thinking chain
    /// (persisted so reasoning survives reloads).
    pub fn assistant_with_reasoning(text: impl Into<String>, reasoning: impl Into<String>) -> Self {
        Self {
            role: Role::Assistant,
            content: vec![ContentPart::Text(text.into())],
            reasoning: Some(reasoning.into()),
            created_at: None,
            thinking_ms: None,
            usage: None,
            timings: None,
            feedback: None,
            interruption: None,
            id: Some(crate::MessageId::new()),
        }
    }

    /// Wire form of this message: role and content only. Display metadata
    /// (`reasoning`, `created_at`, `thinking_ms`, `usage`, `timings`,
    /// `feedback`) is persisted with the message but must never reach a
    /// provider request.
    pub fn to_wire(&self) -> Self {
        Self {
            role: self.role,
            content: self.content.clone(),
            reasoning: None,
            created_at: None,
            thinking_ms: None,
            usage: None,
            timings: None,
            feedback: None,
            interruption: None,
            id: None,
        }
    }

    /// Wire JSON of this message, as a provider expects it: role plus content
    /// blocks in the OpenAI-compatible shape (text string when text-only,
    /// `image_url` data URLs for images, tool blocks as-is). The persisted
    /// form ([`Serialize`]) stores binary attachments losslessly instead.
    pub fn to_wire_value(&self) -> serde_json::Value {
        let is_text_only = self
            .content
            .iter()
            .all(|part| matches!(part, ContentPart::Text(_)));
        let content = if is_text_only {
            serde_json::Value::String(self.text())
        } else {
            serde_json::Value::Array(self.content.iter().map(wire_block).collect())
        };
        serde_json::json!({ "role": self.role, "content": content })
    }

    pub fn tool(text: impl Into<String>) -> Self {
        Self::new(Role::Tool, vec![ContentPart::Text(text.into())])
    }

    pub fn developer(text: impl Into<String>) -> Self {
        Self::new(Role::Developer, vec![ContentPart::Text(text.into())])
    }
}

//

impl Serialize for Message {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        use serde::ser::SerializeStruct;

        let mut map = serializer.serialize_struct("Message", 3)?;
        if let Some(id) = self.id {
            map.serialize_field("id", &id)?;
        }
        map.serialize_field("role", &self.role)?;

        let is_text_only = self
            .content
            .iter()
            .all(|part| matches!(part, ContentPart::Text(_)));

        if is_text_only {
            // OpenAI text-only message: content is a plain string.
            map.serialize_field("content", &self.text())?;
        } else {
            // Multimodal: content is an array of content blocks.
            let blocks = self.content.iter().map(serialize_block).collect::<Vec<_>>();
            map.serialize_field("content", &blocks)?;
        }

        // Thinking chains are persisted with the message (so they survive
        // reloads) but never sent to providers (skip when absent).
        if let Some(reasoning) = &self.reasoning {
            map.serialize_field("reasoning", reasoning)?;
        }
        // Display metadata persisted alongside the message; absent fields are
        // skipped so provider wire JSON stays clean.
        if let Some(created_at) = self.created_at {
            map.serialize_field("created_at", &created_at)?;
        }
        if let Some(thinking_ms) = self.thinking_ms {
            map.serialize_field("thinking_ms", &thinking_ms)?;
        }
        if let Some(usage) = &self.usage {
            map.serialize_field("usage", usage)?;
        }
        if let Some(timings) = &self.timings {
            map.serialize_field("timings", timings)?;
        }
        if let Some(feedback) = self.feedback {
            map.serialize_field("feedback", &feedback)?;
        }

        if let Some(interruption) = &self.interruption {
            map.serialize_field("interruption", interruption)?;
        }
        map.end()
    }
}

/// Persisted block form: binary attachments are stored **losslessly** (base64
/// in a dedicated `image`/`file` block) so history survives reloads. The wire
/// form differs — see [`wire_block`].
fn serialize_block(part: &ContentPart) -> serde_json::Value {
    match part {
        ContentPart::Text(text) => serde_json::json!({ "type": "text", "text": text }),
        ContentPart::Image { mime, data } => serde_json::json!({
            "type": "image",
            "mime": mime.clone().unwrap_or_else(|| "image/png".to_string()),
            "data": base64::engine::general_purpose::STANDARD.encode(data),
        }),
        ContentPart::File { mime, data } => serde_json::json!({
            "type": "file",
            "mime": mime.clone(),
            "data": base64::engine::general_purpose::STANDARD.encode(data),
        }),
        ContentPart::ToolCall(tc) => {
            let mut v = serde_json::json!({
                "type": "tool_call",
                "id": tc.id,
                "name": tc.name,
                "arguments": tc.arguments,
            });
            if let Some(ts) = &tc.thought_signature {
                v["thought_signature"] = ts.as_str().into();
            }
            v
        }
        ContentPart::ToolResult(tr) => serde_json::json!({
            "type": "tool_result",
            "id": tr.id,
            "name": tr.name,
            "content": tr.content,
            "is_error": tr.is_error,
            "duration_ms": tr.duration_ms,
        }),
    }
}

/// Provider wire block form (OpenAI-compatible): images become `image_url`
/// data URLs; file attachments cannot be sent yet and serialize to `{}`.
fn wire_block(part: &ContentPart) -> serde_json::Value {
    match part {
        ContentPart::Text(text) => serde_json::json!({ "type": "text", "text": text }),
        ContentPart::Image { mime, data } => {
            let mime = mime.clone().unwrap_or_else(|| "image/png".to_string());
            serde_json::json!({
                "type": "image_url",
                "image_url": { "url": base64_data_url(&mime, data) }
            })
        }
        ContentPart::File { .. } => serde_json::json!({}),
        ContentPart::ToolCall(tc) => serde_json::json!({
            "type": "tool_call",
            "id": tc.id,
            "name": tc.name,
            "arguments": tc.arguments,
        }),
        ContentPart::ToolResult(tr) => serde_json::json!({
            "type": "tool_result",
            "id": tr.id,
            "name": tr.name,
            "content": tr.content,
            "is_error": tr.is_error,
            "duration_ms": tr.duration_ms,
        }),
    }
}

fn base64_data_url(mime: &str, data: &[u8]) -> String {
    use base64::Engine as _;
    use base64::engine::general_purpose::STANDARD;
    format!("data:{mime};base64,{}", STANDARD.encode(data))
}

impl<'de> Deserialize<'de> for Message {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct RawMessage {
            #[serde(default)]
            id: Option<crate::MessageId>,
            role: Role,
            // `content` may be a string or an array of blocks.
            #[serde(deserialize_with = "de_content")]
            content: Vec<ContentPart>,
            // Older persisted messages have no reasoning field.
            #[serde(default)]
            reasoning: Option<String>,
            #[serde(default)]
            created_at: Option<i64>,
            #[serde(default)]
            thinking_ms: Option<u64>,
            #[serde(default)]
            usage: Option<Usage>,
            #[serde(default)]
            timings: Option<MessageTimings>,
            #[serde(default)]
            feedback: Option<Feedback>,
            #[serde(default)]
            interruption: Option<Interruption>,
        }

        let raw = RawMessage::deserialize(deserializer)?;
        Ok(Self {
            role: raw.role,
            content: raw.content,
            reasoning: raw.reasoning,
            created_at: raw.created_at,
            thinking_ms: raw.thinking_ms,
            usage: raw.usage,
            timings: raw.timings,
            feedback: raw.feedback,
            interruption: raw.interruption,
            id: raw.id,
        })
    }
}

fn de_content<'de, D>(deserializer: D) -> Result<Vec<ContentPart>, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Content {
        Text(String),
        Blocks(Vec<serde_json::Value>),
    }

    let content = Content::deserialize(deserializer)?;
    let decode = |b: &serde_json::Value| -> Result<Vec<u8>, D::Error> {
        let s = b
            .as_str()
            .ok_or_else(|| D::Error::custom("expected base64 string"))?;
        base64::engine::general_purpose::STANDARD
            .decode(s)
            .map_err(|e| D::Error::custom(format!("invalid base64 data: {e}")))
    };
    match content {
        Content::Text(s) => Ok(vec![ContentPart::Text(s)]),
        Content::Blocks(blocks) => Ok(blocks
            .iter()
            .filter_map(|b| {
                let kind = b.get("type")?.as_str()?;
                match kind {
                    "text" => Some(ContentPart::Text(b.get("text")?.as_str()?.to_string())),
                    // lossless persisted form
                    "image" => {
                        let data = decode(b.get("data")?).ok()?;
                        let mime = b.get("mime").and_then(|m| m.as_str()).map(str::to_string);
                        Some(ContentPart::Image { mime, data })
                    }
                    "file" => {
                        let data = decode(b.get("data")?).ok()?;
                        let mime = b
                            .get("mime")
                            .and_then(|m| m.as_str())
                            .unwrap_or("")
                            .to_string();
                        Some(ContentPart::File { mime, data })
                    }
                    // legacy persisted form: wire-style image_url data URLs
                    "image_url" => {
                        let url = b.get("image_url")?.get("url")?.as_str()?;
                        let (meta, b64) = url.split_once(',')?;
                        let mime = meta
                            .strip_prefix("data:")
                            .and_then(|m| m.split(';').next())
                            .filter(|m| !m.is_empty())
                            .map(str::to_string);
                        let data = base64::engine::general_purpose::STANDARD.decode(b64).ok()?;
                        Some(ContentPart::Image { mime, data })
                    }
                    "tool_call" => Some(ContentPart::ToolCall(ToolCall {
                        id: b.get("id")?.as_str()?.to_string(),
                        name: b.get("name")?.as_str()?.to_string(),
                        arguments: b.get("arguments")?.as_str()?.to_string(),
                        thought_signature: b
                            .get("thought_signature")
                            .and_then(|v| v.as_str().map(str::to_string)),
                    })),
                    "tool_result" => Some(ContentPart::ToolResult(ToolResult {
                        id: b.get("id")?.as_str()?.to_string(),
                        name: b.get("name")?.as_str()?.to_string(),
                        content: b.get("content")?.as_str()?.to_string(),
                        is_error: b.get("is_error").and_then(|v| v.as_bool()).unwrap_or(false),
                        duration_ms: b.get("duration_ms").and_then(|v| v.as_u64()),
                    })),
                    // unknown blocks stay dropped (lossy by design)
                    _ => None,
                }
            })
            .collect()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn message_new_sets_role_and_content() {
        let msg = Message::new(Role::User, vec![ContentPart::Text("hello".into())]);
        assert_eq!(msg.role, Role::User);
        assert_eq!(msg.text(), "hello");
    }

    #[test]
    fn message_system_constructor() {
        let msg = Message::system("system prompt");
        assert_eq!(msg.role, Role::System);
        assert_eq!(msg.text(), "system prompt");
    }

    #[test]
    fn message_user_constructor() {
        let msg = Message::user("user message");
        assert_eq!(msg.role, Role::User);
    }

    #[test]
    fn message_assistant_constructor() {
        let msg = Message::assistant("assistant reply");
        assert_eq!(msg.role, Role::Assistant);
    }

    #[test]
    fn message_developer_constructor() {
        let msg = Message::developer("developer instruction");
        assert_eq!(msg.role, Role::Developer);
    }

    #[test]
    fn message_accepts_string_slice_and_string() {
        let _ = Message::user("&str");
        let _ = Message::user(String::from("String"));
    }

    #[test]
    fn text_joins_all_text_parts_ignoring_binary() {
        let msg = Message {
            role: Role::User,
            content: vec![
                ContentPart::Text("foo ".into()),
                ContentPart::Image {
                    mime: None,
                    data: vec![1],
                },
                ContentPart::Text("bar".into()),
            ],
            reasoning: None,
            created_at: None,
            thinking_ms: None,
            usage: None,
            timings: None,
            feedback: None,
            interruption: None,
            id: Some(crate::MessageId::new()),
        };
        assert_eq!(msg.text(), "foo bar");
    }

    #[test]
    fn serialize_text_only_as_plain_string() {
        let msg = Message::user("hello");
        let json = serde_json::to_value(&msg).unwrap();
        assert_eq!(json["role"], "user");
        assert_eq!(json["content"], "hello");
        // reasoning absent → never leaks into provider wire JSON
        assert!(json.get("reasoning").is_none());
    }

    #[test]
    fn serialize_multimodal_as_lossless_blocks() {
        let msg = Message {
            role: Role::User,
            content: vec![
                ContentPart::Text("see".into()),
                ContentPart::Image {
                    mime: Some("image/png".into()),
                    data: vec![1, 2, 3],
                },
            ],
            reasoning: None,
            created_at: None,
            thinking_ms: None,
            usage: None,
            timings: None,
            feedback: None,
            interruption: None,
            id: Some(crate::MessageId::new()),
        };
        // persisted form stores the binary losslessly (dedicated image block)
        let json = serde_json::to_value(&msg).unwrap();
        let arr = json["content"].as_array().unwrap();
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[0]["type"], "text");
        assert_eq!(arr[1]["type"], "image");
        assert_eq!(arr[1]["mime"], "image/png");
        assert_eq!(
            arr[1]["data"].as_str().unwrap(),
            base64::engine::general_purpose::STANDARD.encode([1, 2, 3])
        );
        // roundtrip restores the exact bytes
        let back: Message = serde_json::from_value(json).unwrap();
        assert_eq!(back, msg);

        // wire form converts to image_url data URLs
        let wire = msg.to_wire_value();
        let blocks = wire["content"].as_array().unwrap();
        assert_eq!(blocks[1]["type"], "image_url");
        assert!(
            blocks[1]["image_url"]["url"]
                .as_str()
                .unwrap()
                .starts_with("data:image/png;base64,")
        );
    }

    #[test]
    fn deserialize_string_content() {
        let json = r#"{"role":"assistant","content":"hello there"}"#;
        let msg: Message = serde_json::from_str(json).unwrap();
        assert_eq!(msg.role, Role::Assistant);
        assert_eq!(msg.text(), "hello there");
        assert_eq!(msg.reasoning(), None);
    }

    #[test]
    fn deserialize_blocks_content() {
        let json = r#"{"role":"assistant","content":[{"type":"text","text":"hi"}]}"#;
        let msg: Message = serde_json::from_str(json).unwrap();
        assert_eq!(msg.text(), "hi");
    }

    #[test]
    fn reasoning_roundtrip_and_wire_isolation() {
        let msg = Message::assistant_with_reasoning("answer", "think think");
        assert_eq!(msg.text(), "answer");
        assert_eq!(msg.reasoning(), Some("think think"));

        // persisted form keeps reasoning
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("\"reasoning\":\"think think\""));
        let back: Message = serde_json::from_str(&json).unwrap();
        assert_eq!(back, msg);

        // no reasoning → field absent, provider wire JSON unchanged
        let plain = Message::assistant("answer");
        let json = serde_json::to_string(&plain).unwrap();
        assert!(!json.contains("reasoning"));
    }

    #[test]
    fn tool_call_and_result_roundtrip() {
        let msg = Message {
            role: Role::Assistant,
            content: vec![
                ContentPart::Text("calling".into()),
                ContentPart::ToolCall(ToolCall {
                    id: "call_1".into(),
                    name: "web_search".into(),
                    arguments: r#"{"query":"rust"}"#.into(),
                    thought_signature: Some("sig-echo".into()),
                }),
            ],
            reasoning: Some("need to search".into()),
            created_at: None,
            thinking_ms: None,
            usage: None,
            timings: None,
            feedback: None,
            interruption: None,
            id: Some(crate::MessageId::new()),
        };
        // text() never includes tool parts
        assert_eq!(msg.text(), "calling");

        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("\"type\":\"tool_call\""));
        let back: Message = serde_json::from_str(&json).unwrap();
        assert_eq!(back, msg);
        assert_eq!(
            back.content[1],
            ContentPart::ToolCall(ToolCall {
                id: "call_1".into(),
                name: "web_search".into(),
                arguments: r#"{"query":"rust"}"#.into(),
                thought_signature: Some("sig-echo".into()),
            })
        );

        // tool result message roundtrips including is_error and duration
        let result = Message {
            role: Role::Tool,
            content: vec![ContentPart::ToolResult(ToolResult {
                id: "call_1".into(),
                name: "web_search".into(),
                content: "3 results".into(),
                is_error: true,
                duration_ms: Some(42),
            })],
            reasoning: None,
            created_at: None,
            thinking_ms: None,
            usage: None,
            timings: None,
            feedback: None,
            interruption: None,
            id: Some(crate::MessageId::new()),
        };
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("\"is_error\":true"));
        assert!(json.contains("\"duration_ms\":42"));
        let back: Message = serde_json::from_str(&json).unwrap();
        assert_eq!(back, result);

        // legacy records without duration default to None (zero migration)
        let legacy: Message = serde_json::from_str(
            r#"{"role":"tool","content":[{"type":"tool_result","id":"c","name":"n","content":"x","is_error":false}]}"#,
        )
        .unwrap();
        match &legacy.content[0] {
            ContentPart::ToolResult(tr) => assert_eq!(tr.duration_ms, None),
            other => panic!("expected ToolResult, got {other:?}"),
        }
    }

    #[test]
    fn unknown_blocks_stay_dropped_but_legacy_image_url_restored() {
        // legacy persisted form: wire-style image_url data URLs are restored
        let json = r#"{"role":"user","content":[{"type":"image_url","image_url":{"url":"data:image/png;base64,eA=="}},{"type":"text","text":"keep"}]}"#;
        let msg: Message = serde_json::from_str(json).unwrap();
        assert_eq!(msg.text(), "keep");
        assert_eq!(msg.content.len(), 2);
        match &msg.content[0] {
            ContentPart::Image { mime, data } => {
                assert_eq!(mime.as_deref(), Some("image/png"));
                assert_eq!(data, &[120]); // "eA==" decodes to 0x78
            }
            other => panic!("expected restored Image, got {other:?}"),
        }

        // truly unknown block types stay dropped
        let json = r#"{"role":"user","content":[{"type":"video_url","video_url":{"url":"x"}},{"type":"text","text":"keep"}]}"#;
        let msg: Message = serde_json::from_str(json).unwrap();
        assert_eq!(msg.text(), "keep");
        assert_eq!(msg.content.len(), 1);
    }

    #[test]
    fn display_metadata_roundtrips_but_stays_off_the_wire() {
        let mut msg = Message::assistant("answer");
        msg.created_at = Some(1_720_000_000);
        msg.thinking_ms = Some(850);
        msg.usage = Some(Usage {
            prompt_tokens: 10,
            completion_tokens: 20,
            total_tokens: 30,
            cached_tokens: 5,
        });
        msg.timings = Some(MessageTimings {
            ttft_ms: Some(100),
            reasoning_ms: Some(850),
            total_ms: Some(2000),
        });

        // persisted form keeps everything
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("\"created_at\":1720000000"));
        assert!(json.contains("\"thinking_ms\":850"));
        assert!(json.contains("\"cached_tokens\":5"));
        let back: Message = serde_json::from_str(&json).unwrap();
        assert_eq!(back, msg);

        // legacy json (no metadata) defaults to None
        let legacy: Message =
            serde_json::from_str(r#"{"role":"assistant","content":"hi"}"#).unwrap();
        assert_eq!(legacy.created_at, None);
        assert_eq!(legacy.usage, None);

        // wire form strips every display field
        let wire = msg.to_wire();
        assert_eq!(wire.content, msg.content);
        assert_eq!(wire.reasoning, None);
        assert_eq!(wire.created_at, None);
        assert_eq!(wire.thinking_ms, None);
        assert_eq!(wire.usage, None);
        assert_eq!(wire.timings, None);
    }
    #[test]
    fn interruption_roundtrip_and_wire_isolation() {
        let legacy: Message =
            serde_json::from_str(r#"{"role":"assistant","content":"old"}"#).unwrap();
        assert!(legacy.interruption.is_none());
        let mut partial = Message::assistant_with_reasoning("partial", "thinking");
        partial.interruption = Some(Interruption::Failed("connection lost".into()));
        let decoded: Message =
            serde_json::from_str(&serde_json::to_string(&partial).unwrap()).unwrap();
        assert_eq!(decoded, partial);
        assert!(decoded.to_wire().interruption.is_none());
    }
}
