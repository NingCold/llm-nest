export type ActorId = string
export type ConversationId = string
export type Revision = string
export type AvatarColor = "sage" | "clay" | "blue" | "lavender"

export interface AiContact {
  id: ActorId
  name: string
  avatar: string
  color: AvatarColor
  tagline: string
  persona: string
  interests: string[]
  responseStyle: "natural" | "concise" | "thoughtful"
  execution: "preview" | "unconfigured"
  allowProactive: boolean
  revision: Revision
}

export interface Conversation {
  id: ConversationId
  kind: "direct" | "group"
  relationshipId: string | null
  title: string
  memberIds: ActorId[]
  status: "active" | "archived"
  paused: boolean
  pinned: boolean
  unread: number
  bout: "active" | "waiting" | "ended"
  updatedAt: string
  revision: Revision
}

export interface ContextReference {
  conversationId: ConversationId
  messageId: string
  messageRevision: Revision
  actorId: ActorId
}

export interface SocialMessage {
  id: string
  conversationId: ConversationId
  senderId: ActorId
  senderName: string
  text: string
  mentions: ActorId[]
  replyTo: string | null
  contextReferences: ContextReference[]
  status: "complete" | "generating" | "cancelled" | "interrupted" | "redacted"
  createdAt: string
  revision: Revision
}

export interface AiRun {
  id: string
  conversationId: ConversationId
  actorId: ActorId
  messageId: string
  status: "running" | "succeeded" | "cancelled" | "interrupted"
}

export interface MemoryScope {
  kind: "relationship" | "conversation"
  id: string
}

export interface MemoryRecord {
  id: string
  scope: MemoryScope
  text: string
  status: "confirmed" | "forgotten" | "stale"
  sourceMessageId: string | null
  sourceRevision: Revision | null
  updatedAt: string
  revision: Revision
}

export interface ConversationSummary {
  id: string
  conversationId: ConversationId
  text: string
  sourceIds: string[]
  status: "valid" | "stale"
  updatedAt: string
}

export interface SocialSnapshot {
  schemaVersion: 1
  revision: Revision
  self: { id: ActorId; name: string }
  contacts: AiContact[]
  conversations: Conversation[]
  messages: SocialMessage[]
  memories: MemoryRecord[]
  summaries: ConversationSummary[]
  runs: AiRun[]
  commandReceipts: { clientRequestId: string; requestHash: string; messageId: string }[]
}

export type ContactInput = Omit<AiContact, "id" | "revision">
export interface SendMessageInput {
  clientRequestId: string
  conversationId: ConversationId
  text: string
  mentions: ActorId[]
  replyTo: string | null
  contextReferences: ContextReference[]
}

// The preview adapter owns persistence and simulation. Views only consume this API.
// This is the frontend contract; HTTP/Tauri adaptation follows in M1.
export interface SocialApi {
  init(): Promise<SocialSnapshot>
  subscribe(listener: (snapshot: SocialSnapshot, error?: string) => void): () => void
  createContact(input: ContactInput): Promise<AiContact>
  updateContact(id: ActorId, input: ContactInput, expectedRevision: Revision): Promise<void>
  getDirectConversation(actorId: ActorId): Promise<Conversation>
  createGroup(title: string, memberIds: ActorId[]): Promise<Conversation>
  updateConversation(id: ConversationId, changes: Partial<Pick<Conversation, "title" | "pinned" | "status">>): Promise<void>
  markRead(id: ConversationId): Promise<void>
  sendMessage(input: SendMessageInput): Promise<SocialMessage>
  editMessage(id: string, text: string, expectedRevision: Revision): Promise<void>
  redactMessage(id: string, expectedRevision: Revision): Promise<void>
  cancelRun(id: string): Promise<void>
  setPaused(id: ConversationId, paused: boolean): Promise<void>
  endBout(id: ConversationId): Promise<void>
  saveMemory(scope: MemoryScope, text: string, sourceMessageId?: string, id?: string, expectedRevision?: Revision): Promise<void>
  forgetMemory(id: string, expectedRevision: Revision): Promise<void>
  refreshSummary(id: ConversationId): Promise<void>
}

export function memoryScope(conversation: Conversation): MemoryScope {
  return conversation.kind === "direct"
    ? { kind: "relationship", id: conversation.relationshipId! }
    : { kind: "conversation", id: conversation.id }
}

export function sameScope(a: MemoryScope, b: MemoryScope): boolean {
  return a.kind === b.kind && a.id === b.id
}
