use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::role::Role;

#[derive(Debug, Clone, PartialEq)]
pub enum ContentPart {
    Text(String),
    Image { mime: Option<String>, data: Vec<u8> },
    File { mime: String, data: Vec<u8> },
}

/// Common content format used by OpenAI-compatible APIs:
///
/// Text-only message: `{ "role": ..., "content": "the text" }`
/// Multimodal message: `{ "role": ..., "content": [{ "type": "text", "text": ... }, ...] }`
#[derive(Debug, Clone, PartialEq)]
pub struct Message {
    pub role: Role,
    pub content: Vec<ContentPart>,
}

impl Message {
    pub fn new(role: Role, content: impl Into<Vec<ContentPart>>) -> Self {
        Self {
            role,
            content: content.into(),
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

    pub fn system(text: impl Into<String>) -> Self {
        Self::new(Role::System, vec![ContentPart::Text(text.into())])
    }

    pub fn user(text: impl Into<String>) -> Self {
        Self::new(Role::User, vec![ContentPart::Text(text.into())])
    }

    pub fn assistant(text: impl Into<String>) -> Self {
        Self::new(Role::Assistant, vec![ContentPart::Text(text.into())])
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

        let mut map = serializer.serialize_struct("Message", 2)?;
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

        map.end()
    }
}

fn serialize_block(part: &ContentPart) -> serde_json::Value {
    match part {
        ContentPart::Text(text) => serde_json::json!({ "type": "text", "text": text }),
        ContentPart::Image { mime, data } => {
            let mime = mime.clone().unwrap_or_else(|| "image/png".to_string());
            let b64 = base64_data_url(&mime, data);
            serde_json::json!({
                "type": "image_url",
                "image_url": { "url": b64 }
            })
        }
        ContentPart::File { .. } => serde_json::json!({}),
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
            role: Role,
            // `content` may be a string or an array of blocks.
            #[serde(deserialize_with = "de_content")]
            content: Vec<ContentPart>,
        }

        let raw = RawMessage::deserialize(deserializer)?;
        Ok(Self {
            role: raw.role,
            content: raw.content,
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
        Blocks(Vec<Block>),
    }

    #[derive(Deserialize)]
    struct Block {
        text: Option<String>,
    }

    let content = Content::deserialize(deserializer)?;
    match content {
        Content::Text(s) => Ok(vec![ContentPart::Text(s)]),
        Content::Blocks(blocks) => Ok(blocks
            .into_iter()
            .filter_map(|b| b.text.map(ContentPart::Text))
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
        };
        assert_eq!(msg.text(), "foo bar");
    }

    #[test]
    fn serialize_text_only_as_plain_string() {
        let msg = Message::user("hello");
        let json = serde_json::to_value(&msg).unwrap();
        assert_eq!(json["role"], "user");
        assert_eq!(json["content"], "hello");
    }

    #[test]
    fn serialize_multimodal_as_blocks() {
        let msg = Message {
            role: Role::User,
            content: vec![
                ContentPart::Text("see".into()),
                ContentPart::Image {
                    mime: Some("image/png".into()),
                    data: vec![1, 2, 3],
                },
            ],
        };
        let json = serde_json::to_value(&msg).unwrap();
        let arr = json["content"].as_array().unwrap();
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[0]["type"], "text");
        assert!(arr[1]["type"] == "image_url");
    }

    #[test]
    fn deserialize_string_content() {
        let json = r#"{"role":"assistant","content":"hello there"}"#;
        let msg: Message = serde_json::from_str(json).unwrap();
        assert_eq!(msg.role, Role::Assistant);
        assert_eq!(msg.text(), "hello there");
    }

    #[test]
    fn deserialize_blocks_content() {
        let json = r#"{"role":"assistant","content":[{"type":"text","text":"hi"}]}"#;
        let msg: Message = serde_json::from_str(json).unwrap();
        assert_eq!(msg.text(), "hi");
    }
}
