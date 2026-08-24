use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Usage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
    /// Cache-hit prompt tokens (normalized across providers: OpenAI
    /// `prompt_tokens_details.cached_tokens`, Anthropic
    /// `cache_read_input_tokens + cache_creation_input_tokens`, Gemini
    /// `cachedContentTokenCount`). Cache hit rate is
    /// `cached / (prompt + cached)`.
    #[serde(default)]
    pub cached_tokens: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn usage_serde_roundtrip() {
        let usage = Usage {
            prompt_tokens: 10,
            completion_tokens: 20,
            total_tokens: 30,
            cached_tokens: 4,
        };
        let json = serde_json::to_string(&usage).unwrap();
        let deserialized: Usage = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.prompt_tokens, 10);
        assert_eq!(deserialized.completion_tokens, 20);
        assert_eq!(deserialized.total_tokens, 30);
        assert_eq!(deserialized.cached_tokens, 4);
    }

    #[test]
    fn usage_legacy_json_defaults_cached_tokens() {
        let json = r#"{"prompt_tokens":1,"completion_tokens":2,"total_tokens":3}"#;
        let usage: Usage = serde_json::from_str(json).unwrap();
        assert_eq!(usage.cached_tokens, 0);
    }
}
