export interface Account {
  id: string;
  name: string;
  bio: string;
  kind: string;
  status: "active" | "disabled";
}
export interface Attachment {
  index: number;
  kind: string;
  name?: string;
  mime: string;
  byteLength: number;
}
export interface Message {
  id: string;
  conversationId: string;
  senderId: string;
  text: string;
  revision: string;
  position: string;
  status: string;
  createdAt: string;
  attachments: Attachment[];
  senderName: string;
  replyTo: string | null;
  mentions: string[];
  contextReference: (ReferenceSelection & { revision: string; stale: boolean }) | null;
  aiResponses: { aiId: string; taskId: string; status: string; error?: string | null }[];
}
export interface Conversation {
  id: string;
  kind: "direct" | "group";
  name: string;
  peer: Account | null;
  members: { account: Account; role: "owner" | "admin" | "member" }[];
  revision: string;
  directBlocked: boolean;
  lastMessage: Message | null;
  unreadCount: number;
  manualUnread: boolean;
  pinned: boolean;
  listRevision: string;
  latestPosition: string;
}
export interface Snapshot {
  self: Account;
  contacts: Account[];
  conversations: Conversation[];
  cursor: string;
}
export interface Page {
  messages: Message[];
  hasMore: boolean;
  latestPosition: string;
}

export interface SourceRevision { messageId: string; revision: string }
export interface ReferenceSelection { conversationId: string; sources: SourceRevision[] }
export interface DraftReference { actorId: string; selection: ReferenceSelection }
export type CloudDraftRecord = import("../social/drafts").DraftRecord<DraftReference>;
export interface NodeConversation { id: string; kind: "direct" | "group"; name: string; revision: string; members: Account[] }
export interface ReferenceMessage { id: string; senderId: string; senderName: string; text: string; revision: string; createdAt: string; position: string; attachments: Attachment[]; replyTo: string | null; mentions: string[] }
export interface ReferencePreview { conversation: NodeConversation; messages: ReferenceMessage[] }
export type MemoryKind = "preference" | "fact" | "shared_experience" | "open_topic";
export interface MemoryItem { id: string; revision: string; kind: MemoryKind; text: string; status: "confirmed" | "stale" | "forgotten"; source: SourceRevision | null; createdAt: string; updatedAt: string }
export interface SummaryModel { providerId: string; modelId: string; reasoningEffort: string | null; maxTokens: number }
export interface SummarySettings { revision: string; enabled: boolean; binding: SummaryModel | null }
export interface SummaryItem { id: string; revision: string; status: "queued" | "running" | "ready" | "failed" | "interrupted" | "stale"; text: string; fromPosition: string; throughPosition: string; sources: SourceRevision[]; error: string | null; createdAt: string; updatedAt: string }
export interface SummaryModelChoice { providerId: string; modelId: string; name: string; reasoningEfforts: string[] }
export interface ConsciousnessView { memories: MemoryItem[]; summaries: SummaryItem[]; settings: SummarySettings | null; models: SummaryModelChoice[] }
export type ConsciousnessOperation = { type: "memories" } | { type: "createMemory"; kind: MemoryKind; text: string; source: SourceRevision | null } | { type: "editMemory"; id: string; revision: string; kind: MemoryKind; text: string; source: SourceRevision | null } | { type: "forgetMemory"; id: string; revision: string } | { type: "summaries" } | { type: "configureSummaries"; revision: string; enabled: boolean; binding: SummaryModel | null } | { type: "generateSummary"; retryId: string | null };
