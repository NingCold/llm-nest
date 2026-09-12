import { invoke } from "@tauri-apps/api/core"
import { listen } from "@tauri-apps/api/event"
import { toGuiTimings, toGuiUsage } from "./normalize"
import type {
  ChatApi,
  ChatParams,
  ChatEventHandler,
  GuiEvent as GuiEventRaw,
  AppInit as AppInitRaw,
  SessionSummary as SessionSummaryRaw,
  GuiConfig,
  MessageFeedback,
  ProviderDraft,
  ProviderInfo,
  ProviderTemplate,
  StoredMessage,
} from "./types"

/** Rust `GuiMessage` 的 wire 形态（字段 camelCase） */
interface GuiMessageRaw {
  revision?: string
  id: string
  role: string
  content: string
  /** 持久化的思维链（随 assistant 消息保存） */
  reasoning?: string | null
  thinkingMs?: number | null
  status: StoredMessage["status"]
  error?: string
  createdAt?: number | null
  feedback?: MessageFeedback
  attachments?: import("./types").GuiAttachment[]
  usage?: import("./types").GuiUsage | null
  timings?: import("./types").GuiTimings | null
  tools?: import("./types").GuiToolBlock[]
}

export const tauriApi: ChatApi = {
  async init() {
    const raw = await invoke<AppInitRaw>("init_app")
    return raw
  },

  async chat(params: ChatParams, onEvent: ChatEventHandler) {
    let resolveDone!: () => void
    const done = new Promise<void>((resolve) => { resolveDone = resolve })
    const unlisten = await listen<GuiEventRaw>("chat-event", (event) => {
      const payload = event.payload as Record<string, unknown>
      if (String(payload.messageId ?? "") !== params.messageId) return
      const type = String(payload.type ?? "")
      const content =
        payload.content !== null && payload.content !== undefined
          ? String(payload.content)
          : undefined
      const error =
        payload.error !== null && payload.error !== undefined
          ? String(payload.error)
          : undefined
      const usage =
        payload.usage !== null && payload.usage !== undefined
          ? toGuiUsage(payload.usage)
          : undefined
      const timings =
        payload.timings !== null && payload.timings !== undefined
          ? toGuiTimings(payload.timings)
          : undefined
      const toolId =
        payload.toolId !== null && payload.toolId !== undefined
          ? String(payload.toolId)
          : undefined
      const toolName =
        payload.toolName !== null && payload.toolName !== undefined
          ? String(payload.toolName)
          : undefined
      const toolArguments =
        payload.toolArguments !== null && payload.toolArguments !== undefined
          ? String(payload.toolArguments)
          : undefined
      const toolContent =
        payload.toolContent !== null && payload.toolContent !== undefined
          ? String(payload.toolContent)
          : undefined
      const isError =
        payload.isError !== null && payload.isError !== undefined
          ? Boolean(payload.isError)
          : undefined
      const durationMs =
        payload.durationMs !== null && payload.durationMs !== undefined
          ? Number(payload.durationMs)
          : undefined
      const guiEvent = {
        type: type as import("./types").GuiEvent["type"],
        messageId: String(payload.messageId ?? ""),
        ...(content !== undefined ? { content } : {}),
        ...(error !== undefined ? { error } : {}),
        ...(usage !== undefined ? { usage } : {}),
        ...(timings !== undefined ? { timings } : {}),
        ...(toolId !== undefined ? { toolId } : {}),
        ...(toolName !== undefined ? { toolName } : {}),
        ...(toolArguments !== undefined ? { toolArguments } : {}),
        ...(toolContent !== undefined ? { toolContent } : {}),
        ...(isError !== undefined ? { isError } : {}),
        ...(durationMs !== undefined ? { durationMs } : {}),
      } as import("./types").GuiEvent
      onEvent(guiEvent)
      if (type === "finished" || type === "error" || type === "cancelled") {
        resolveDone()
      }
    })
    try {
      await invoke("chat", {
        params: {
          sessionId: params.sessionId,
          messageId: params.messageId ?? "",
          input: params.input,
          model: params.model,
          temperature: params.temperature,
          maxTokens: params.maxTokens ?? null,
          attachments: params.attachments ?? [],
          edit: params.edit ?? null,
        },
      })
      await done
    } finally { unlisten() }
  },

  async cancelChat(sessionId: string) {
    await invoke("cancel_chat", { sessionId: sessionId })
  },

  async getMessages(sessionId: string): Promise<StoredMessage[]> {
    const raw = await invoke<GuiMessageRaw[]>("get_messages", {
      sessionId: sessionId,
    })
    // 后端历史消息无真实时间戳（createdAt 为 null）时用加载时刻兜底
    const now = Date.now()
    return raw.map((m) => ({
      id: m.id,
      revision: m.revision,
      role: m.role === "assistant" ? "assistant" : m.role === "tool" ? "tool" : "user",
      content: m.content,
      ...(m.reasoning ? { reasoning: m.reasoning } : {}),
      ...(m.thinkingMs ? { thinkingMs: m.thinkingMs } : {}),
      ...(m.feedback ? { feedback: m.feedback } : {}),
      ...(m.attachments && m.attachments.length > 0
        ? { attachments: m.attachments }
        : {}),
      ...(m.usage ? { usage: toGuiUsage(m.usage) } : {}),
      ...(m.timings ? { timings: toGuiTimings(m.timings) } : {}),
      ...(m.tools && m.tools.length > 0 ? { tools: m.tools } : {}),
      status: m.status ?? "done",
      error: m.error,
      createdAt: m.createdAt ?? now,
    }))
  },

  async listSessions() {
    const raw = await invoke<SessionSummaryRaw[]>("list_sessions")
    return raw
  },

  async newSession() {
    const raw = await invoke<SessionSummaryRaw>("new_session")
    return raw
  },

  async deleteSession(id: string) {
    await invoke("delete_session", { sessionId: id })
  },

  async renameSession(id: string, title: string) {
    await invoke("rename_session", { sessionId: id, title })
  },

  async setConfig(config: GuiConfig) {
    await invoke("set_config", { config })
  },

  async setMessageFeedback(
    sessionId: string,
    messageId: string,
    feedback: MessageFeedback,
    revision: string,
  ) {
    // Persisted IDs plus the loaded revision prevent stale-page mutations.
    await invoke("set_message_feedback", {
      sessionId: sessionId,
      messageId,
      revision,
      feedback,
    })
  },

  async listProviderTemplates(): Promise<ProviderTemplate[]> {
    throw new Error("桌面模式暂不支持在线添加供应商")
  },

  async addProvider(_draft: ProviderDraft): Promise<ProviderInfo[]> {
    throw new Error("桌面模式暂不支持在线添加供应商")
  },

  async deleteProvider(_id: string): Promise<ProviderInfo[]> {
    throw new Error("桌面模式暂不支持在线添加供应商")
  },
}
