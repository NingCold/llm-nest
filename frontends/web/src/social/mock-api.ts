import { createSeed } from "./seed"
import { memoryScope, sameScope } from "./types"
import type { AiContact, ContactInput, Conversation, MemoryScope, SendMessageInput, SocialApi, SocialMessage, SocialSnapshot } from "./types"

export const PREVIEW_STORAGE_KEY = "llm-nest-social-preview-v1"
type PreviewStorage = Pick<Storage, "getItem" | "setItem">
const uuid = () => crypto.randomUUID()
const now = () => new Date().toISOString()
const increment = (revision: string) => String(BigInt(revision) + 1n)
const clone = <T,>(value: T): T => structuredClone(value)
const uuidPattern = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i

function requiredText(text: string, max: number) {
  if (!text.trim() || text.length > max) throw new Error(`请输入内容，长度不超过 ${max} 字。`)
  return text.trim()
}

function validateContact(input: ContactInput) {
  requiredText(input.name, 40)
  requiredText(input.persona, 2000)
  if (input.tagline.length > 120 || input.interests.length > 8 || input.interests.some(v => v.length > 40)) throw new Error("介绍或兴趣标签过长。")
}

function validateSnapshot(value: unknown): asserts value is SocialSnapshot {
  if (!value || typeof value !== "object") throw new Error("预览数据格式不正确。")
  const data = value as SocialSnapshot
  if (data.schemaVersion !== 1 || !data.self || !uuidPattern.test(data.self.id)
    || !/^\d+$/.test(data.revision) || ![data.contacts, data.conversations, data.messages, data.memories, data.summaries, data.runs, data.commandReceipts].every(Array.isArray)) {
    throw new Error("预览数据版本或结构不正确，原数据已保留。")
  }
  for (const list of [data.contacts, data.conversations, data.messages, data.memories, data.summaries, data.runs]) {
    const ids = new Set<string>()
    for (const item of list) {
      if (!item || !uuidPattern.test(item.id) || ids.has(item.id)) throw new Error("预览数据有无效或重复 ID，原数据已保留。")
      ids.add(item.id)
    }
  }
  const contacts = new Set(data.contacts.map(c => c.id))
  const directs = new Set<string>()
  for (const conversation of data.conversations) {
    if (!Array.isArray(conversation.memberIds) || !conversation.memberIds.includes(data.self.id)
      || conversation.memberIds.some(id => id !== data.self.id && !contacts.has(id))) throw new Error("预览聊天成员数据不正确。")
    if (conversation.kind === "direct") {
      const actorId = conversation.memberIds.find(id => id !== data.self.id)
      if (!actorId || directs.has(actorId) || conversation.memberIds.length !== 2 || !conversation.relationshipId) throw new Error("预览私聊唯一性数据不正确。")
      directs.add(actorId)
    }
  }
}

/** Browser-only interactive prototype. No model, HTTP or Tauri calls. */
export function createPreviewApi(storage: PreviewStorage, options: { stepMs?: number } = {}): SocialApi {
  let db: SocialSnapshot | null = null
  const listeners = new Set<(snapshot: SocialSnapshot, error?: string) => void>()
  const controllers = new Map<string, { timer: ReturnType<typeof setTimeout>; conversationId: string }>()
  let queue: { conversationId: string; actorId: string; messageId: string }[] = []
  const stepMs = options.stepMs ?? 38
  function current() {
    if (!db) throw new Error("预览尚未初始化。")
    return db
  }
  function publish(error?: string) { for (const listener of listeners) listener(clone(current()), error) }
  function commit<T>(mutate: (next: SocialSnapshot) => T): T {
    const next = clone(current())
    const result = mutate(next)
    next.revision = increment(next.revision)
    try { storage.setItem(PREVIEW_STORAGE_KEY, JSON.stringify(next)) }
    catch { throw new Error("浏览器未能保存预览数据。请检查可用空间或浏览器存储权限后重试。") }
    db = next
    publish()
    return clone(result)
  }
  function conversation(data: SocialSnapshot, id: string) {
    const found = data.conversations.find(c => c.id === id)
    if (!found) throw new Error("聊天不存在，请从消息列表重新选择。")
    return found
  }
  function contact(data: SocialSnapshot, id: string) {
    const found = data.contacts.find(c => c.id === id)
    if (!found) throw new Error("联系人不存在。")
    return found
  }
  function touch(item: Conversation) { item.updatedAt = now(); item.revision = increment(item.revision) }
  function makeDirect(data: SocialSnapshot, actorId: string) {
    const existing = data.conversations.find(c => c.kind === "direct" && c.memberIds.includes(actorId))
    if (existing) return existing
    const actor = contact(data, actorId)
    const created: Conversation = { id: uuid(), kind: "direct", relationshipId: uuid(), title: actor.name, memberIds: [data.self.id, actorId], status: "active", paused: false, pinned: false, unread: 0, bout: "waiting", updatedAt: now(), revision: "1" }
    data.conversations.push(created)
    return created
  }
  function stopTimers(id: string) {
    queue = queue.filter(job => job.conversationId !== id)
    for (const [runId, controller] of controllers) if (controller.conversationId === id) {
      clearTimeout(controller.timer)
      controllers.delete(runId)
    }
  }
  function cancelIn(data: SocialSnapshot, id: string) {
    for (const run of data.runs) if (run.conversationId === id && run.status === "running") {
      run.status = "cancelled"
      const message = data.messages.find(m => m.id === run.messageId)!
      message.status = "cancelled"
      message.revision = increment(message.revision)
    }
  }
  function invalidate(data: SocialSnapshot, id: string) {
    for (const summary of data.summaries) if (summary.sourceIds.includes(id)) summary.status = "stale"
    for (const memory of data.memories) if (memory.sourceMessageId === id && memory.status !== "forgotten") {
      memory.status = "stale"; memory.revision = increment(memory.revision)
    }
    for (const run of data.runs) if (run.status === "running") {
      const output = data.messages.find(m => m.id === run.messageId)
      const trigger = output?.replyTo ? data.messages.find(m => m.id === output.replyTo) : undefined
      if (trigger?.id === id || trigger?.contextReferences.some(ref => ref.messageId === id)) {
        run.status = "cancelled"
        if (output) { output.status = "cancelled"; output.revision = increment(output.revision) }
      }
    }
  }
  function previewResponse(actor: AiContact, trigger: SocialMessage) {
    const quote = trigger.text.slice(0, 64)
    const reference = trigger.contextReferences[0]
    if (reference) {
      const source = conversation(current(), reference.conversationId)
      return `我们可以接着「${source.title}」里的这个话题聊。\n\n关于“${quote}”，你最想单独展开的是哪一个细节？这段后续讨论会留在我们的私聊里。`
    }
    if (actor.responseStyle === "concise") return `关于“${quote}”，可以先明确你最在意的一个问题，再把它拆成几个可讨论的小步骤。你想从哪里开始？`
    if (actor.responseStyle === "thoughtful") return `“${quote}”让我想到，或许可以先抓住一个具体的画面，再慢慢展开它。\n\n你愿意多说一点，是什么让你产生了这个想法吗？`
    return `我在听。关于“${quote}”，我想先了解你自己的感受。\n\n我们可以慢慢聊，你最想从哪一点说起？`
  }
  function pump(id: string) {
    const data = current()
    if (data.runs.some(run => run.conversationId === id && run.status === "running")) return
    const item = conversation(data, id)
    if (item.paused || item.status === "archived" || item.bout === "ended") return
    const index = queue.findIndex(job => job.conversationId === id)
    if (index < 0) return
    const job = queue.splice(index, 1)[0]
    const actor = contact(data, job.actorId)
    const trigger = data.messages.find(m => m.id === job.messageId)
    if (!trigger || trigger.status !== "complete" || actor.execution !== "preview") { pump(id); return }
    const runId = uuid(), messageId = uuid()
    let full: string
    try {
      for (const ref of trigger.contextReferences) {
        const source = data.messages.find(m => m.id === ref.messageId)
        if (!source || source.status !== "complete" || source.revision !== ref.messageRevision) throw new Error("引用来源已变化，请重新选择后接续。")
      }
      full = previewResponse(actor, trigger)
      commit(next => {
        next.runs.push({ id: runId, conversationId: id, actorId: actor.id, messageId, status: "running" })
        next.messages.push({ id: messageId, conversationId: id, senderId: actor.id, senderName: actor.name, text: "", mentions: [], replyTo: trigger.id, contextReferences: [], status: "generating", createdAt: now(), revision: "1" })
        conversation(next, id).bout = "active"
      })
    } catch (error) { publish(String(error)); return }
    let offset = 0
    function step() {
      if (!controllers.has(runId)) return
      const existing = current().runs.find(run => run.id === runId)
      if (!existing || existing.status !== "running") { controllers.delete(runId); pump(id); return }
      offset = Math.min(full.length, offset + 5)
      const finished = offset === full.length
      try {
        commit(next => {
          const message = next.messages.find(m => m.id === messageId)!
          message.text = full.slice(0, offset)
          message.revision = increment(message.revision)
          if (finished) {
            message.status = "complete"
            next.runs.find(run => run.id === runId)!.status = "succeeded"
            const space = conversation(next, id)
            space.bout = "waiting"; space.unread += 1; touch(space)
          }
        })
      } catch (error) { controllers.delete(runId); publish(String(error)); return }
      if (finished) { controllers.delete(runId); pump(id) }
      else controllers.set(runId, { timer: setTimeout(step, stepMs), conversationId: id })
    }
    controllers.set(runId, { timer: setTimeout(step, stepMs * 3), conversationId: id })
  }
  return {
    async init() {
      if (!db) {
        let parsed: SocialSnapshot
        const saved = storage.getItem(PREVIEW_STORAGE_KEY)
        try { parsed = saved ? JSON.parse(saved) : createSeed() }
        catch { throw new Error("无法读取浏览器中的预览数据，原数据已保留。") }
        validateSnapshot(parsed)
        for (const run of parsed.runs) if (run.status === "running") run.status = "interrupted"
        for (const message of parsed.messages) if (message.status === "generating") message.status = "interrupted"
        try { storage.setItem(PREVIEW_STORAGE_KEY, JSON.stringify(parsed)) }
        catch { throw new Error("无法保存预览数据，请允许当前网站使用浏览器存储后重试。") }
        db = parsed
      }
      return clone(current())
    },
    subscribe(listener) { listeners.add(listener); return () => { listeners.delete(listener) } },
    async createContact(input) {
      validateContact(input)
      return commit(data => {
        const actor: AiContact = { ...clone(input), name: input.name.trim(), id: uuid(), revision: "1" }
        data.contacts.push(actor); makeDirect(data, actor.id)
        return actor
      })
    },
    async updateContact(id, input, expectedRevision) {
      validateContact(input)
      commit(data => {
        const actor = contact(data, id)
        if (actor.revision !== expectedRevision) throw new Error("联系人资料已更新，请重新打开编辑。")
        Object.assign(actor, clone(input), { name: input.name.trim(), revision: increment(actor.revision) })
        for (const space of data.conversations) if (space.kind === "direct" && space.memberIds.includes(id)) space.title = actor.name
      })
    },
    async getDirectConversation(actorId) {
      const existing = current().conversations.find(c => c.kind === "direct" && c.memberIds.includes(actorId))
      return existing ? clone(existing) : commit(data => makeDirect(data, actorId))
    },
    async createGroup(title, memberIds) {
      const name = requiredText(title, 60)
      return commit(data => {
        const selected = [...new Set(memberIds)].filter(id => id !== data.self.id)
        if (!selected.length) throw new Error("请至少选择一个 AI 联系人。")
        selected.forEach(id => contact(data, id))
        const space: Conversation = { id: uuid(), kind: "group", relationshipId: null, title: name, memberIds: [data.self.id, ...selected], status: "active", paused: false, pinned: false, unread: 0, bout: "waiting", updatedAt: now(), revision: "1" }
        data.conversations.push(space)
        return space
      })
    },
    async updateConversation(id, changes) {
      commit(data => {
        const item = conversation(data, id)
        if (changes.title !== undefined) item.title = requiredText(changes.title, 60)
        if (changes.pinned !== undefined) item.pinned = changes.pinned
        if (changes.status !== undefined) {
          item.status = changes.status
          if (changes.status === "archived") cancelIn(data, id)
        }
        touch(item)
      })
      if (changes.status === "archived") stopTimers(id)
    },
    async markRead(id) {
      if (conversation(current(), id).unread === 0) return
      commit(data => { conversation(data, id).unread = 0 })
    },
    async sendMessage(input: SendMessageInput) {
      const text = requiredText(input.text, 20_000)
      if (!uuidPattern.test(input.clientRequestId)) throw new Error("发送请求 ID 不正确。")
      const digest = await crypto.subtle.digest("SHA-256", new TextEncoder().encode(JSON.stringify({ conversationId: input.conversationId, text, mentions: input.mentions, replyTo: input.replyTo, contextReferences: input.contextReferences })))
      const hash = Array.from(new Uint8Array(digest), byte => byte.toString(16).padStart(2, "0")).join("")
      const receipt = current().commandReceipts.find(r => r.clientRequestId === input.clientRequestId)
      if (receipt) {
        if (receipt.requestHash !== hash) throw new Error("发送请求已存在，请使用新的请求 ID。")
        return clone(current().messages.find(m => m.id === receipt.messageId)!)
      }
      const sent = commit(data => {
        const item = conversation(data, input.conversationId)
        if (item.status === "archived") throw new Error("请先恢复归档聊天。")
        for (const target of input.mentions) if (!item.memberIds.includes(target) || target === data.self.id) throw new Error("提及的 AI 不是当前聊天成员。")
        if (input.replyTo && !data.messages.some(m => m.id === input.replyTo && m.conversationId === item.id && m.status !== "redacted")) throw new Error("回复来源不可用。")
        for (const ref of input.contextReferences) {
          const source = conversation(data, ref.conversationId)
          const message = data.messages.find(m => m.id === ref.messageId && m.conversationId === source.id)
          if (item.kind !== "direct" || !item.memberIds.includes(ref.actorId) || source.kind !== "group" || !source.memberIds.includes(ref.actorId)
            || !message || message.status !== "complete" || message.revision !== ref.messageRevision) throw new Error("引用来源已变化或不在双方共同可见范围，请重新选择。")
        }
        const message: SocialMessage = { id: uuid(), conversationId: item.id, senderId: data.self.id, senderName: data.self.name, text, mentions: [...new Set(input.mentions)], replyTo: input.replyTo, contextReferences: clone(input.contextReferences), status: "complete", createdAt: now(), revision: "1" }
        data.messages.push(message)
        data.commandReceipts.push({ clientRequestId: input.clientRequestId, requestHash: hash, messageId: message.id })
        item.bout = "active"; touch(item)
        return message
      })
      const item = conversation(current(), sent.conversationId)
      const targets = item.kind === "direct" ? item.memberIds.filter(id => id !== current().self.id) : [...new Set(input.mentions)]
      if (!item.paused) {
        for (const actorId of targets) if (contact(current(), actorId).execution === "preview") queue.push({ conversationId: item.id, actorId, messageId: sent.id })
        pump(item.id)
      }
      return sent
    },
    async editMessage(id, text, expectedRevision) {
      const body = requiredText(text, 20_000)
      commit(data => {
        const message = data.messages.find(m => m.id === id)
        if (!message || message.senderId !== data.self.id || message.status !== "complete") throw new Error("只能编辑自己已发送的完整消息。")
        if (conversation(data, message.conversationId).status === "archived") throw new Error("请先恢复归档聊天。")
        if (message.revision !== expectedRevision) throw new Error("消息版本已变化，请重新编辑。")
        message.text = body; message.revision = increment(message.revision); invalidate(data, id)
      })
    },
    async redactMessage(id, expectedRevision) {
      commit(data => {
        const message = data.messages.find(m => m.id === id)
        if (!message || message.senderId !== data.self.id) throw new Error("只能撤回自己的消息。")
        if (conversation(data, message.conversationId).status === "archived") throw new Error("请先恢复归档聊天。")
        if (message.revision !== expectedRevision) throw new Error("消息版本已变化，请重新打开操作。")
        message.text = ""; message.status = "redacted"; message.revision = increment(message.revision); invalidate(data, id)
      })
    },
    async cancelRun(id) {
      const existing = current().runs.find(run => run.id === id)
      if (!existing || existing.status !== "running") return
      commit(data => {
        data.runs.find(run => run.id === id)!.status = "cancelled"
        const message = data.messages.find(m => m.id === existing.messageId)!
        message.status = "cancelled"; message.revision = increment(message.revision)
      })
      const timer = controllers.get(id)
      if (timer) clearTimeout(timer.timer)
      controllers.delete(id)
      pump(existing.conversationId)
    },
    async setPaused(id, paused) {
      commit(data => { const item = conversation(data, id); item.paused = paused; if (paused) cancelIn(data, id); touch(item) })
      if (paused) stopTimers(id)
    },
    async endBout(id) {
      commit(data => { const item = conversation(data, id); item.bout = "ended"; cancelIn(data, id); touch(item) })
      stopTimers(id)
    },
    async saveMemory(scope: MemoryScope, text, sourceMessageId, id, expectedRevision) {
      const body = requiredText(text, 1200)
      commit(data => {
        const item = data.conversations.find(c => sameScope(memoryScope(c), scope))
        if (!item) throw new Error("记忆范围不存在。")
        const source = sourceMessageId ? data.messages.find(m => m.id === sourceMessageId && m.conversationId === item.id && m.status === "complete") : null
        if (sourceMessageId && !source) throw new Error("记忆来源不可用。")
        if (id) {
          const record = data.memories.find(m => m.id === id && sameScope(m.scope, scope))
          if (!record || record.revision !== expectedRevision) throw new Error("记忆版本已变化，请重新编辑。")
          record.text = body; record.status = "confirmed"; record.updatedAt = now(); record.revision = increment(record.revision)
          // Editing is a new explicit user confirmation, independent of a stale source.
          record.sourceMessageId = source?.id ?? null; record.sourceRevision = source?.revision ?? null
        } else data.memories.push({ id: uuid(), scope: clone(scope), text: body, status: "confirmed", sourceMessageId: source?.id ?? null, sourceRevision: source?.revision ?? null, updatedAt: now(), revision: "1" })
      })
    },
    async forgetMemory(id, expectedRevision) {
      commit(data => {
        const record = data.memories.find(m => m.id === id)
        if (!record || record.revision !== expectedRevision) throw new Error("记忆版本已变化，请刷新后重试。")
        record.status = "forgotten"; record.text = ""; record.revision = increment(record.revision); record.updatedAt = now()
      })
    },
    async refreshSummary(id) {
      commit(data => {
        conversation(data, id)
        const messages = data.messages.filter(m => m.conversationId === id && m.status === "complete").slice(-8)
        if (!messages.length) throw new Error("还没有可整理的完整消息。")
        const text = messages.map(m => `${m.senderName}：${m.text.slice(0, 96)}`).join("\n\n")
        const existing = data.summaries.find(s => s.conversationId === id)
        const summary = { id: existing?.id ?? uuid(), conversationId: id, text, sourceIds: messages.map(m => m.id), status: "valid" as const, updatedAt: now() }
        if (existing) Object.assign(existing, summary)
        else data.summaries.push(summary)
      })
    },
  }
}
