use serde::{Deserialize, Serialize};
use social::{
    ActorId, BoutId, ConversationId, EventCursor, MemoryId, MessageId, PlanId, Revision, RunId,
};

/// Durable changes contain references only. Consumers re-read current authorized
/// entities; replay never contains forgotten content or provider request copies.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum SocialChange {
    ConversationListChanged {
        conversation_id: ConversationId,
        actor_id: ActorId,
        revision: Revision,
    },
    ContactScheduleChanged {
        conversation_id: ConversationId,
    },
    ParticipationChanged {
        conversation_id: ConversationId,
    },
    SummariesChanged {
        conversation_id: ConversationId,
    },
    MemoryChanged {
        conversation_id: ConversationId,
        actor_id: ActorId,
        memory_id: MemoryId,
        revision: Revision,
    },
    ContactChanged {
        owner_id: ActorId,
        account_id: ActorId,
        revision: Revision,
    },
    PlanChanged {
        conversation_id: ConversationId,
        plan_id: PlanId,
        revision: Revision,
    },
    ActorChanged {
        actor_id: ActorId,
        revision: Revision,
    },
    ConversationChanged {
        conversation_id: ConversationId,
        revision: Revision,
    },
    MessageChanged {
        conversation_id: ConversationId,
        message_id: MessageId,
        revision: Revision,
    },
    RunChanged {
        conversation_id: ConversationId,
        run_id: RunId,
        revision: Revision,
    },
    BoutChanged {
        conversation_id: ConversationId,
        bout_id: BoutId,
        revision: Revision,
    },
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SocialEvent {
    pub cursor: EventCursor,
    pub change: SocialChange,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn wire_fields_and_large_cursors_follow_the_shared_contract() {
        let id = MessageId::new();
        let event = SocialEvent {
            cursor: EventCursor::new(9_007_199_254_740_993).unwrap(),
            change: SocialChange::MessageChanged {
                conversation_id: ConversationId::new(),
                message_id: id,
                revision: Revision::ONE,
            },
        };
        let mut wire = serde_json::to_value(&event).unwrap();
        assert_eq!(wire["cursor"], "9007199254740993");
        assert_eq!(wire["change"]["messageId"], id.to_string());
        assert!(wire["change"].get("message_id").is_none());
        assert_eq!(
            serde_json::from_value::<SocialEvent>(wire.clone()).unwrap(),
            event
        );
        wire["cursor"] = serde_json::json!(9007199254740993u64);
        assert!(serde_json::from_value::<SocialEvent>(wire).is_err());
    }
}
