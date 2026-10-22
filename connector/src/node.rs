//! Version one cloud node protocol. Identifiers and revisions are opaque strings.
use serde::{Deserialize, Serialize};

macro_rules! dto { ($name:ident { $($field:ident : $ty:ty),* $(,)? }) => {
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct $name { $(pub $field: $ty),* }
}; }
dto!(PairingRequest {
    node_id: String,
    node_name: String
});
dto!(PairingCreated {
    pairing_id: String,
    user_code: String,
    expires_at: String
});
dto!(PairingStatus { state: String, ai_id: Option<String>, binding_version: Option<String> });
dto!(NodeAccount {
    id: String,
    name: String,
    bio: String,
    kind: String
});
dto!(NodeStatus { id: String, name: String, online: bool, last_seen_at: Option<String> });
dto!(ManagedAi { account: NodeAccount, role: String, binding_version: String, node: Option<NodeStatus> });
dto!(NodeTask {
    id: String,
    ai_id: String,
    conversation_id: String,
    binding_version: String,
    revision: String,
    trigger_message_id: String,
    status: String,
    expires_at: String
});
dto!(NodeAttachment { index: usize, kind: String, mime: String, byte_length: u64, name: Option<String> });
dto!(NodeMessage { id: String, sender_id: String, text: String, revision: String, created_at: String, attachments: Vec<NodeAttachment> });
dto!(SourceRevision {
    message_id: String,
    revision: String
});
dto!(TaskMaterials { task: NodeTask, ai: NodeAccount, peer: NodeAccount, messages: Vec<NodeMessage>, sources: Vec<SourceRevision> });
dto!(StartRequest { request_id: String, task_revision: String, sources: Vec<SourceRevision>, configuration_revision: String });
dto!(StartReceipt {
    task_id: String,
    run_id: String,
    message_id: String,
    confirmation_id: String,
    binding_version: String
});
dto!(NodeUsage {
    input_tokens: u64,
    output_tokens: u64,
    cached_tokens: u64
});
dto!(ResultRequest { request_id: String, run_id: String, message_id: String, confirmation_id: String, status: String, text: String, error_code: Option<String>, usage: Option<NodeUsage> });
dto!(RejectRequest {
    request_id: String,
    task_revision: String,
    code: String,
    message: String
});

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ClientFrame {
    Hello {
        protocol_version: u32,
        node_id: String,
        ai_id: String,
        binding_version: String,
    },
    Delta {
        task_id: String,
        run_id: String,
        message_id: String,
        confirmation_id: String,
        sequence: u64,
        text: String,
    },
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ServerFrame {
    Ready { protocol_version: u32 },
    TasksChanged,
    Cancel { task_id: String },
    DeltaAccepted { task_id: String, sequence: u64 },
    Error { code: String, message: String },
}

mod transport;
pub use transport::{NodeClient, NodeConnection, NodeError};
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn frame_wire_names() {
        let frame = ClientFrame::Hello {
            protocol_version: 1,
            node_id: "node".into(),
            ai_id: "ai".into(),
            binding_version: "2".into(),
        };
        let json = serde_json::to_value(&frame).unwrap();
        assert_eq!(
            json,
            serde_json::json!({"type":"hello","protocolVersion":1,"nodeId":"node","aiId":"ai","bindingVersion":"2"})
        );
        assert_eq!(serde_json::from_value::<ClientFrame>(json).unwrap(), frame);
        assert_eq!(
            serde_json::to_value(ServerFrame::TasksChanged).unwrap(),
            serde_json::json!({"type":"tasksChanged"})
        );
    }
    #[test]
    fn transport_policy() {
        for endpoint in [
            "http://example.com",
            "ftp://localhost",
            "https://user:secret@example.com",
            "https://example.com/?token=secret",
        ] {
            assert!(NodeClient::new(endpoint, "secret", None).is_err());
        }
        for endpoint in [
            "http://localhost:1234",
            "http://127.0.0.1:1234",
            "http://[::1]:1234",
            "https://example.com",
        ] {
            assert!(NodeClient::new(endpoint, "secret", None).is_ok());
        }
        assert!(
            NodeClient::new("https://example.com", "secret", Some(b"not a certificate")).is_err()
        );
    }
    #[test]
    fn result_roundtrip_and_unknown_frame() {
        let result = ResultRequest {
            request_id: "request".into(),
            run_id: "run".into(),
            message_id: "message".into(),
            confirmation_id: "confirmation".into(),
            status: "uncertain".into(),
            text: "partial".into(),
            error_code: Some("transport_lost".into()),
            usage: Some(NodeUsage {
                input_tokens: 12,
                output_tokens: 3,
                cached_tokens: 4,
            }),
        };
        let json = serde_json::to_value(&result).unwrap();
        assert_eq!(json["usage"]["cachedTokens"], 4);
        assert_eq!(
            serde_json::from_value::<ResultRequest>(json).unwrap(),
            result
        );
        assert!(
            serde_json::from_value::<ServerFrame>(serde_json::json!({"type":"unknown"})).is_err()
        );
    }
}
