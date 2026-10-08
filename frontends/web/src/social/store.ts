import { create } from "zustand"
import { createPreviewApi } from "./mock-api"
import type { ContextReference, SocialApi, SocialSnapshot } from "./types"

export interface ConversationDraft {
  text: string
  mentions: string[]
  replyTo: string | null
  contextReferences: ContextReference[]
  clientRequestId: string | null
}

const DRAFT_KEY = "llm-nest-social-preview-drafts-v1"
const emptyDraft = (): ConversationDraft => ({ text: "", mentions: [], replyTo: null, contextReferences: [], clientRequestId: null })
function initialDrafts(): Record<string, ConversationDraft> {
  try {
    const saved: unknown = JSON.parse(localStorage.getItem(DRAFT_KEY) ?? "{}")
    if (!saved || typeof saved !== "object" || Array.isArray(saved)) return {}
    return Object.fromEntries(Object.entries(saved).filter(([, value]) => value && typeof value.text === "string" && Array.isArray(value.mentions) && Array.isArray(value.contextReferences)))
  } catch { return {} }
}

interface SocialStore {
  api: SocialApi | null
  snapshot: SocialSnapshot | null
  error: string | null
  loading: boolean
  drafts: Record<string, ConversationDraft>
  sending: Record<string, boolean>
  initialize(): Promise<void>
  action<T>(operation: (api: SocialApi) => Promise<T>): Promise<T | undefined>
  dismissError(): void
  updateDraft(id: string, patch: Partial<ConversationDraft>): void
  send(id: string): Promise<boolean>
}

let initializePromise: Promise<void> | null = null
let unsubscribe: (() => void) | null = null
export const useSocialStore = create<SocialStore>((set, get) => ({
  api: null, snapshot: null, error: null, loading: false, drafts: initialDrafts(), sending: {},
  initialize() {
    if (get().snapshot) return Promise.resolve()
    if (initializePromise) return initializePromise
    set({ loading: true, error: null })
    initializePromise = (async () => {
      try {
        const api = get().api ?? createPreviewApi(localStorage)
        const snapshot = await api.init()
        unsubscribe?.()
        unsubscribe = api.subscribe((next, error) => set({ snapshot: next, ...(error ? { error } : {}) }))
        set({ api, snapshot, loading: false })
      } catch (error) { set({ error: String(error), loading: false }) }
      finally { initializePromise = null }
    })()
    return initializePromise
  },
  async action(operation) {
    const api = get().api
    if (!api) { set({ error: "预览尚未初始化。" }); return undefined }
    try { set({ error: null }); return await operation(api) }
    catch (error) { set({ error: String(error) }); return undefined }
  },
  dismissError() { set({ error: null }) },
  updateDraft(id, patch) {
    const draft = { ...(get().drafts[id] ?? emptyDraft()), ...patch, clientRequestId: null }
    const drafts = { ...get().drafts, [id]: draft }
    set({ drafts })
    try { localStorage.setItem(DRAFT_KEY, JSON.stringify(drafts)) } catch { set({ error: "输入仍保留在页面中，但浏览器未能保存草稿。" }) }
  },
  async send(id) {
    if (get().sending[id]) return false
    const draft = get().drafts[id] ?? emptyDraft()
    if (!draft.text.trim()) return false
    const api = get().api
    if (!api) return false
    const clientRequestId = draft.clientRequestId ?? crypto.randomUUID()
    set({ sending: { ...get().sending, [id]: true }, error: null, drafts: { ...get().drafts, [id]: { ...draft, clientRequestId } } })
    try {
      // Persist the retry ID before dispatch so a reload cannot duplicate a saved send.
      localStorage.setItem(DRAFT_KEY, JSON.stringify(get().drafts))
      await api.sendMessage({ ...draft, conversationId: id, clientRequestId })
      // Never erase text entered while a send was awaiting acknowledgement.
      if (get().drafts[id]?.clientRequestId === clientRequestId) {
        const drafts = { ...get().drafts, [id]: emptyDraft() }
        set({ drafts })
        try { localStorage.setItem(DRAFT_KEY, JSON.stringify(drafts)) } catch { set({ error: "消息已保存，但浏览器未能更新草稿缓存。" }) }
      }
      return true
    } catch (error) { set({ error: String(error) }); return false }
    finally { set({ sending: { ...get().sending, [id]: false } }) }
  },
}))
