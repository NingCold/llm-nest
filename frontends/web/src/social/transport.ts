import type { Account, AccountView, AiInput, ContactEntry, ContactInput, ManagedAi, Change, ControlAction, Conversation, LiveEvent, Message, VersionedCommand, MessagePage, ParticipationPlan, RunDetails, SendInput, SendReceipt, Snapshot } from "./live-types"

export class SocialApiError extends Error { constructor(message: string, public code: string) { super(message) } }
export interface SocialClient {
  referencePreview(input: import("./live-types").ReferenceInput): Promise<import("./live-types").ReferencePreview>
  summaries(id: string, before?: string): Promise<import("./live-types").SummaryState>
  configureSummaries(id: string, input: import("./live-types").SummarySettingsInput): Promise<import("./live-types").SummarySettings>
  requestSummary(id: string, input: import("./live-types").SummaryRequest): Promise<import("./live-types").Summary | null>
  memories(id: string, actor: string): Promise<import("./live-types").Memory[]>
  createMemory(id: string, actor: string, input: import("./live-types").MemoryContent & { requestId: string }): Promise<import("./live-types").Memory>
  updateMemory(id: string, input: import("./live-types").MemoryContent & VersionedCommand): Promise<import("./live-types").Memory>
  forgetMemory(id: string, input: VersionedCommand): Promise<import("./live-types").Memory>
  message(id: string): Promise<Message>
  revise(id: string, input: VersionedCommand & { text: string }): Promise<Message>
  withdraw(id: string, input: VersionedCommand): Promise<Message>
  control(id: string, input: VersionedCommand & { action: ControlAction }): Promise<Conversation>
  pauseMember(id: string, input: VersionedCommand & { actorId: string; paused: boolean }): Promise<Conversation>
  createGroup(input: { requestId: string; title: string; memberIds: string[] }): Promise<string>
  changeMembership(id: string, input: VersionedCommand & { actorId: string; action: "add" | "remove" }): Promise<Conversation>
  plans(id: string): Promise<ParticipationPlan[]>
  init(): Promise<Snapshot>
  account(id: string): Promise<Account>
  updateAccount(id: string, input: VersionedCommand & { name: string; bio: string }): Promise<Account>
  setContact(id: string, input: ContactInput): Promise<ContactEntry>
  managedAccounts(): Promise<ManagedAi[]>
  managedAccount(id: string): Promise<ManagedAi>
  createAi(input: AiInput): Promise<ManagedAi>
  updateAi(id: string, input: AiInput & { expectedRevision: string }): Promise<ManagedAi>
  controlAccount(id: string, input: VersionedCommand & { action: "archive" | "restore" }): Promise<Account>
  setAdministrator(id: string, input: VersionedCommand & { accountId: string; granted: boolean }): Promise<ManagedAi>
  accountView(id: string): Promise<AccountView>
  accountViewMessages(id: string, conversation: string, before?: string): Promise<MessagePage>
  openDirect(id: string, requestId: string): Promise<string>
  messages(id: string, before?: string): Promise<MessagePage>
  send(id: string, input: SendInput): Promise<SendReceipt>
  cancel(id: string): Promise<void>
  runDetails(id: string): Promise<RunDetails | null>
}
export type Subscribe = (cursor: string, signal: AbortSignal, change: (event: Change) => Promise<void>, live: (event: LiveEvent) => void, resync: () => Promise<void>, ready: () => void) => Promise<void>
export type SubscriptionEvent = { kind: "change"; event: Change } | { kind: "generation"; event: LiveEvent } | { kind: "resync" }
