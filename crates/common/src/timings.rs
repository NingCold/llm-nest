use serde::{Deserialize, Serialize};

/// Per-message timing statistics, measured by the chat feature and persisted
/// with the assistant message so frontends can render them after reloads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageTimings {
    /// Request start → first token (reasoning or final answer), milliseconds.
    pub ttft_ms: Option<u64>,
    /// Request start → first final-answer delta, milliseconds. For thinking
    /// models this is the thinking phase plus transport; without a thinking
    /// chain it equals `ttft_ms`.
    pub reasoning_ms: Option<u64>,
    /// Request start → stream end, milliseconds.
    pub total_ms: Option<u64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timings_serde_roundtrip() {
        let t = MessageTimings {
            ttft_ms: Some(120),
            reasoning_ms: Some(120),
            total_ms: Some(3400),
        };
        let json = serde_json::to_string(&t).unwrap();
        let back: MessageTimings = serde_json::from_str(&json).unwrap();
        assert_eq!(back, t);
    }
}
