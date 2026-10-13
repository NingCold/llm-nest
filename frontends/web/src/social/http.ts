import type { Change, LiveEvent } from "./live-types"

import { SocialApiError, type SocialClient } from "./transport"

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

export const socialClient: SocialClient = {
  updateConversationList: (id, input) => request(`/conversations/${id}/list-state`, "POST", input),
  messageContext: id => request(`/messages/${id}/context`),
  searchMessages: input => { const q = new URLSearchParams({ query: input.query }); if (input.conversationId) q.set("conversationId", input.conversationId); if (input.before) q.set("before", input.before); return request(`/messages/search?${q}`) },
  previewLegacy: () => request("/history-import"),
  importLegacy: preview => request("/history-import", "POST", preview),
  referencePreview: input => request("/references/preview", "POST", input),
  summaries: (id, before) => request(`/conversations/${id}/summaries${before ? `?before=${before}` : ""}`),
  configureSummaries: (id, input) => request(`/conversations/${id}/summary-settings`, "POST", input),
  requestSummary: (id, input) => request(`/conversations/${id}/summaries`, "POST", input),
  memories: (id, actor) => request(`/conversations/${id}/memories/${actor}`),
  createMemory: (id, actor, input) => request(`/conversations/${id}/memories/${actor}`, "POST", input),
  updateMemory: (id, input) => request(`/memories/${id}`, "PATCH", input),
  forgetMemory: (id, input) => request(`/memories/${id}/forget`, "POST", input),
  message: id => request(`/messages/${id}`),
  revise: (id, input) => request(`/messages/${id}/revise`, "POST", input),
  withdraw: (id, input) => request(`/messages/${id}/withdraw`, "POST", input),
  control: (id, input) => request(`/conversations/${id}/control`, "POST", input),
  pauseMember: (id, input) => request(`/conversations/${id}/members/participation`, "POST", input),
  updateParticipation: (id, input) => request(`/conversations/${id}/participation`, "POST", input),
  updateContactSettings: (id, input) => request(`/conversations/${id}/contact-settings`, "POST", input),
  createContactIntent: (id, input) => request(`/conversations/${id}/contact-intents`, "POST", input),
  cancelContactIntent: (id, intent, input) => request(`/conversations/${id}/contact-intents/${intent}/cancel`, "POST", input),
  createGroup: input => request("/groups", "POST", input),
  changeMembership: (id, input) => request(`/conversations/${id}/members`, "POST", input),
  plans: id => request(`/conversations/${id}/plans`),
  init: () => request("/init"),
  account: id => request(`/accounts/${id}`),
  updateAccount: (id, input) => request(`/accounts/${id}`, "PATCH", input),
  setContactBlock: (id, input) => request(`/contacts/${id}/block`, "POST", input),
  setContact: (id, input) => request(`/contacts/${id}`, "POST", input),
  managedAccounts: () => request("/ai-accounts"),
  managedAccount: id => request(`/ai-accounts/${id}`),
  createAi: input => request("/ai-accounts", "POST", input),
  updateAi: (id, input) => request(`/ai-accounts/${id}`, "PATCH", input),
  controlAccount: (id, input) => request(`/ai-accounts/${id}/control`, "POST", input),
  setAdministrator: (id, input) => request(`/ai-accounts/${id}/administrators`, "POST", input),
  accountView: id => request(`/ai-accounts/${id}/view`),
  accountViewMessages: (id, conversation, before) => request(`/ai-accounts/${id}/view/conversations/${conversation}/messages${before ? `?before=${before}` : ""}`),
  openDirect: (id, requestId) => request(`/accounts/${id}/direct`, "POST", { requestId }),
  messages: (id, before) => request(`/conversations/${id}/messages${before ? `?before=${before}` : ""}`),
  send: (id, input) => request(`/conversations/${id}/messages`, "POST", input),
  cancel: id => request(`/runs/${id}/cancel`, "POST"),
  runDetails: id => request(`/runs/${id}`),
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
