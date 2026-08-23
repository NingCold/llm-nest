import { create } from "zustand"

export interface Message {
  id: string
  role: "user" | "assistant"
  content: string
  status: "pending" | "streaming" | "done" | "error" | "cancelled"
  error?: string
}

interface ChatStore {
  messagesBySession: Record<string, Message[]>
  isStreaming: boolean

  getMessages: (sessionId: string) => Message[]
  addUserMessage: (sessionId: string, content: string) => string
  startAssistantMessage: (sessionId: string, messageId: string) => void
  appendDelta: (sessionId: string, messageId: string, content: string) => void
  finishMessage: (sessionId: string, messageId: string) => void
  failMessage: (sessionId: string, messageId: string, error: string) => void
  cancelMessage: (sessionId: string, messageId: string) => void
  setStreaming: (v: boolean) => void
  clearSession: (sessionId: string) => void
}

let msgCounter = 0
function genId(): string {
  msgCounter++
  return `msg-${Date.now()}-${msgCounter}`
}

export const useChatStore = create<ChatStore>((set, get) => ({
  messagesBySession: {},
  isStreaming: false,

  getMessages: (sessionId) => get().messagesBySession[sessionId] ?? [],

  addUserMessage: (sessionId, content) => {
    const id = genId()
    const msg: Message = { id, role: "user", content, status: "done" }
    set((state) => ({
      messagesBySession: {
        ...state.messagesBySession,
        [sessionId]: [...(state.messagesBySession[sessionId] ?? []), msg],
      },
    }))
    return id
  },

  startAssistantMessage: (sessionId, messageId) => {
    const msg: Message = { id: messageId, role: "assistant", content: "", status: "streaming" }
    set((state) => ({
      messagesBySession: {
        ...state.messagesBySession,
        [sessionId]: [...(state.messagesBySession[sessionId] ?? []), msg],
      },
    }))
  },

  appendDelta: (sessionId, messageId, content) => {
    set((state) => {
      const messages = state.messagesBySession[sessionId] ?? []
      return {
        messagesBySession: {
          ...state.messagesBySession,
          [sessionId]: messages.map((m) =>
            m.id === messageId ? { ...m, content: m.content + content } : m,
          ),
        },
      }
    })
  },

  finishMessage: (sessionId, messageId) => {
    set((state) => {
      const messages = state.messagesBySession[sessionId] ?? []
      return {
        messagesBySession: {
          ...state.messagesBySession,
          [sessionId]: messages.map((m) =>
            m.id === messageId ? { ...m, status: "done" } : m,
          ),
        },
        isStreaming: false,
      }
    })
  },

  failMessage: (sessionId, messageId, error) => {
    set((state) => {
      const messages = state.messagesBySession[sessionId] ?? []
      return {
        messagesBySession: {
          ...state.messagesBySession,
          [sessionId]: messages.map((m) =>
            m.id === messageId ? { ...m, status: "error", error } : m,
          ),
        },
        isStreaming: false,
      }
    })
  },

  cancelMessage: (sessionId, messageId) => {
    set((state) => {
      const messages = state.messagesBySession[sessionId] ?? []
      return {
        messagesBySession: {
          ...state.messagesBySession,
          [sessionId]: messages.map((m) =>
            m.id === messageId ? { ...m, status: "cancelled" } : m,
          ),
        },
        isStreaming: false,
      }
    })
  },

  setStreaming: (v) => set({ isStreaming: v }),

  clearSession: (sessionId) => {
    set((state) => {
      const { [sessionId]: _, ...rest } = state.messagesBySession
      return { messagesBySession: rest }
    })
  },
}))