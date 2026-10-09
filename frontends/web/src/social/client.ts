import type { Binding, Change, Contact, ControlAction, Conversation, LiveEvent, Message, VersionedCommand, MessagePage, ParticipationPlan, RunDetails, SendInput, SendReceipt, Snapshot } from "./live-types"

export class SocialApiError extends Error { constructor(message: string, public code: string) { super(message) } }

async function request<T>(path: string, method = "GET", body?: unknown): Promise<T> {
  const headers = new Headers({ "X-LLMN-Client": "1" })
  if (body !== undefined) headers.set("Content-Type", "application/json")
  const response = await fetch(`/api/social${path}`, { method, headers, ...(body !== undefined ? { body: JSON.stringify(body) } : {}) })
  if (!response.ok) {
    const error = response.headers.get("content-type")?.includes("application/json") ? await response.json() as { message: string; code?: string } : { code: "http_error", message: `请求失败（${response.status}）` }
    throw new SocialApiError(error.message, error.code ?? "http_error")
  }
  return response.status === 204 ? undefined as T : await response.json() as T
}

export const socialClient = {
  message: (id: string) => request<Message>(`/messages/${id}`),
  revise: (id: string, input: VersionedCommand & { text: string }) => request<Message>(`/messages/${id}/revise`, "POST", input),
  withdraw: (id: string, input: VersionedCommand) => request<Message>(`/messages/${id}/withdraw`, "POST", input),
  control: (id: string, input: VersionedCommand & { action: ControlAction }) => request<Conversation>(`/conversations/${id}/control`, "POST", input),
  pauseMember: (id: string, input: VersionedCommand & { actorId: string; paused: boolean }) => request<Conversation>(`/conversations/${id}/members/participation`, "POST", input),
  createGroup: (input: { requestId: string; title: string; memberIds: string[] }) => request<string>("/groups", "POST", input),
  changeMembership: (id: string, input: { requestId: string; actorId: string; expectedRevision: string; action: "add" | "remove" }) => request<Conversation>(`/conversations/${id}/members`, "POST", input),
  plans: (id: string) => request<ParticipationPlan[]>(`/conversations/${id}/plans`),
  init: () => request<Snapshot>("/init"),
  createContact: (input: { requestId: string; name: string; persona: string; binding: Binding | null }) => request<Contact>("/contacts", "POST", input),
  updateContact: (id: string, input: { requestId: string; name: string; persona: string; binding: Binding | null; expectedRevision: string }) => request<Contact>(`/contacts/${id}`, "PATCH", input),
  openDirect: (id: string, requestId: string) => request<string>(`/contacts/${id}/direct`, "POST", { requestId }),
  messages: (id: string, before?: string) => request<MessagePage>(`/conversations/${id}/messages${before ? `?before=${before}` : ""}`),
  send: (id: string, input: SendInput) => request<SendReceipt>(`/conversations/${id}/messages`, "POST", input),
  cancel: (id: string) => request<void>(`/runs/${id}/cancel`, "POST"),
  runDetails: (id: string) => request<RunDetails | null>(`/runs/${id}`),
}

// Fetch is used instead of EventSource so every request carries the local-client header.
export async function subscribe(cursor: string, signal: AbortSignal, change: (event: Change) => Promise<void>, live: (event: LiveEvent) => void, resync: () => Promise<void>, ready: () => void): Promise<void> {
  const headers = new Headers({ "X-LLMN-Client": "1" })
  const response = await fetch(`/api/social/subscribe?after=${cursor}`, { headers, signal })
  if (!response.ok || !response.body) throw new Error(`消息订阅失败（${response.status}）`)
  ready()
  const reader = response.body.getReader(), decoder = new TextDecoder()
  let buffer = ""
  try {
    for (;;) {
      const { value, done } = await reader.read()
      if (done) throw new Error("消息订阅已断开")
      buffer += decoder.decode(value, { stream: true })
      buffer = buffer.replace(/\r\n/g, "\n")
      let end: number
      while ((end = buffer.indexOf("\n\n")) !== -1) {
        const frame = buffer.slice(0, end); buffer = buffer.slice(end + 2)
        const lines = frame.split("\n")
        const kind = lines.find(line => line.startsWith("event:"))?.slice(6).trim()
        const data = lines.filter(line => line.startsWith("data:")).map(line => line.slice(5).trimStart()).join("\n")
        if (!data) continue
        if (kind === "change") await change(JSON.parse(data) as Change)
        else if (kind === "generation") live(JSON.parse(data) as LiveEvent)
        else if (kind === "resync") await resync()
        else if (kind === "error") throw new Error((JSON.parse(data) as FailureBody).message)
      }
    }
  } finally {
    await reader.cancel(); reader.releaseLock()
  }
}
interface FailureBody { message: string }
