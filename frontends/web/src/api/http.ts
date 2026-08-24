import type {
  AppInit,
  ChatApi,
  ChatParams,
  ChatEventHandler,
  GuiConfig,
  GuiEvent,
  MessageFeedback,
  SessionSummary,
  StoredMessage,
} from "./types"
import { toGuiTimings, toGuiUsage } from "./normalize"

/**
 * HTTP adapter — talks to the real LLM Nest backend (crates/web-server) over
 * REST + SSE. Used by the pure-web frontend (frontends/web) so the browser
 * can drive the actual ChatFeature: streaming, thinking chain, multimodal.
 */

const BASE = "/api"

async function req<T>(path: string, init?: RequestInit): Promise<T> {
  const res = await fetch(`${BASE}${path}`, init)
  if (!res.ok) {
    let detail = res.statusText
    try {
      const body = (await res.json()) as { error?: string }
      if (body.error) detail = body.error
    } catch {
      /* keep statusText */
    }
    throw new Error(`${res.status} ${detail}`)
  }
  if (res.status === 204) return undefined as T
  return (await res.json()) as T
}

function jsonInit(method: string, body: unknown): RequestInit {
  return {
    method,
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(body),
  }
}

/** SSE 帧 → GuiEvent（后端每事件一帧 `data: {json}\n\n`） */
function parseFrame(frame: string): GuiEvent | null {
  const line = frame
    .split("\n")
    .find((l) => l.startsWith("data:"))
  if (!line) return null
  try {
    return JSON.parse(line.slice(5).trim()) as GuiEvent
  } catch {
    return null
  }
}

async function streamChat(
  path: string,
  params: Record<string, unknown>,
  onEvent: ChatEventHandler,
): Promise<void> {
  const res = await fetch(`${BASE}${path}`, jsonInit("POST", params))
  if (!res.ok || !res.body) {
    throw new Error(`chat failed: ${res.status} ${res.statusText}`)
  }
  const reader = res.body.getReader()
  const decoder = new TextDecoder()
  let buf = ""
  for (;;) {
    const { done, value } = await reader.read()
    if (done) break
    buf += decoder.decode(value, { stream: true })
    let idx: number
    while ((idx = buf.indexOf("\n\n")) !== -1) {
      const frame = buf.slice(0, idx)
      buf = buf.slice(idx + 2)
      const evt = parseFrame(frame)
      if (evt) {
        // wire 归一化：后端 usage/timings 是 snake_case，转 camelCase
        if (evt.type === "finished") {
          onEvent({
            ...evt,
            ...(toGuiUsage(evt.usage) ? { usage: toGuiUsage(evt.usage) } : {}),
            ...(toGuiTimings(evt.timings) ? { timings: toGuiTimings(evt.timings) } : {}),
          })
        } else {
          onEvent(evt)
        }
        if (evt.type === "finished" || evt.type === "error" || evt.type === "cancelled") {
          return
        }
      }
    }
  }
}

/** 探测后端是否可用（不可用时前端回退到演示适配器） */
export async function backendAvailable(): Promise<boolean> {
  try {
    const ctrl = new AbortController()
    const timer = setTimeout(() => ctrl.abort(), 1500)
    const res = await fetch(`${BASE}/init`, { signal: ctrl.signal })
    clearTimeout(timer)
    return res.ok
  } catch {
    return false
  }
}

export const httpApi: ChatApi = {
  async init(): Promise<AppInit> {
    return req<AppInit>("/init")
  },

  async chat(params: ChatParams, onEvent: ChatEventHandler): Promise<void> {
    await streamChat(
      `/sessions/${encodeURIComponent(params.sessionId)}/chat`,
      {
        sessionId: params.sessionId,
        messageId: params.messageId ?? "",
        input: params.input,
        model: params.model,
        temperature: params.temperature,
        maxTokens: params.maxTokens ?? null,
        attachments: params.attachments ?? [],
      },
      onEvent,
    )
  },

  async cancelChat(sessionId: string): Promise<void> {
    await req("/cancel", jsonInit("POST", { sessionId }))
  },

  async getMessages(sessionId: string): Promise<StoredMessage[]> {
    const raw = await req<StoredMessage[]>(
      `/sessions/${encodeURIComponent(sessionId)}/messages`,
    )
    const now = Date.now()
    return raw.map((m) => ({
      ...m,
      role: m.role === "assistant" ? "assistant" : m.role === "tool" ? "tool" : "user",
      status: "done",
      createdAt: m.createdAt ?? now,
      // wire 归一化：usage/timings 字段名 snake_case → camelCase
      ...(toGuiUsage(m.usage) ? { usage: toGuiUsage(m.usage) } : {}),
      ...(toGuiTimings(m.timings) ? { timings: toGuiTimings(m.timings) } : {}),
    }))
  },

  async listSessions(): Promise<SessionSummary[]> {
    return req<SessionSummary[]>("/sessions")
  },

  async newSession(): Promise<SessionSummary> {
    return req<SessionSummary>("/sessions", { method: "POST" })
  },

  async deleteSession(id: string): Promise<void> {
    await req(`/sessions/${encodeURIComponent(id)}`, { method: "DELETE" })
  },

  async renameSession(id: string, title: string): Promise<void> {
    await req(
      `/sessions/${encodeURIComponent(id)}`,
      jsonInit("PATCH", { title }),
    )
  },

  async setConfig(_config: GuiConfig): Promise<void> {
    // 后端无独立配置写回；模型/温度随每次 chat 请求携带
  },

  async setMessageFeedback(
    sessionId: string,
    messageId: string,
    feedback: MessageFeedback,
  ): Promise<void> {
    const idx = Number(messageId.replace(/^m-/, ""))
    await req(
      `/sessions/${encodeURIComponent(sessionId)}/messages/${Number.isFinite(idx) ? idx : -1}`,
      jsonInit("PATCH", { feedback }),
    )
  },
}
