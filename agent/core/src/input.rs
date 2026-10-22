use social::*;

/// Permission-checked materials supplied by the host. No storage or credentials are read here.
#[derive(Debug, Clone)]
pub struct ContactInput {
    pub actor: Actor,
    pub profile: AiProfile,
    pub binding: Option<ExecutionBinding>,
}

#[derive(Debug, Clone)]
pub struct ContextInput {
    pub required_message_ids: Vec<MessageId>,
    pub reference: Option<ReferenceInput>,
    pub summaries: Vec<ConversationSummary>,
    pub memories: Vec<MemoryRecord>,
    pub contact: ContactInput,
    pub conversation: Conversation,
    pub members: Vec<Membership>,
    pub relationship_revision: Option<Revision>,
    /// Current public roster; other members' private profiles are not context.
    pub participant_names: Vec<String>,
    /// Recent committed history through the trigger, followed by completed answers to it.
    pub messages: Vec<SocialMessage>,
    pub trigger: ContextTrigger,
}

#[derive(Debug, Clone)]
pub enum ContextTrigger {
    Message(MessageId),
    Contact(ContactIntent),
}
impl ContextTrigger {
    pub fn message_id(&self) -> Option<MessageId> {
        match self {
            Self::Message(id) => Some(*id),
            Self::Contact(_) => None,
        }
    }
}
#[derive(Debug, Clone)]
pub struct ReferenceInputPreview {
    pub conversation: Conversation,
    pub messages: Vec<SocialMessage>,
    pub summary: Option<ConversationSummary>,
    pub dependencies: Vec<VisibilityDependency>,
}
#[derive(Debug, Clone)]
pub struct ReferenceInput {
    pub message_id: MessageId,
    pub revision: Revision,
    pub required: bool,
    pub preview: ReferenceInputPreview,
}
