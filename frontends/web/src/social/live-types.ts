// Business DTOs from features/social/src/wire.rs; counters stay decimal strings.
export interface Binding { providerId: string; modelId: string; reasoningEffort: string | null; temperature: number | null; maxTokens: number | null }
export interface Account { id: string; kind: "human" | "ai"; name: string; avatarRef: string | null; bio: string; revision: string; status: "active" | "archived" }
export interface ContactEntry { ownerId: string; accountId: string; listed: boolean; revision: string }
export interface ManagedAi { account: Account; persona: string; binding: Binding | null; administrators: { accountId: string; initial: boolean }[]; isInitialAdministrator: boolean }
export interface AiInput { requestId: string; name: string; bio: string; persona: string; binding: Binding | null }
export interface ContactInput { requestId: string; listed: boolean; expectedRevision: string | null }
export interface AccountView { operatorId: string; snapshot: Snapshot }
export interface GenerationLimits { temperatureMax: number | null; minOutputTokens: number; maxOutputTokens: number | null }
export interface ModelOption { providerId: string; modelId: string; displayName: string; reasoningLevels: string[]; generationLimits: Record<string, GenerationLimits> }
export interface Message {
  id: string; conversationId: string; senderId: string; senderName: string
  sequence: string; text: string; status: "committed" | "generating" | "interrupted" | "withdrawn"
  revision: string; mentions: string[]; replyTo: string | null; runId: string | null; createdAt: string
}
export interface Run { id: string; conversationId: string; actorId: string; messageId: string; status: "running" | "succeeded" | "failed" | "cancelled" | "interrupted"; revision: string }
export interface Conversation {
  memoryOwnerIds: string[]
  id: string; kind: "direct" | "group"; title: string; memberIds: string[]; status: "active" | "archived"
  paused: boolean; revision: string; lastSequence: string; legacy: boolean; latestMessage: Message | null; activeRun: Run | null
  members: Membership[]; pendingPlans: ParticipationPlan[]; currentBout: { id: string; status: "open" | "ended"; revision: string } | null
}
export interface Membership { actorId: string; paused: boolean; revision: string }
export interface ParticipationPlan { id: string; actorId: string; triggerId: string; triggerSequence: string; ordinal: number; status: "queued" | "running" | "succeeded" | "failed" | "cancelled" | "interrupted"; runId: string | null; failure: string | null; revision: string }
export interface Snapshot { selfId: string; selfName: string; selfAccount: Account; accounts: Account[]; addressBook: ContactEntry[]; conversations: Conversation[]; models: ModelOption[]; cursor: string }
export interface Failure { code: string; message: string }
export interface RunDetails {
  provider: string; model: string; wireModel: string; protocol: string; reasoning: string
  usage: { prompt_tokens: number; completion_tokens: number; total_tokens: number; cached_tokens?: number | null } | null
  timings: { ttft_ms: number | null; reasoning_ms: number | null; total_ms: number | null } | null; error: Failure | null
}
export interface SendInput { requestId: string; text: string; replyTo: string | null; mentions: string[] }
export interface SendReceipt { message: Message; executionNotice: Failure | null }
export interface MessagePage { messages: Message[]; hasMore: boolean }
export interface Change { cursor: string; change: { kind: string; conversationId?: string; messageId?: string; memoryId?: string; runId?: string; actorId?: string; revision: string } }
export type LiveEvent = { type: "execution.notice"; conversationId: string; error: Failure }
  | { type: "generation.delta" | "generation.reasoning_delta"; conversationId: string; runId: string; messageId: string; actorId: string; deltaSeq: number; text: string }

export type ControlAction = "pause" | "resume" | "archive" | "restore" | "end"
export interface VersionedCommand { requestId: string; expectedRevision: string }
export type MemoryKind = "preference" | "fact" | "shared_experience" | "open_topic"
export interface MemoryContent { kind: MemoryKind; text: string; source: { messageId: string; revision: string } | null }
export interface Memory extends MemoryContent {
  id: string; actorId: string; conversationId: string; status: "confirmed" | "stale" | "forgotten"; revision: string
  createdBy: string; updatedBy: string; createdAt: string; updatedAt: string
}
