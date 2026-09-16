import { create } from "zustand"
import type { GuiAttachment, GuiTimings, GuiToolBlock, GuiUsage } from "@/api/types"

export type MessageStatus = "pending" | "streaming" | "done" | "error" | "cancelled"
export type Feedback = "up" | "down" | null

export interface Message {
  revision?: string
  id: string
  role: "user" | "assistant" | "tool"
  content: string
  reasoning?: string
  thinkingMs?: number
  status: MessageStatus
  error?: string
  feedback?: Feedback
  attachments?: GuiAttachment[]
  usage?: GuiUsage
  timings?: GuiTimings
  /** 工具调用（assistant）/ 工具结果（tool 角色消息） */
  tools?: GuiToolBlock[]
  /** tool 消息：执行中状态 */
  toolPending?: boolean
  createdAt: number
}

interface ChatStore {
  messagesBySession: Record<string, Message[]>
  isStreaming: boolean

  getMessages: (sessionId: string) => Message[]
  addUserMessage: (
    sessionId: string,
    content: string,
    attachments?: GuiAttachment[],
  ) => string
  startAssistantMessage: (sessionId: string, messageId: string) => void
  appendDelta: (sessionId: string, messageId: string, content: string) => void
  appendReasoningDelta: (
    sessionId: string,
    messageId: string,
    content: string,
  ) => void
  setThinkingTime: (sessionId: string, messageId: string, ms: number) => void
  setUsageTimings: (
    sessionId: string,
    messageId: string,
    usage?: GuiUsage,
    timings?: GuiTimings,
  ) => void
  finishMessage: (sessionId: string, messageId: string) => void
  failMessage: (sessionId: string, messageId: string, error: string) => void
  cancelMessage: (sessionId: string, messageId: string) => void
  setFeedback: (sessionId: string, messageId: string, fb: Feedback) => void
  /** 添加一条工具调用消息（流式收到 tool_call 时，pending 状态） */
  addToolCall: (
    sessionId: string,
    toolId: string,
    name: string,
    toolArguments: string,
  ) => void
  /** 更新工具结果为完成/错误（收到 tool_result 时） */
  updateToolResult: (
    sessionId: string,
    toolId: string,
    content: string,
    isError: boolean,
    durationMs?: number,
  ) => void
  /** 替换临时消息 ID；正常完成后以服务端持久化历史为准 */
  renameMessageId: (sessionId: string, oldId: string, newId: string) => void
  editUserMessage: (sessionId: string, messageId: string, content: string) => void
  truncateFrom: (sessionId: string, messageId: string) => void
  hydrateSession: (sessionId: string, messages: Message[]) => void
  setStreaming: (v: boolean) => void
  clearSession: (sessionId: string) => void
}

let msgCounter = 0
function genId(): string {
  msgCounter++
  return `msg-${Date.now()}-${msgCounter}`
}

function patch(
  state: ChatStore,
  sessionId: string,
  messageId: string,
  fn: (m: Message) => Message,
) {
  const messages = state.messagesBySession[sessionId] ?? []
  return {
    messagesBySession: {
      ...state.messagesBySession,
      [sessionId]: messages.map((m) => (m.id === messageId ? fn(m) : m)),
    },
  }
}

export const useChatStore = create<ChatStore>((set, get) => ({
  messagesBySession: {},
  isStreaming: false,

  getMessages: (sessionId) => get().messagesBySession[sessionId] ?? [],

  addUserMessage: (sessionId, content, attachments) => {
    const id = genId()
    const msg: Message = {
      id,
      role: "user",
      content,
      status: "done",
      ...(attachments && attachments.length > 0 ? { attachments } : {}),
      createdAt: Date.now(),
    }
    set((state) => ({
      messagesBySession: {
        ...state.messagesBySession,
        [sessionId]: [...(state.messagesBySession[sessionId] ?? []), msg],
      },
    }))
    return id
  },

  startAssistantMessage: (sessionId, messageId) => {
    const msg: Message = {
      id: messageId,
      role: "assistant",
      content: "",
      status: "streaming",
      createdAt: Date.now(),
    }
    set((state) => ({
      messagesBySession: {
        ...state.messagesBySession,
        [sessionId]: [...(state.messagesBySession[sessionId] ?? []), msg],
      },
    }))
  },

  appendDelta: (sessionId, messageId, content) =>
    set((state) =>
      patch(state, sessionId, messageId, (m) => ({
        ...m,
        content: m.content + content,
      })),
    ),

  appendReasoningDelta: (sessionId, messageId, content) =>
    set((state) =>
      patch(state, sessionId, messageId, (m) => ({
        ...m,
        reasoning: (m.reasoning ?? "") + content,
      })),
    ),

  setThinkingTime: (sessionId, messageId, ms) =>
    set((state) => patch(state, sessionId, messageId, (m) => ({ ...m, thinkingMs: ms }))),

  setUsageTimings: (sessionId, messageId, usage, timings) =>
    set((state) =>
      patch(state, sessionId, messageId, (m) => ({
        ...m,
        ...(usage ? { usage } : {}),
        ...(timings ? { timings } : {}),
      })),
    ),

  finishMessage: (sessionId, messageId) =>
    set((state) => ({
      ...patch(state, sessionId, messageId, (m) => ({ ...m, status: "done" })),
      isStreaming: false,
    })),

  failMessage: (sessionId, messageId, error) =>
    set((state) => ({
      ...patch(state, sessionId, messageId, (m) => ({ ...m, status: "error", error })),
      isStreaming: false,
    })),

  cancelMessage: (sessionId, messageId) =>
    set((state) => ({
      ...patch(state, sessionId, messageId, (m) => ({ ...m, status: "cancelled" })),
      isStreaming: false,
    })),

  setFeedback: (sessionId, messageId, fb) =>
    set((state) => patch(state, sessionId, messageId, (m) => ({ ...m, feedback: fb }))),

  addToolCall: (sessionId, toolId, name, toolArguments) =>
    set((state) => {
      const msg: Message = {
        id: `tool-${toolId}`,
        role: "tool",
        content: "",
        status: "pending",
        toolPending: true,
        tools: [{ kind: "call", id: toolId, name, arguments: toolArguments }],
        createdAt: Date.now(),
      }
      return {
        messagesBySession: {
          ...state.messagesBySession,
          [sessionId]: [...(state.messagesBySession[sessionId] ?? []), msg],
        },
      }
    }),

  updateToolResult: (sessionId, toolId, content, isError, durationMs) =>
    set((state) =>
      patch(state, sessionId, `tool-${toolId}`, (m) => ({
        ...m,
        status: "done",
        toolPending: false,
        content,
        tools: [
          {
            kind: "result",
            id: toolId,
            name: m.tools?.[0]?.name ?? "",
            content,
            isError,
            ...(durationMs != null ? { durationMs } : {}),
          },
        ],
      })),
    ),

  renameMessageId: (sessionId, oldId, newId) =>
    set((state) => patch(state, sessionId, oldId, (m) => ({ ...m, id: newId }))),

  editUserMessage: (sessionId, messageId, content) =>
    set((state) => {
      const messages = state.messagesBySession[sessionId] ?? []
      const idx = messages.findIndex((m) => m.id === messageId)
      if (idx === -1) return state
      const updated = messages
        .slice(0, idx + 1)
        .map((m) => (m.id === messageId ? { ...m, content } : m))
      return {
        messagesBySession: { ...state.messagesBySession, [sessionId]: updated },
      }
    }),

  truncateFrom: (sessionId, messageId) =>
    set((state) => {
      const messages = state.messagesBySession[sessionId] ?? []
      const idx = messages.findIndex((m) => m.id === messageId)
      if (idx === -1) return state
      return {
        messagesBySession: {
          ...state.messagesBySession,
          [sessionId]: messages.slice(0, idx),
        },
      }
    }),

  hydrateSession: (sessionId, messages) =>
    set((state) => ({
      messagesBySession: { ...state.messagesBySession, [sessionId]: messages },
    })),

  setStreaming: (v) => set({ isStreaming: v }),

  clearSession: (sessionId) =>
    set((state) => {
      const { [sessionId]: _, ...rest } = state.messagesBySession
      return { messagesBySession: rest }
    }),
}))
