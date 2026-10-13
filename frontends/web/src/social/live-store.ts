import { create } from "zustand"
import { socialClient, subscribe } from "./client"
import type { Change, LiveEvent, Memory, Message, RunDetails, Snapshot, SummaryState } from "./live-types"

export const LIVE_DRAFT_KEY = "llm-nest-social-live-drafts-v1"
interface Draft { reference?: import("./live-types").ReferenceInput | null; text: string; requestId: string | null; mentions?: string[]; replyTo?: string | null }
interface Generation { text: string; reasoning: string; sequence: number }
const emptyDraft = (): Draft => ({ text: "", requestId: null, mentions: [], replyTo: null })
function readDrafts(): Record<string, Draft> {
  try {
    const saved = JSON.parse(localStorage.getItem(LIVE_DRAFT_KEY) ?? "{}") as Record<string, Draft>
    return Object.fromEntries(Object.entries(saved).filter(([, draft]) => typeof draft?.text === "string"))
  } catch { return {} }
}

interface SocialState {
  historyReadThrough: Record<string, string>
  updateList(id: string, action: import("./live-types").ConversationListAction): Promise<void>
  summaries: Record<string, SummaryState | null>; summaryErrors: Record<string, string | null>; loadingSummaries: Record<string, boolean>
  loadSummaries(conversation: string, older?: boolean): Promise<void>
  memories: Record<string, Memory[]>; loadingMemories: Record<string, boolean>; memoryErrors: Record<string, string | null>
  loadMemories(conversation: string, actor: string): Promise<void>
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
  referenceDraft(id: string, reference: import("./live-types").ReferenceInput | null): void
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
const listWrites = new Map<string, Promise<void>>()
const memoryReads = new Map<string, Promise<void>>()
const summaryReads = new Map<string, Promise<void>>()

export const useLiveSocialStore = create<SocialState>((set, get) => ({
  historyReadThrough: {},
  async updateList(id, action) {
    const write = (listWrites.get(id) ?? Promise.resolve()).catch(() => {}).then(async () => {
      const conversation = get().snapshot!.conversations.find(c => c.id === id)!
      try {
        await socialClient.updateConversationList(id, { requestId: crypto.randomUUID(), expectedRevision: conversation.listState.revision, action })
      } finally { await get().refresh() }
    })
    listWrites.set(id, write)
    try { await write } finally { if (listWrites.get(id) === write) listWrites.delete(id) }
  },
  summaries: {}, summaryErrors: {}, loadingSummaries: {},
  async loadSummaries(id, older = false) {
    const read = (summaryReads.get(id) ?? Promise.resolve()).catch(() => {}).then(async () => {
      const generation = historyGeneration
      const previous = get().summaries[id]
      set({ summaries: { ...get().summaries, [id]: previous ?? null }, loadingSummaries: { ...get().loadingSummaries, [id]: true } })
      try {
        const page = await socialClient.summaries(id, older ? previous?.segments[previous.segments.length - 1]?.throughSequence : undefined)
        if (generation !== historyGeneration) return
        const value = older && previous ? { ...page, segments: [...previous.segments, ...page.segments] } : page
        set({ summaries: { ...get().summaries, [id]: value }, summaryErrors: { ...get().summaryErrors, [id]: null } })
      } catch (error) {
        if (generation === historyGeneration) set({ summaries: { ...get().summaries, [id]: null }, summaryErrors: { ...get().summaryErrors, [id]: String(error) } })
        throw error
      } finally { set({ loadingSummaries: { ...get().loadingSummaries, [id]: false } }) }
    })
    summaryReads.set(id, read)
    try { await read } finally { if (summaryReads.get(id) === read) summaryReads.delete(id) }
  },
  memories: {}, loadingMemories: {}, memoryErrors: {},
  async loadMemories(conversation, actor) {
    const key = `${conversation}/${actor}`
    // UI refreshes and subscription reads share ordering. A previous read reports
    // its own error to its caller; it must not prevent the next explicit retry.
    const previous = memoryReads.get(key) ?? Promise.resolve()
    const read = previous.catch(() => {}).then(async () => {
      const generation = historyGeneration
      set({ memories: { ...get().memories, [key]: get().memories[key] ?? [] }, loadingMemories: { ...get().loadingMemories, [key]: true } })
      try {
        const memories = await socialClient.memories(conversation, actor)
        if (generation === historyGeneration) set({ memories: { ...get().memories, [key]: memories }, memoryErrors: { ...get().memoryErrors, [key]: null } })
      } catch (error) {
        if (generation === historyGeneration) set({ memories: { ...get().memories, [key]: [] }, memoryErrors: { ...get().memoryErrors, [key]: String(error) } })
        throw error
      } finally { set({ loadingMemories: { ...get().loadingMemories, [key]: false } }) }
    })
    memoryReads.set(key, read)
    try { await read } finally { if (memoryReads.get(key) === read) memoryReads.delete(key) }
  },
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
      historyReadThrough: page.readThrough != null && BigInt(page.readThrough) >= BigInt(get().historyReadThrough[id] ?? "0") ? { ...get().historyReadThrough, [id]: page.readThrough } : get().historyReadThrough,
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
  referenceDraft(id, reference) {
    const drafts = { ...get().drafts, [id]: { ...(get().drafts[id] ?? emptyDraft()), reference, requestId: null } }
    set({ drafts })
    try { localStorage.setItem(LIVE_DRAFT_KEY, JSON.stringify(drafts)) }
    catch { set({ error: "引用保留在页面中，但浏览器未能保存草稿。" }) }
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
      const receipt = await socialClient.send(id, { requestId, text: draft.text, replyTo: draft.replyTo ?? null, mentions: draft.mentions ?? [], ...(draft.reference ? { reference: draft.reference.selection } : {}) })
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
  if (event.change.kind !== "conversation_list_changed" && event.change.conversationId && store.histories[event.change.conversationId]) {
    if (event.change.messageId) await store.loadMessage(event.change.messageId)
    const current = useLiveSocialStore.getState()
    const latest = current.snapshot?.conversations.find(c => c.id === event.change.conversationId)?.listState.latestDelivery
    if (!event.change.messageId || (latest && BigInt(latest) > BigInt(current.historyReadThrough[event.change.conversationId] ?? "0"))) await store.loadHistory(event.change.conversationId)
  }
  if (event.change.runId && !signal.aborted) await store.loadDetails(event.change.runId)
  if (event.change.kind === "memory_changed" && !signal.aborted) {
    const { conversationId, actorId } = event.change
    if (conversationId && actorId && `${conversationId}/${actorId}` in useLiveSocialStore.getState().memories) await store.loadMemories(conversationId, actorId)
  }
  if (event.change.kind === "summaries_changed" && !signal.aborted) {
    const id = event.change.conversationId
    if (id && id in useLiveSocialStore.getState().summaries) await store.loadSummaries(id)
  }
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
          const memoryScopes = Object.keys(store.memories)
          const summaryIds = Object.keys(store.summaries)
          historyGeneration++
          useLiveSocialStore.setState({ generations: {}, histories: {}, historyReadThrough: {}, hasMore: {}, details: {}, memories: {}, memoryErrors: {}, summaries: {}, summaryErrors: {} })
          await store.refresh()
          for (const id of ids) { if (attemptSignal.aborted) return; await store.loadHistory(id) }
          for (const id of summaryIds) { if (attemptSignal.aborted) return; await store.loadSummaries(id) }
          for (const key of memoryScopes) { if (attemptSignal.aborted) return; const [conversation, actor] = key.split("/"); await store.loadMemories(conversation, actor) }
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
