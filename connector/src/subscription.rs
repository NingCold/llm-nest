use serde::{Deserialize, Serialize};

pub use crate::SocialGenerationEvent as LiveEvent;

/// Durable references, transient generation fragments, or a request to resync.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SubscriptionEvent {
    Change { event: crate::SocialEvent },
    Generation { event: LiveEvent },
    Resync,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn subscription_variants_decode_and_preserve_the_wire_contract() {
        let id = "00000000-0000-4000-8000-000000000001";
        let samples = [
            json!({"kind": "change", "event": {
                "cursor": "9007199254740993", "change": {
                    "kind": "message_changed", "conversationId": id,
                    "messageId": id, "revision": "1"
                }
            }}),
            json!({"kind": "generation", "event": {
                "type": "generation.delta", "runId": id, "messageId": id,
                "conversationId": id, "actorId": id, "deltaSeq": 3, "text": "你好"
            }}),
            json!({"kind": "generation", "event": {
                "type": "generation.reasoning_delta", "runId": id, "messageId": id,
                "conversationId": id, "actorId": id, "deltaSeq": 4, "text": "思考"
            }}),
            json!({"kind": "generation", "event": {
                "type": "execution.notice", "conversationId": id,
                "error": {"code": "model_unavailable", "message": "配置模型"}
            }}),
            json!({"kind": "resync"}),
        ];
        for sample in samples {
            let event: SubscriptionEvent = serde_json::from_value(sample.clone()).unwrap();
            assert_eq!(serde_json::to_value(event).unwrap(), sample);
        }
    }

    #[test]
    fn subscription_rejects_a_numeric_durable_cursor() {
        let event = json!({"kind": "change", "event": {
            "cursor": 9007199254740993u64,
            "change": {"kind": "participation_changed",
                "conversationId": "00000000-0000-4000-8000-000000000001"}
        }});
        assert!(serde_json::from_value::<SubscriptionEvent>(event).is_err());
    }
}
