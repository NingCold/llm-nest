import { create } from "zustand"
import { socialClient, subscribe } from "./client"
import type { Change, LiveEvent, Message, RunDetails, Snapshot } from "./live-types"

export const LIVE_DRAFT_KEY = "llm-nest-social-live-drafts-v1"
interface Draft { text: string; requestId: string | null; mentions?: string[]; replyTo?: string | null }
interface Generation { text: string; reasoning: string; sequence: number }
const emptyDraft = (): Draft => ({ text: "", requestId: null, mentions: [], replyTo: null })
function readDrafts(): Record<string, Draft> {
  try {
    const saved = JSON.parse(localStorage.getItem(LIVE_DRAFT_KEY) ?? "{}") as Record<string, Draft>
    return Object.fromEntries(Object.entries(saved).filter(([, draft]) => typeof draft?.text === "string"))
  } catch { return {} }
}

interface SocialState {
  snapshot: Snapshot | null; loading: boolean; error: string | null; connectionError: string | null; connected: boolean
  histories: Record<string, Message[]>; hasMore: Record<string, boolean>; loadingHistory: Record<string, boolean>
  generations: Record<string, Generation>; details: Record<string, RunDetails>; drafts: Record<string, Draft>; sending: Record<string, boolean>
  initialize(): Promise<void>
  disconnect(): void
  refresh(): Promise<void>
  loadHistory(id: string, older?: boolean): Promise<void>
  loadDetails(id: string): Promise<void>
  loadMessage(id: string): Promise<void>
  applyMessage(message: Message): void
  updateDraft(id: string, text: string): void
  targetDraft(id: string, mentions: string[], replyTo: string | null): void
  send(id: string): Promise<void>
  action<T>(operation: () => Promise<T>): Promise<T | undefined>
  dismissError(): void
}

let initialization: Promise<void> | null = null
let connection: AbortController | null = null
let cursor = "0"
let historyGeneration = 0
let active = false

export const useLiveSocialStore = create<SocialState>((set, get) => ({
  snapshot: null, loading: false, error: null, connectionError: null, connected: false,
  histories: {}, hasMore: {}, loadingHistory: {}, generations: {}, details: {}, drafts: readDrafts(), sending: {},
  initialize() {
    active = true
    if (initialization) return initialization
    set({ loading: true, error: null })
    initialization = (async () => {
      try {
        const snapshot = await socialClient.init()
        if (!get().snapshot) cursor = snapshot.cursor
        set({ snapshot, loading: false })
        if (active) startSubscription()
      } catch (error) { set({ loading: false, error: String(error) }) }
      finally { initialization = null }
    })()
    return initialization
  },
  disconnect() {
    active = false
    connection?.abort(); connection = null
    historyGeneration++
    set({ connected: false, generations: {} })
  },
  async refresh() {
    const snapshot = await socialClient.init()
    if (!get().snapshot || BigInt(snapshot.cursor) >= BigInt(get().snapshot!.cursor)) set({ snapshot })
  },
  async loadHistory(id, older = false) {
    const generation = historyGeneration
    const previous = get().histories[id] ?? []
    set({ loadingHistory: { ...get().loadingHistory, [id]: true } })
    try {
    const page = await socialClient.messages(id, older ? previous[0]?.sequence : undefined)
    if (generation !== historyGeneration) return
    const messages = new Map((get().histories[id] ?? []).map(m => [m.id, m]))
    for (const message of page.messages) {
      const current = messages.get(message.id)
      if (!current || BigInt(message.revision) > BigInt(current.revision)
        || (message.revision === current.revision && (message.status !== "generating" || message.text.length >= current.text.length))) messages.set(message.id, message)
    }
    const history = [...messages.values()].sort((a, b) => BigInt(a.sequence) < BigInt(b.sequence) ? -1 : 1)
    const generations = { ...get().generations }
    for (const message of history) if (message.status !== "generating") delete generations[message.id]
    set({ histories: { ...get().histories, [id]: history }, generations,
      hasMore: { ...get().hasMore, [id]: older || get().hasMore[id] === undefined ? page.hasMore : get().hasMore[id] } })
    } finally { set({ loadingHistory: { ...get().loadingHistory, [id]: false } }) }
  },
  async loadMessage(id) {
    const generation = historyGeneration
    const message = await socialClient.message(id)
    if (generation === historyGeneration) get().applyMessage(message)
  },
  applyMessage(message) {
    const previous = get().histories[message.conversationId] ?? []
    const current = previous.find(m => m.id === message.id)
    if (current && (BigInt(current.revision) > BigInt(message.revision) || (current.revision === message.revision && message.status === "generating" && current.text.length > message.text.length))) return
    const messages = previous.filter(m => m.id !== message.id).concat(message).sort((a, b) => BigInt(a.sequence) < BigInt(b.sequence) ? -1 : 1)
    const generations = { ...get().generations }
    if (message.status !== "generating") delete generations[message.id]
    set({ histories: { ...get().histories, [message.conversationId]: messages }, generations })
  },
  async loadDetails(id) {
    const details = await socialClient.runDetails(id)
    if (details) set({ details: { ...get().details, [id]: details } })
  },
  updateDraft(id, text) {
    const drafts = { ...get().drafts, [id]: { ...get().drafts[id], text, requestId: null } }
    set({ drafts })
    try { localStorage.setItem(LIVE_DRAFT_KEY, JSON.stringify(drafts)) }
    catch { set({ error: "输入保留在页面中，但浏览器未能保存草稿。" }) }
  },
  targetDraft(id, mentions, replyTo) {
    const drafts = { ...get().drafts, [id]: { ...(get().drafts[id] ?? emptyDraft()), mentions, replyTo, requestId: null } }
    set({ drafts })
    try { localStorage.setItem(LIVE_DRAFT_KEY, JSON.stringify(drafts)) }
    catch { set({ error: "接话选择保留在页面中，但浏览器未能保存草稿。" }) }
  },
  async send(id) {
    if (get().sending[id]) return
    const draft = get().drafts[id] ?? emptyDraft()
    if (!draft.text.trim()) return
    const generation = historyGeneration
    const requestId = draft.requestId ?? crypto.randomUUID()
    set({ sending: { ...get().sending, [id]: true }, drafts: { ...get().drafts, [id]: { ...draft, requestId } }, error: null })
    try {
      localStorage.setItem(LIVE_DRAFT_KEY, JSON.stringify(get().drafts))
      const receipt = await socialClient.send(id, { requestId, text: draft.text, replyTo: draft.replyTo ?? null, mentions: draft.mentions ?? [] })
      if (get().drafts[id]?.requestId === requestId) {
        const drafts = { ...get().drafts, [id]: emptyDraft() }
        set({ drafts })
        try { localStorage.setItem(LIVE_DRAFT_KEY, JSON.stringify(drafts)) }
        catch { set({ error: "消息已保存，但浏览器未能更新草稿缓存。" }) }
      }
      // The acknowledgement is already committed; a subsequent refresh error must not undo it.
      const messages = get().histories[id] ?? []
      if (generation !== historyGeneration) await get().loadMessage(receipt.message.id)
      else if (!messages.some(m => m.id === receipt.message.id)) get().applyMessage(receipt.message)
      if (receipt.executionNotice) set({ error: receipt.executionNotice.message })
      await get().refresh()
    } catch (error) { set({ error: String(error) }) }
    finally { set({ sending: { ...get().sending, [id]: false } }) }
  },
  async action(operation) {
    try { set({ error: null }); return await operation() }
    catch (error) { set({ error: String(error) }); return undefined }
  },
  dismissError() { set({ error: null, connectionError: null }) },
}))

async function applyChange(event: Change, signal: AbortSignal) {
  if (signal.aborted) return
  if (BigInt(event.cursor) <= BigInt(cursor)) return
  const store = useLiveSocialStore.getState()
  await store.refresh()
  if (signal.aborted) return
  if (event.change.conversationId && store.histories[event.change.conversationId]) {
    if (event.change.messageId) await store.loadMessage(event.change.messageId)
    else await store.loadHistory(event.change.conversationId)
  }
  if (event.change.runId && !signal.aborted) await store.loadDetails(event.change.runId)
  if (!signal.aborted) cursor = event.cursor
}

function applyLive(event: LiveEvent) {
  if (event.type === "execution.notice") { useLiveSocialStore.setState({ error: event.error.message }); return }
  const state = useLiveSocialStore.getState()
  const message = state.histories[event.conversationId]?.find(m => m.id === event.messageId)
  if (message && message.status !== "generating") return
  const current = state.generations[event.messageId]
  // Only a stream observed from its first fragment can be assembled safely.
  if (!current && event.deltaSeq !== 1) return
  if (current && event.deltaSeq !== current.sequence + 1) {
    const generations = { ...state.generations }; delete generations[event.messageId]
    useLiveSocialStore.setState({ generations }); return
  }
  const generation = current ?? { text: "", reasoning: "", sequence: 0 }
  useLiveSocialStore.setState({ generations: { ...state.generations, [event.messageId]: {
    text: generation.text + (event.type === "generation.delta" ? event.text : ""),
    reasoning: generation.reasoning + (event.type === "generation.reasoning_delta" ? event.text : ""), sequence: event.deltaSeq,
  } } })
}

function startSubscription() {
  connection?.abort()
  connection = new AbortController()
  const signal = connection.signal
  void (async () => {
    while (!signal.aborted) {
      const attempt = new AbortController()
      const abort = () => attempt.abort()
      signal.addEventListener("abort", abort, { once: true })
      const attemptSignal = attempt.signal
      try {
        await subscribe(cursor, attemptSignal, event => applyChange(event, attemptSignal), event => {
          if (!attemptSignal.aborted) applyLive(event)
        }, async () => {
          if (attemptSignal.aborted) return
          const store = useLiveSocialStore.getState()
          const ids = Object.keys(store.histories)
          historyGeneration++
          useLiveSocialStore.setState({ generations: {}, histories: {}, hasMore: {}, details: {} })
          await store.refresh()
          for (const id of ids) { if (attemptSignal.aborted) return; await store.loadHistory(id) }
        }, () => { if (!attemptSignal.aborted) useLiveSocialStore.setState({ connected: true, connectionError: null }) })
      } catch (error) {
        attempt.abort()
        if (signal.aborted) return
        historyGeneration++
        useLiveSocialStore.setState({ connected: false, generations: {}, connectionError: `${String(error)}，正在重新连接…` })
        await new Promise(resolve => setTimeout(resolve, 1000))
      } finally {
        attempt.abort()
        signal.removeEventListener("abort", abort)
      }
    }
  })()
}
