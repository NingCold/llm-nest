import type { DraftImage } from "./images"
import type { DraftFile } from "./files"
type DraftAttachment = DraftImage | DraftFile
import type { Draft, DraftRecord, PendingDraft } from "./drafts"

export const LEGACY_DRAFT_KEY = "llm-nest-social-live-drafts-v1"
export const DRAFT_DATABASE = "llm-nest-social-drafts"
interface StoredAttachment { id: string; name: string; mime: string; byteLength: number; blob: Blob }
type StoredDraft = Omit<Draft, "images" | "files"> & { images?: StoredAttachment[]; files?: StoredAttachment[] }
interface StoredRecord { owner: string; conversation: string; version: number; draft: StoredDraft; pending: StoredDraft | null }

function completed(transaction: IDBTransaction): Promise<void> {
  return new Promise((resolve, reject) => {
    transaction.oncomplete = () => resolve()
    transaction.onabort = () => reject(transaction.error ?? new Error("草稿事务未完成"))
    transaction.onerror = () => reject(transaction.error ?? new Error("草稿写入失败"))
  })
}

// One record contains the editable draft, its attachments and any unconfirmed send.
// Successful transactions are the only source of the UI's "saved" status.
export class DraftDatabase {
  private connection: Promise<IDBDatabase> | null = null
  private attachments = new WeakMap<DraftAttachment, StoredAttachment>()
  private versions = new Map<string, number>()
  constructor(private factory?: IDBFactory, private legacyStorage?: Storage) {}

  private open(): Promise<IDBDatabase> {
    if (this.connection) return this.connection
    this.connection = new Promise<IDBDatabase>((resolve, reject) => {
      const request = (this.factory ?? indexedDB).open(DRAFT_DATABASE, 1)
      let blocked = false
      request.onupgradeneeded = () => {
        const store = request.result.createObjectStore("drafts", { keyPath: ["owner", "conversation"] })
        store.createIndex("owner", "owner")
      }
      request.onerror = () => reject(request.error)
      request.onblocked = () => { blocked = true; reject(new Error("请关闭其他旧版页面后重试恢复草稿")) }
      request.onsuccess = () => {
        const db = request.result
        if (blocked) { db.close(); return }
        db.onversionchange = () => { db.close(); this.connection = null }
        resolve(db)
      }
    }).catch(error => { this.connection = null; throw error })
    return this.connection
  }

  private encode(draft: Draft): StoredDraft {
    const { images, files, ...fields } = draft
    const encode = (image: DraftAttachment): StoredAttachment => {
      let saved = this.attachments.get(image)
      if (!saved) {
        const bytes = Uint8Array.from(atob(image.data), c => c.charCodeAt(0))
        saved = { id: image.id, name: image.name, mime: image.mime, byteLength: image.byteLength, blob: new Blob([bytes], { type: image.mime }) }
        this.attachments.set(image, saved)
      }
      return saved
    }
    return { ...fields, images: images?.map(encode), files: files?.map(encode) }
  }

  private async decode(draft: StoredDraft): Promise<Draft> {
    if (typeof draft?.text !== "string") throw new Error("本地草稿格式无法读取，原数据已保留")
    const decode = async (saved: StoredAttachment): Promise<DraftAttachment> => {
      const bytes = new Uint8Array(await saved.blob.arrayBuffer())
      if (bytes.length !== saved.byteLength) throw new Error("草稿附件不完整，原数据已保留")
      let binary = ""
      for (let i = 0; i < bytes.length; i += 32768) binary += String.fromCharCode(...bytes.subarray(i, i + 32768))
      const image: DraftAttachment = { id: saved.id, name: saved.name, mime: saved.mime, byteLength: saved.byteLength, data: btoa(binary) }
      this.attachments.set(image, saved)
      return image
    }
    return { ...draft, images: await Promise.all((draft.images ?? []).map(decode)), files: await Promise.all((draft.files ?? []).map(decode)) }
  }

  async load(owner: string, conversations: string[]): Promise<Record<string, DraftRecord>> {
    const db = await this.open()
    const storage = this.legacyStorage ?? localStorage
    const legacyText = storage.getItem(LEGACY_DRAFT_KEY)
    const legacy: Record<string, Draft> = legacyText ? JSON.parse(legacyText) : {}
    const selected = new Set(conversations)
    const tx = db.transaction("drafts", "readwrite", { durability: "strict" })
    const done = completed(tx), store = tx.objectStore("drafts")
    const request = store.index("owner").getAll(owner)
    let records: StoredRecord[] = []
    request.onsuccess = () => {
      records = (request.result as StoredRecord[]).filter(record => selected.has(record.conversation))
      for (const id of conversations) {
        const old = legacy[id]
        if (!old || records.some(record => record.conversation === id)) continue
        if (typeof old.text !== "string" || old.images?.length || old.files?.length) { tx.abort(); return }
        const converted = this.encode(old)
        const record: StoredRecord = { owner, conversation: id, version: 1, draft: converted, pending: old.requestId ? converted : null }
        store.put(record); records.push(record)
      }
    }
    await done
    for (const id of conversations) this.versions.set(`${owner}/${id}`, records.find(r => r.conversation === id)?.version ?? 0)
    // Empty records remain as migration tombstones; cleared legacy drafts cannot reappear.
    for (const id of conversations) delete legacy[id]
    if (legacyText) {
      if (Object.keys(legacy).length) storage.setItem(LEGACY_DRAFT_KEY, JSON.stringify(legacy))
      else storage.removeItem(LEGACY_DRAFT_KEY)
    }
    return Object.fromEntries(await Promise.all(records.map(async record => {
      if (record.pending && typeof record.pending.requestId !== "string") throw new Error("发送草稿的重试编号无法读取，原数据已保留")
      return [record.conversation, { draft: await this.decode(record.draft), pending: record.pending ? await this.decode(record.pending) as PendingDraft : null }]
    })))
  }

  async save(owner: string, conversation: string, record: DraftRecord): Promise<void> {
    const db = await this.open()
    const key = `${owner}/${conversation}`
    const value: StoredRecord = { owner, conversation, version: 0, draft: this.encode(record.draft), pending: record.pending ? this.encode(record.pending) : null }
    const tx = db.transaction("drafts", "readwrite", { durability: "strict" })
    const done = completed(tx)
    const store = tx.objectStore("drafts"), read = store.get([owner, conversation])
    let conflict: Error | null = null
    read.onsuccess = () => {
      const previous = (read.result as StoredRecord | undefined)?.version ?? 0
      if (previous !== (this.versions.get(key) ?? 0)) {
        conflict = new Error("另一个页面已修改这份草稿。请先保留当前输入，再刷新查看，避免覆盖其他页面的内容。")
        tx.abort(); return
      }
      value.version = previous + 1
      store.put(value)
    }
    try { await done } catch (error) { throw conflict ?? error }
    this.versions.set(key, value.version)
  }
}

export const draftDatabase = new DraftDatabase()
