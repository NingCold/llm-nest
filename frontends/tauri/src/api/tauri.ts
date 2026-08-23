import { invoke } from "@tauri-apps/api/core"
import { listen } from "@tauri-apps/api/event"
import type {
  ChatApi,
  ChatParams,
  ChatEventHandler,
  GuiEvent as GuiEventRaw,
  AppInit as AppInitRaw,
  SessionSummary as SessionSummaryRaw,
  GuiConfig,
} from "./types"

export const tauriApi: ChatApi = {
  async init() {
    const raw = await invoke<AppInitRaw>("init_app")
    return raw
  },

  async chat(params: ChatParams, onEvent: ChatEventHandler) {
    const unlisten = await listen<GuiEventRaw>("chat-event", (event) => {
      const payload = event.payload as Record<string, unknown>
      const type = String(payload.type ?? "")
      const guiEvent: import("./types").GuiEvent = {
        type: type as import("./types").GuiEvent["type"],
        messageId: String(payload.messageId ?? ""),
        content: payload.content !== null && payload.content !== undefined ? String(payload.content) : undefined,
        error: payload.error !== null && payload.error !== undefined ? String(payload.error) : undefined,
      }
      onEvent(guiEvent)
      if (type === "finished" || type === "error" || type === "cancelled") {
        unlisten()
      }
    })
    await invoke("chat", {
      params: {
        sessionId: params.sessionId,
        input: params.input,
        model: params.model,
        temperature: params.temperature,
        maxTokens: params.maxTokens ?? null,
      },
    })
  },

  async cancelChat(sessionId: string) {
    await invoke("cancel_chat", { session_id: sessionId })
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
    await invoke("delete_session", { session_id: id })
  },

  async renameSession(id: string, title: string) {
    await invoke("rename_session", { session_id: id, title })
  },

  async setConfig(config: GuiConfig) {
    await invoke("set_config", { config })
  },
}