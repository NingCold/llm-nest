export interface AvatarStyle { avatar: string; color: "sage" | "clay" | "blue" | "lavender" }
export interface LegacyPreview {
  sessions: { source_id: string; session_id: string; checksum: string; title: string | null; message_count: number; missing_ids: number; interrupted_checkpoint: boolean }[]
}
// Business DTOs from features/social/src/wire.rs; counters stay decimal strings.
export interface Binding { providerId: string; modelId: string; reasoningEffort: string | null; temperature: number | null; maxTokens: number | null }
export interface Account { id: string; kind: "human" | "ai"; name: string; avatarRef: string | null; bio: string; revision: string; status: "active" | "archived" }
export interface ContactEntry { ownerId: string; accountId: string; listed: boolean; blocked: boolean; revision: string }
export interface ManagedAi { account: Account; persona: string; binding: Binding | null; administrators: { accountId: string; initial: boolean }[]; isInitialAdministrator: boolean }
export interface AiInput { requestId: string; name: string; bio: string; persona: string; binding: Binding | null }
export interface ContactBlockInput { requestId: string; blocked: boolean; expectedRevision: string | null }
export interface ContactInput { requestId: string; listed: boolean; expectedRevision: string | null }
export interface AccountView { operatorId: string; snapshot: Snapshot }
export interface GenerationLimits { temperatureMax: number | null; minOutputTokens: number; maxOutputTokens: number | null }
export interface ModelOption { providerId: string; modelId: string; displayName: string; reasoningLevels: string[]; generationLimits: Record<string, GenerationLimits> }
export interface Message {
  contextReference?: ContextReference | null
  id: string; conversationId: string; senderId: string; senderName: string
  sequence: string; text: string; status: "committed" | "generating" | "interrupted" | "withdrawn"
  revision: string; mentions: string[]; replyTo: string | null; runId: string | null; createdAt: string
}
export interface Run { id: string; conversationId: string; actorId: string; messageId: string; status: "running" | "succeeded" | "failed" | "cancelled" | "interrupted"; revision: string }
export interface Conversation {
  directBlocked: boolean
  listState: ConversationListState
  contactSchedule: { settings: ContactSettings; intents: ContactIntent[] }
  participation: ParticipationView
  memoryOwnerIds: string[]
  id: string; kind: "direct" | "group"; title: string; memberIds: string[]; status: "active" | "archived"
  paused: boolean; revision: string; lastSequence: string; legacy: boolean; latestMessage: Message | null; activeRun: Run | null
  members: Membership[]; pendingPlans: ParticipationPlan[]; currentBout: { id: string; status: "open" | "ended"; revision: string } | null
}
export interface Membership { actorId: string; paused: boolean; revision: string }
export interface ParticipationPlan { contactIntentId: string | null; automatic: boolean; inputIds: string[]; notBefore: string | null; expiresAt: string | null; id: string; actorId: string; triggerId: string | null; triggerSequence: string; ordinal: number; status: "queued" | "running" | "succeeded" | "failed" | "cancelled" | "interrupted"; runId: string | null; failure: string | null; revision: string }
export interface ContactSettings { revision: string; enabled: boolean; quietUntil: string | null; quietAction: "defer" | "skip" }
export interface ContactIntentInput { requestId: string; actorId: string; text: string; dueAt: string; expiresAt: string }
export type ContactIntentStatus = "pending" | "claimed" | "executing" | "delivered" | "failed" | "cancelled" | "expired" | "skipped" | "uncertain"
export interface ContactIntent { id: string; conversationId: string; actorId: string; createdBy: string; authorizationId: string; text: string; dueAt: string; expiresAt: string; utcOffsetMinutes: number; status: ContactIntentStatus; planId: string | null; revision: string; reason: string | null; createdAt: string; updatedAt: string }
export interface ParticipationPolicy { revision: string; mergeDelayMs: number; idleSeconds: number; limitedAuto: boolean; maxConsecutive: number; maxAutoPerBout: number; maxAutoPerDay: number; dailyTokenBudget: number }
export type ParticipationReason = "waiting" | "input_pending" | "turn_limit" | "daily_limit" | "token_limit" | "no_candidate" | "expired" | "idle" | "user_ended" | "policy_changed"
export interface ParticipationView { policy: ParticipationPolicy; state: { lastActivity: string | null; consecutiveAi: number; boutAutoTurns: number; reason: ParticipationReason | null }; budgetDay: string; autoTurnsToday: number; reservedTokensToday: number }
export interface Snapshot { selfId: string; selfName: string; selfAccount: Account; accounts: Account[]; addressBook: ContactEntry[]; conversations: Conversation[]; models: ModelOption[]; cursor: string }
export interface Failure { code: string; message: string }
export interface RunDetails {
  provider: string; model: string; wireModel: string; protocol: string; reasoning: string
  usage: { prompt_tokens: number; completion_tokens: number; total_tokens: number; cached_tokens?: number | null } | null
  timings: { ttft_ms: number | null; reasoning_ms: number | null; total_ms: number | null } | null; error: Failure | null
}
export interface SendInput { reference?: ReferenceSelection | null; requestId: string; text: string; replyTo: string | null; mentions: string[] }
export interface SendReceipt { message: Message; executionNotice: Failure | null }
export interface MessagePage { messages: Message[]; hasMore: boolean; readThrough: string | null }
export interface Change { cursor: string; change: { kind: string; conversationId?: string; messageId?: string; memoryId?: string; runId?: string; actorId?: string; revision?: string } }
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

export interface SummaryBinding { providerId: string; modelId: string; reasoningEffort: string | null; maxTokens: number }
export interface SummarySettings { id: string; enabled: boolean; binding: SummaryBinding | null; revision: string }
export interface Summary {
  id: string; conversationId: string; fromSequence: string; throughSequence: string; text: string
  status: "queued" | "running" | "ready" | "failed" | "interrupted" | "stale"; revision: string
  sources: { kind: "message"; conversation_id: string; message_id: string; revision: string }[]
  binding: SummaryBinding; settingsRevision: string; requestedBy: string; membershipRevision: string
  participationEpoch: string; attempt: string; error: string | null; createdAt: string; updatedAt: string
}
export interface SummaryState { settings: SummarySettings; segments: Summary[]; hasMore: boolean }
export interface SummarySettingsInput extends VersionedCommand { enabled: boolean; binding: SummaryBinding | null }
export interface SummaryRequest { requestId: string; retry: { id: string; expectedRevision: string } | null }

export type ReferenceSource = { kind: "message"; conversation_id: string; message_id: string; revision: string } | { kind: "summary"; summary_id: string; revision: string }
export interface ReferenceSelection { conversationId: string; sources: ReferenceSource[] }
export interface ReferenceInput { actorId: string; selection: ReferenceSelection }
export interface ContextReference { target: { actor_id: string; conversation_id: string }; initiatedBy: string; attachedTo: string; selection: ReferenceSelection; stale: boolean }
export interface ReferencePreview { conversationId: string; title: string; messages: Message[]; summary: Summary | null }

export interface ConversationListState { readThrough: string; latestDelivery: string; unreadCount: number; markedUnread: boolean; pinned: boolean; revision: string }
export type ConversationListAction = { kind: "read"; through: string } | { kind: "unread" } | { kind: "pin"; pinned: boolean }
export interface SearchMessages { query: string; conversationId?: string | null; before?: string | null }
export interface MessageSearchPage { messages: Message[]; nextBefore: string | null }
export interface MessageContext { messages: Message[]; hasMore: boolean; hasNewer: boolean }
