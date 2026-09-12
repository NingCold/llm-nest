import { create } from "zustand"
import { useConfigStore } from "./config"
import { useUiStore } from "./ui"
import type { SessionSummary } from "@/api/types"
async function getApi() {
  const mod = await import("@/api")
  return mod.getApi()
}

interface SessionStore {
  sessions: SessionSummary[]
  currentSessionId: string | null
  loading: boolean
  setSessions: (sessions: SessionSummary[]) => void
  setCurrentSession: (id: string) => void
  refreshSessions: () => Promise<void>
  createSession: () => Promise<SessionSummary>
  deleteSession: (id: string) => Promise<void>
  renameSession: (id: string, title: string) => Promise<void>
}

export const useSessionStore = create<SessionStore>((set, get) => ({
  sessions: [],
  currentSessionId: null,
  loading: false,

  setSessions: (sessions) => set({ sessions }),

  setCurrentSession: (id) => {
    set({ currentSessionId: id })
    const model = get().sessions.find((s) => s.id === id)?.model
    if (model) {
      const { reasoning_effort, ...selection } = model as typeof model & { reasoning_effort?: string }
      useConfigStore.getState().setModel(selection)
      useUiStore.getState().setReasoningEffort(model.reasoningEffort ?? reasoning_effort ?? "off")
    }
  },

  refreshSessions: async () => {
    set({ loading: true })
    try {
      const api = await getApi()
      const sessions = await api.listSessions()
      set({ sessions, loading: false })
    } catch {
      set({ loading: false })
    }
  },

  createSession: async () => {
    const api = await getApi()
    const session = await api.newSession()
    set((state) => ({
      sessions: [session, ...state.sessions],
      currentSessionId: session.id,
    }))
    return session
  },

  deleteSession: async (id) => {
    const api = await getApi()
    await api.deleteSession(id)
    set((state) => {
      const sessions = state.sessions.filter((s) => s.id !== id)
      const currentSessionId =
        state.currentSessionId === id
          ? sessions[0]?.id ?? null
          : state.currentSessionId
      return { sessions, currentSessionId }
    })
  },

  renameSession: async (id, title) => {
    const api = await getApi()
    await api.renameSession(id, title)
    set((state) => ({
      sessions: state.sessions.map((s) =>
        s.id === id ? { ...s, title } : s,
      ),
    }))
  },
}))