// Business DTOs from features/social/src/wire.rs; counters stay decimal strings.
export interface Binding { providerId: string; modelId: string; reasoningEffort: string | null }
export interface Contact { id: string; name: string; persona: string; revision: string; binding: Binding | null }
export interface ModelOption { providerId: string; modelId: string; displayName: string; reasoningLevels: string[] }
export interface Message {
  id: string; conversationId: string; senderId: string; senderName: string
  sequence: string; text: string; status: "committed" | "generating" | "interrupted" | "withdrawn"
  revision: string; mentions: string[]; replyTo: string | null; runId: string | null; createdAt: string
}
export interface Run { id: string; conversationId: string; actorId: string; messageId: string; status: "running" | "succeeded" | "failed" | "cancelled" | "interrupted"; revision: string }
export interface Conversation {
  id: string; kind: "direct" | "group"; title: string; memberIds: string[]; status: "active" | "archived"
  paused: boolean; revision: string; lastSequence: string; legacy: boolean; latestMessage: Message | null; activeRun: Run | null
  members: Membership[]; pendingPlans: ParticipationPlan[]
}
export interface Membership { actorId: string; revision: string }
export interface ParticipationPlan { id: string; actorId: string; triggerId: string; triggerSequence: string; ordinal: number; status: "queued" | "running" | "succeeded" | "failed" | "cancelled" | "interrupted"; runId: string | null; failure: string | null; revision: string }
export interface Snapshot { selfId: string; selfName: string; contacts: Contact[]; conversations: Conversation[]; models: ModelOption[]; cursor: string }
export interface Failure { code: string; message: string }
export interface RunDetails {
  provider: string; model: string; wireModel: string; protocol: string; reasoning: string
  usage: { prompt_tokens: number; completion_tokens: number; total_tokens: number; cached_tokens?: number | null } | null
  timings: { ttft_ms: number | null; reasoning_ms: number | null; total_ms: number | null } | null; error: Failure | null
}
export interface SendInput { requestId: string; text: string; replyTo: string | null; mentions: string[] }
export interface SendReceipt { message: Message; executionNotice: Failure | null }
export interface MessagePage { messages: Message[]; hasMore: boolean }
export interface Change { cursor: string; change: { kind: string; conversationId?: string; messageId?: string; runId?: string; actorId?: string; revision: string } }
export type LiveEvent = { type: "execution.notice"; conversationId: string; error: Failure }
  | { type: "generation.delta" | "generation.reasoning_delta"; conversationId: string; runId: string; messageId: string; actorId: string; deltaSeq: number; text: string }
