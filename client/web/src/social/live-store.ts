import { create } from "zustand"
import { socialClient, subscribe } from "./client"
import type { Change, LiveEvent, Memory, Message, RunDetails, Snapshot, SummaryState } from "./live-types"
import { acknowledgeDraft, emptyDraft, sendInput, type Draft, type DraftRecord, type PendingDraft } from "./drafts"
import { draftDatabase } from "./draft-storage"
import { readFiles, validateFiles } from "./files"
import { ImageProcessingRequired, readImages } from "./images"
import { draftImageFile, imageSelectionIssue, inspectImages, releaseImages, type ImageSelection, type ImageSource } from "./image-processing"
interface Generation { text: string; reasoning: string; sequence: number }
interface DraftSaveState { status: "saving" | "saved" | "error"; error?: string }

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
  pendingSends: Record<string, PendingDraft | null>
  draftOwner: string | null; draftsReady: boolean; draftsClosing: boolean; draftLoadError: string | null; draftSaveStates: Record<string, DraftSaveState>
  loadDrafts(): Promise<void>
  saveDraft(id: string): Promise<void>
  discardPending(id: string): void
  initialize(): Promise<void>
  disconnect(): void
  refresh(): Promise<void>
  loadHistory(id: string, older?: boolean): Promise<void>
  loadDetails(id: string): Promise<void>
  loadMessage(id: string): Promise<void>
  applyMessage(message: Message): void
  updateDraft(id: string, text: string): void
  fileDraft(id: string, files: import("./files").DraftFile[]): void
  addDraftAttachments(id: string, files: File[]): Promise<void>
  addDraftFiles(id: string, files: File[]): Promise<void>
  imageDraft(id: string, images: import("./images").DraftImage[]): void
  addDraftImages(id: string, files: File[]): Promise<void>
  imageSelection: ImageSelection | null
  editDraftImage(id: string, imageId: string): Promise<void>
  cancelImageSelection(): void
  acceptImageSelection(selection: ImageSelection, sources: ImageSource[]): Promise<void>
  referenceDraft(id: string, reference: import("./live-types").ReferenceInput | null): void
  targetDraft(id: string, mentions: string[], replyTo: string | null): void
  send(id: string): Promise<void>
  action<T>(operation: () => Promise<T>, clearError?: boolean): Promise<T | undefined>
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
const draftWrites = new Map<string, Promise<void>>()
const draftReads = new Map<string, Promise<void>>()
let draftLoading: Promise<void> | null = null

function requireDrafts() {
  if (!useLiveSocialStore.getState().draftsReady) throw new Error("请先恢复本地草稿再编辑或发送")
  if (useLiveSocialStore.getState().draftsClosing) throw new Error("应用正在保存并关闭，请稍后编辑")
}

async function trackDraftRead(id: string, operation: () => Promise<void>) {
  requireDrafts()
  if (draftReads.size) throw new Error("请等待当前附件读取完成")
  const read = operation()
  draftReads.set(id, read)
  try { await read } finally { draftReads.delete(id) }
}

function selectionAllowed() {
  if (useLiveSocialStore.getState().imageSelection) throw new Error("请先确认或取消当前图片处理")
}

function persistDraft(id: string): Promise<void> {
  const store = useLiveSocialStore.getState(), owner = store.draftOwner!
  const record: DraftRecord = { draft: store.drafts[id] ?? emptyDraft(), pending: store.pendingSends[id] ?? null }
  const key = `${owner}/${id}`
  const current = () => {
    const state = useLiveSocialStore.getState()
    return state.draftOwner === owner && state.drafts[id] === record.draft && (state.pendingSends[id] ?? null) === record.pending
  }
  const status = (value: DraftSaveState) => {
    if (current()) useLiveSocialStore.setState(state => ({ draftSaveStates: { ...state.draftSaveStates, [id]: value } }))
  }
  status({ status: "saving" })
  // A failed write has its own visible error; later explicit edits can still be saved.
  const write = (draftWrites.get(key) ?? Promise.resolve()).catch(() => {}).then(async () => {
    try { await draftDatabase.save(owner, id, record); status({ status: "saved" }) }
    catch (error) { status({ status: "error", error: String(error) }); throw error }
  })
  draftWrites.set(key, write)
  void write.finally(() => { if (draftWrites.get(key) === write) draftWrites.delete(key) }).catch(() => {})
  return write
}

function persistEditedDraft(id: string) {
  // Errors are shown beside the affected draft, and the in-memory input stays editable.
  void persistDraft(id).catch(() => {})
}

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
  histories: {}, hasMore: {}, loadingHistory: {}, generations: {}, details: {}, drafts: {}, sending: {},
  pendingSends: {}, draftOwner: null, draftsReady: false, draftsClosing: false, draftLoadError: null, draftSaveStates: {},
  loadDrafts() {
    if (draftLoading) return draftLoading
    const snapshot = get().snapshot!
    set({ draftsReady: false, draftLoadError: null })
    draftLoading = (async () => {
      try {
        const records = await draftDatabase.load(snapshot.selfId, snapshot.conversations.map(c => c.id))
        set({ draftOwner: snapshot.selfId, drafts: Object.fromEntries(Object.entries(records).map(([id, r]) => [id, r.draft])),
          pendingSends: Object.fromEntries(Object.entries(records).map(([id, r]) => [id, r.pending])),
          draftsReady: true, draftSaveStates: {} })
      } catch (error) { set({ draftLoadError: `本地草稿未能恢复：${String(error)}。原数据已保留。` }) }
      finally { draftLoading = null }
    })()
    return draftLoading
  },
  async saveDraft(id) { requireDrafts(); await persistDraft(id) },
  discardPending(id) {
    requireDrafts()
    if (get().sending[id]) return
    set({ pendingSends: { ...get().pendingSends, [id]: null }, drafts: { ...get().drafts, [id]: { ...get().drafts[id], requestId: null } } })
    persistEditedDraft(id)
  },
  initialize() {
    active = true
    if (initialization) return initialization
    set({ loading: true, error: null })
    initialization = (async () => {
      try {
        const snapshot = await socialClient.init()
        if (!get().snapshot) cursor = snapshot.cursor
        set({ snapshot, loading: false })
        if (!get().draftsReady || get().draftOwner !== snapshot.selfId) await get().loadDrafts()
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
    requireDrafts()
    const drafts = { ...get().drafts, [id]: { ...get().drafts[id], text, requestId: null } }
    set({ drafts })
    persistEditedDraft(id)
  },
  fileDraft(id, files) {
    requireDrafts()
    set({ drafts: { ...get().drafts, [id]: { ...(get().drafts[id] ?? emptyDraft()), files, requestId: null } } })
    persistEditedDraft(id)
  },
  async addDraftAttachments(id, files) {
    selectionAllowed()
    await trackDraftRead(id, async () => {
      const attachments = files.filter(file => !file.type.startsWith("image/"))
      const images = files.filter(file => file.type.startsWith("image/"))
      const selectedFiles = await readFiles(attachments, get().drafts[id]?.files ?? [])
      let selectedImages: import("./images").DraftImage[] = []
      try { selectedImages = await readImages(images, get().drafts[id]?.images ?? []) }
      catch (error) {
        if (!(error instanceof ImageProcessingRequired)) throw error
        const sources = await inspectImages(images)
        set({ imageSelection: { id: crypto.randomUUID(), conversationId: id, sources, reason: error.message } })
      }
      const draft = get().drafts[id] ?? emptyDraft()
      const combined = [...(draft.files ?? []), ...selectedFiles]
      validateFiles(combined)
      set({ drafts: { ...get().drafts, [id]: { ...draft, files: combined, images: [...(draft.images ?? []), ...selectedImages], requestId: null } } })
      await persistDraft(id)
    })
  },
  async addDraftFiles(id, files) {
    selectionAllowed()
    await trackDraftRead(id, async () => {
      const selected = await readFiles(files, get().drafts[id]?.files ?? [])
      const draft = get().drafts[id] ?? emptyDraft()
      const combined = [...(draft.files ?? []), ...selected]
      validateFiles(combined)
      set({ drafts: { ...get().drafts, [id]: { ...draft, files: combined, requestId: null } } })
      await persistDraft(id)
    })
  },
  imageDraft(id, images) {
    requireDrafts()
    const drafts = { ...get().drafts, [id]: { ...(get().drafts[id] ?? emptyDraft()), images, requestId: null } }
    set({ drafts })
    persistEditedDraft(id)
  },
  imageSelection: null,
  async addDraftImages(id, files) {
    selectionAllowed()
    await trackDraftRead(id, async () => {
      let selected
      try { selected = await readImages(files, get().drafts[id]?.images ?? []) }
      catch (error) {
        if (!(error instanceof ImageProcessingRequired)) throw error
        const sources = await inspectImages(files)
        set({ imageSelection: { id: crypto.randomUUID(), conversationId: id, sources, reason: error.message } })
        return
      }
      const draft = get().drafts[id] ?? emptyDraft()
      // A selection started before close must finish and persist before the host exits.
      set({ drafts: { ...get().drafts, [id]: { ...draft, images: [...(draft.images ?? []), ...selected], requestId: null } } })
      await persistDraft(id)
    })
  },
  async editDraftImage(id, imageId) {
    selectionAllowed()
    await trackDraftRead(id, async () => {
      const image = get().drafts[id]!.images!.find(image => image.id === imageId)!
      const sources = await inspectImages([draftImageFile(image)])
      set({ imageSelection: { id: crypto.randomUUID(), conversationId: id, sources, reason: "调整这张草稿图片，确认后替换", replaceId: imageId } })
    })
  },
  cancelImageSelection() {
    if (get().imageSelection) releaseImages(get().imageSelection!.sources)
    set({ imageSelection: null })
  },
  async acceptImageSelection(selection, sources) {
    await trackDraftRead(selection.conversationId, async () => {
      if (get().imageSelection !== selection) throw new Error("这次图片选择已取消")
      const id = selection.conversationId
      const draft = get().drafts[id] ?? emptyDraft()
      const remaining = (draft.images ?? []).filter(image => image.id !== selection.replaceId)
      const issue = imageSelectionIssue(sources, remaining)
      if (issue) throw new Error(issue)
      const selected = await readImages(sources.map(source => source.file), remaining)
      if (get().imageSelection !== selection) throw new Error("这次图片选择已取消")
      const current = get().drafts[id] ?? emptyDraft()
      if (selection.replaceId && !current.images?.some(image => image.id === selection.replaceId)) throw new Error("原草稿图片已移除，请取消后重新选择")
      const images = selection.replaceId ? current.images!.map(image => image.id === selection.replaceId ? selected[0] : image) : [...(current.images ?? []), ...selected]
      set({ drafts: { ...get().drafts, [id]: { ...current, images, requestId: null } }, imageSelection: null })
      releaseImages(selection.sources)
      await persistDraft(id)
    })
  },
  referenceDraft(id, reference) {
    requireDrafts()
    const drafts = { ...get().drafts, [id]: { ...(get().drafts[id] ?? emptyDraft()), reference, requestId: null } }
    set({ drafts })
    persistEditedDraft(id)
  },
  targetDraft(id, mentions, replyTo) {
    requireDrafts()
    const drafts = { ...get().drafts, [id]: { ...(get().drafts[id] ?? emptyDraft()), mentions, replyTo, requestId: null } }
    set({ drafts })
    persistEditedDraft(id)
  },
  async send(id) {
    requireDrafts()
    if (get().sending[id] || draftReads.has(id) || get().imageSelection?.conversationId === id) return
    const pending = get().pendingSends[id]
    const draft = pending ?? get().drafts[id] ?? emptyDraft()
    if (!draft.text.trim() && !draft.images?.length && !draft.files?.length) return
    const generation = historyGeneration
    const requestId = draft.requestId ?? crypto.randomUUID()
    const sent: PendingDraft = { ...draft, requestId }
    set({ sending: { ...get().sending, [id]: true }, pendingSends: { ...get().pendingSends, [id]: sent },
      drafts: pending ? get().drafts : { ...get().drafts, [id]: sent }, error: null })
    try {
      // Persist the exact command before it can reach the backend. Reload never auto-sends it.
      await persistDraft(id)
      const receipt = await socialClient.send(id, sendInput(sent))
      set({ drafts: { ...get().drafts, [id]: acknowledgeDraft(get().drafts[id] ?? emptyDraft(), sent) }, pendingSends: { ...get().pendingSends, [id]: null } })
      try { await persistDraft(id) }
      catch { set({ error: "消息已保存，但本地草稿尚未清理，请重试保存草稿。" }) }
      // The acknowledgement is already committed; a subsequent refresh error must not undo it.
      const messages = get().histories[id] ?? []
      if (generation !== historyGeneration) await get().loadMessage(receipt.message.id)
      else if (!messages.some(m => m.id === receipt.message.id)) get().applyMessage(receipt.message)
      if (receipt.executionNotice) set({ error: receipt.executionNotice.message })
      await get().refresh()
    } catch (error) { set({ error: String(error) }) }
    finally { set({ sending: { ...get().sending, [id]: false } }) }
  },
  async action(operation, clearError = true) {
    try { if (clearError) set({ error: null }); return await operation() }
    catch (error) { set({ error: String(error) }); return undefined }
  },
  dismissError() { set({ error: null, connectionError: null }) },
}))

export function hasUnsavedDrafts(): boolean {
  return !!useLiveSocialStore.getState().imageSelection || draftReads.size > 0 || Object.values(useLiveSocialStore.getState().draftSaveStates).some(value => value.status !== "saved")
}

export async function flushDrafts(): Promise<void> {
  while (draftReads.size) await Promise.all(draftReads.values())
  if (useLiveSocialStore.getState().imageSelection) throw new Error("请先确认图片处理结果或取消选图，再关闭应用")
  while (draftWrites.size) await Promise.all(draftWrites.values())
  // Retry also covers drafts hidden by a later block/archive or a different page.
  const failed = Object.entries(useLiveSocialStore.getState().draftSaveStates).filter(([, value]) => value.status === "error")
  try { await Promise.all(failed.map(([id]) => persistDraft(id))) }
  catch (error) { throw new Error(`草稿尚未保存：${String(error)}`) }
  if (hasUnsavedDrafts()) throw new Error("草稿尚未保存，请在聊天中重试保存草稿后再关闭")
}

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
