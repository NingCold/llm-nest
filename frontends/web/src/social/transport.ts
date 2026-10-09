import type { Binding, Change, Contact, ControlAction, Conversation, LiveEvent, Message, VersionedCommand, MessagePage, ParticipationPlan, RunDetails, SendInput, SendReceipt, Snapshot } from "./live-types"

export class SocialApiError extends Error { constructor(message: string, public code: string) { super(message) } }
type ContactInput = { requestId: string; name: string; persona: string; binding: Binding | null }
export interface SocialClient {
  message(id: string): Promise<Message>
  revise(id: string, input: VersionedCommand & { text: string }): Promise<Message>
  withdraw(id: string, input: VersionedCommand): Promise<Message>
  control(id: string, input: VersionedCommand & { action: ControlAction }): Promise<Conversation>
  pauseMember(id: string, input: VersionedCommand & { actorId: string; paused: boolean }): Promise<Conversation>
  createGroup(input: { requestId: string; title: string; memberIds: string[] }): Promise<string>
  changeMembership(id: string, input: VersionedCommand & { actorId: string; action: "add" | "remove" }): Promise<Conversation>
  plans(id: string): Promise<ParticipationPlan[]>
  init(): Promise<Snapshot>
  createContact(input: ContactInput): Promise<Contact>
  updateContact(id: string, input: ContactInput & { expectedRevision: string }): Promise<Contact>
  openDirect(id: string, requestId: string): Promise<string>
  messages(id: string, before?: string): Promise<MessagePage>
  send(id: string, input: SendInput): Promise<SendReceipt>
  cancel(id: string): Promise<void>
  runDetails(id: string): Promise<RunDetails | null>
}
export type Subscribe = (cursor: string, signal: AbortSignal, change: (event: Change) => Promise<void>, live: (event: LiveEvent) => void, resync: () => Promise<void>, ready: () => void) => Promise<void>
export type SubscriptionEvent = { kind: "change"; event: Change } | { kind: "generation"; event: LiveEvent } | { kind: "resync" }
