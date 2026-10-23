export interface Account {
  id: string;
  name: string;
  bio: string;
  kind: string;
  status: "active" | "disabled";
}
export interface ManagedAi {
  account: Account;
  role: "initial" | "admin";
  bindingVersion: string;
  statusRevision: string;
  profileRevision: string;
  administratorsRevision: string;
  node: { id: string; name: string; online: boolean; lastSeenAt: string | null; capabilities: string[]; handoverState?: "active" | "draining" | "ready" } | null;
}
export interface AgentProfile {
  revision: string;
  persona: string;
  providerId: string;
  modelId: string;
  reasoningEffort: string | null;
  temperature: number | null;
  maxTokens: number | null;
  autoDailyTokens: number | null;
}
export interface AgentModelChoice {
  providerId: string;
  modelId: string;
  name: string;
  reasoningEfforts: string[];
  imageInput: "supported" | "unsupported" | "unknown";
}
export interface AgentManagementView { profile: AgentProfile; models: AgentModelChoice[] }
export type SaveAgentProfile = Omit<AgentProfile, "revision"> & { type: "saveProfile"; expectedRevision: string };
export interface Attachment {
  index: number;
  kind: string;
  name?: string;
  mime: string;
  byteLength: number;
}
export interface Message {
  sealed?: import("./crypto/content").Sealed | null;
  cryptoError?: string;
  local?: { synced: boolean };
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
  importOrigin?: { importId: string; sourceMessageId: string; sourceRevision: string } | null;
}
export interface Conversation {
  encryption?: { version: 1; activationPosition: string } | null;
  id: string;
  kind: "direct" | "group";
  name: string;
  peer: Account | null;
  members: { account: Account; role: "owner" | "admin" | "member" }[];
  revision: string;
  directBlocked: boolean;
  readOnly?: boolean;
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
export interface ParticipationPolicy {
  mode: "mentionedOnly" | "limitedAuto";
  directMergeMs: number;
  maxConsecutiveAi: number;
  maxBoutCalls: number;
  maxDailyCalls: number;
  idleSeconds: number;
}
export interface ContactSettings {
  revision: string;
  enabled: boolean;
  quietUntil: string | null;
  quietAction: "defer" | "skip";
}
export interface ContactIntent {
  id: string;
  creatorId: string;
  aiId: string;
  content: string;
  dueAt: string;
  expiresAt: string;
  utcOffsetMinutes: number;
  status: "pending" | "claimed" | "executing" | "delivered" | "cancelled" | "skipped" | "expired" | "failed" | "uncertain";
  revision: string;
  reason: string | null;
  taskId: string | null;
}
export interface ParticipationView {
  revision: string;
  canManage: boolean;
  policy: ParticipationPolicy;
  state: { paused: boolean; pausedAiIds: string[]; boutId: string | null; consecutiveAi: number; boutCalls: number; dailyCalls: number; reason: string | null };
  settings: ContactSettings;
  intents: ContactIntent[];
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
export interface SummaryItem { importOrigin?: string | null; id: string; revision: string; status: "queued" | "running" | "ready" | "failed" | "interrupted" | "stale"; text: string; fromPosition: string; throughPosition: string; sources: SourceRevision[]; error: string | null; createdAt: string; updatedAt: string }
export interface SummaryModelChoice { providerId: string; modelId: string; name: string; reasoningEfforts: string[] }
export interface ConsciousnessView { memories: MemoryItem[]; summaries: SummaryItem[]; settings: SummarySettings | null; models: SummaryModelChoice[] }
export type ConsciousnessOperation = { type: "memories" } | { type: "createMemory"; kind: MemoryKind; text: string; source: SourceRevision | null } | { type: "editMemory"; id: string; revision: string; kind: MemoryKind; text: string; source: SourceRevision | null } | { type: "forgetMemory"; id: string; revision: string } | { type: "summaries" } | { type: "configureSummaries"; revision: string; enabled: boolean; binding: SummaryModel | null } | { type: "generateSummary"; retryId: string | null };
