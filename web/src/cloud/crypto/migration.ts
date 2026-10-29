import { request, downloadBytes, server, HttpError } from "../api";
import { cryptoDevice, recoveryConfirmed, recoveryReader, matrixRoom, matrixUser, seal, signEnvelope, encryptAttachment, type RecoveryFile } from "./engine";
import { projectMessage, projectIntent, type Content } from "./content";
import { decodeAttachment } from "./attachments";
import { canonicalJson } from "./canonical";
import type { Conversation, Message, ContactIntent, Attachment, HistoryMigration } from "../types";

export interface MigrationPreview { conversationId: string; conversationRevision: string; sourceDigest: string; messageCount: number; contactIntentCount: number; attachmentCount: number; backupRequired: boolean; blockers?: { nodeTasks: number; summaryCalls: number; generatingMessages: number; contactIntents: number }; canBegin?: boolean; activeMigrationId?: string | null }
export interface MigrationStatus { id: string; conversationId: string; conversationRevision: string; sourceDigest: string; state: "active" | "complete" | "conflicted"; currentConversationRevision?: string; remaining: number; backupDigest: string; recoveryDigest: string; verificationDigest: string | null }
export interface MigrationItem { kind: "message" | "contactIntent"; id: string; revision: string; sourceDigest: string; converted: boolean; originalAudienceRevision?: string | null; record: Message | ContactIntent }
interface Command { path: string; method: string; body: Record<string, unknown> }
export interface MigrationSession { version?: number; status?: MigrationStatus; pending?: Command; rejected?: { command: Command; error: string }[] }
let database: Promise<IDBDatabase> | undefined;
const key = (account: string, conversation: string) => JSON.stringify([server, account, conversation]);
function db() { return database ??= new Promise((resolve, reject) => { const r = indexedDB.open("llmn-history-migrations", 1); r.onupgradeneeded = () => r.result.createObjectStore("sessions"); r.onsuccess = () => resolve(r.result); r.onerror = () => reject(r.error); }); }
async function storage<T>(mode: IDBTransactionMode, action: (s: IDBObjectStore) => IDBRequest<T>) { const tx = (await db()).transaction("sessions", mode, { durability: "strict" }), result = action(tx.objectStore("sessions")); return new Promise<T>((resolve, reject) => { tx.oncomplete = () => resolve(result.result); tx.onerror = tx.onabort = () => reject(tx.error); }); }
export async function migrationSession(account: string, conversation: string): Promise<MigrationSession> { return await storage<MigrationSession | undefined>("readonly", s => s.get(key(account, conversation))) ?? {}; }
async function remember(account: string, conversation: string, value: MigrationSession) {
  const tx = (await db()).transaction("sessions", "readwrite", { durability: "strict" }), table = tx.objectStore("sessions"), id = key(account, conversation), read = table.get(id);
  let conflict: Error | undefined;
  read.onsuccess = () => {
    if ((read.result?.version ?? 0) !== (value.version ?? 0)) { conflict = new Error("另一个页面已更新转换请求，请重新读取状态，不能覆盖原意图"); tx.abort(); return; }
    table.put({ ...value, version: (value.version ?? 0) + 1 }, id);
  };
  await new Promise<void>((resolve, reject) => { tx.oncomplete = () => resolve(); tx.onabort = tx.onerror = () => reject(conflict ?? tx.error); });
}
export async function digest(bytes: Uint8Array) { return Array.from(new Uint8Array(await crypto.subtle.digest("SHA-256", new Uint8Array(bytes))), b => b.toString(16).padStart(2, "0")).join(""); }
const digestJson = (value: unknown) => digest(new TextEncoder().encode(canonicalJson(value)));

function device(account: string) { const id = cryptoDevice(account); if (!id || !recoveryConfirmed(account)) throw new Error("请先解锁可信设备并确认恢复材料"); return id; }
export const previewMigration = (conversation: string) => request<MigrationPreview>("/crypto/history-migrations/preview", "POST", { conversationId: conversation });
export async function refreshMigration(account: string, conversation: string) {
  const saved = await migrationSession(account, conversation);
  const result = await request<{ migration: MigrationStatus | null }>(`/crypto/history-migrations?conversationId=${encodeURIComponent(conversation)}`);
  const next = { ...saved, status: result.migration ?? undefined }; await remember(account, conversation, next); return next;
}
// Both initial dispatch and a later retry read the same persisted command. No retry advances Megolm.
export const retryMigration = (account: string, conversation: string) => executePending(account, conversation, false);
export async function confirmMigrationResult(account: string, conversation: string): Promise<MigrationSession> {
  const saved = await migrationSession(account, conversation);
  if (!saved.pending) return saved;
  const command = saved.pending, parts = command.path.split("/");
  const operation = parts.length === 3 ? "begin" : parts[4] === "finish" ? `finish:${parts[3]}` : `${parts[3]}:${parts[5]}:${parts[6]}`;
  const result = await request<{ receipt: { fingerprint: string; migration: MigrationStatus } | null }>(`/crypto/history-migration-receipts/${encodeURIComponent(String(command.body.requestId))}`);
  if (!result.receipt) throw new Error("尚未找到原请求回执，不能判断是否已提交；原请求仍保留");
  if (result.receipt.fingerprint !== await digestJson([operation, command.body])) throw new Error("回执与已保存的原请求不一致，未解除待确认状态");
  const next = { ...saved, status: result.receipt.migration, pending: undefined };
  await remember(account, conversation, next); return next;
}
async function executePending(account: string, conversation: string, first: boolean): Promise<MigrationSession> {
  const saved = await migrationSession(account, conversation);
  if (!saved.pending) return refreshMigration(account, conversation);
  device(account);
  let result: any;
  try { result = await request<any>(saved.pending.path, saved.pending.method, saved.pending.body); }
  catch (error) {
    if (error instanceof HttpError && [400, 403, 404, 409, 422].includes(error.status)) {
      let status = saved.status;
      if (!first && status) status = await request<MigrationStatus>(`/crypto/history-migrations/${status.id}`);
      if (first || status?.state === "conflicted") await remember(account, conversation, { version: saved.version, status, rejected: [...(saved.rejected ?? []), { command: saved.pending, error: error.message }] });
    }
    throw error;
  }
  const status = result.state ? result as MigrationStatus : await request<MigrationStatus>(`/crypto/history-migrations/${saved.status!.id}`);
  const next = { ...saved, status, pending: undefined }; await remember(account, conversation, next); return next;
}
async function dispatch(account: string, conversation: string, command: Command) {
  if (command.body.deviceId !== device(account)) throw new Error("加密设备已变化，未保存转换请求");
  const saved = await migrationSession(account, conversation);
  if (saved.pending) throw new Error("已有转换请求尚未确认，请先重试原请求或核对状态");
  await remember(account, conversation, { ...saved, pending: command });
  return executePending(account, conversation, true);
}
export async function beginMigration(account: string, conversation: Conversation, preview: MigrationPreview, backupDigest: string, recoveryDigest: string, file: RecoveryFile) {
  if (preview.canBegin === false) throw new Error("请先处理预览中的未完成事项，或恢复当前转换");
  if (!/^[a-f0-9]{64}$/.test(backupDigest)) throw new Error("请输入一致备份脚本提供的 SHA-256 摘要");
  const reader = await recoveryReader(file); reader.close();
  return dispatch(account, conversation.id, { path: "/crypto/history-migrations", method: "POST", body: { requestId: crypto.randomUUID(), deviceId: device(account), conversationId: conversation.id, expectedConversationRevision: preview.conversationRevision, sourceDigest: preview.sourceDigest, backupDigest, recoveryDigest, localPendingResolved: true } });
}
export async function migrationItems(id: string, after = "0", includeConverted = false) { return request<{ items: MigrationItem[]; next: string; hasMore: boolean }>(`/crypto/history-migrations/${id}/items?after=${encodeURIComponent(after)}&limit=32${includeConverted ? "&includeConverted=true" : ""}`); }
async function encryptItem(account: string, conversation: Conversation, status: MigrationStatus, item: MigrationItem): Promise<Command> {
  const migration: HistoryMigration = { id: status.id, actorId: account, sourceDigest: item.sourceDigest };
  const attachments: Attachment[] = [], encryptedAttachments: { index: number; data: string }[] = [];
  let inner: Content;
  const base = { version: 1 as const, server, conversationId: conversation.id, conversationRevision: status.conversationRevision, messageId: item.id, historyMigration: migration, runId: null, sequence: null };
  if (item.kind === "message") {
    const record = item.record as Message;
    for (const a of record.attachments) {
      const bytes = await downloadBytes(`/chat/messages/${record.id}/attachments/${a.index}?revision=${record.revision}`);
      if (bytes.length !== a.byteLength) throw new Error("旧附件读取不完整，未提交转换");
      const encrypted = encryptAttachment(bytes);
      attachments.push({ ...a, mediaEncryptionInfo: encrypted.mediaEncryptionInfo }); encryptedAttachments.push({ index: a.index, data: encrypted.data });
    }
    inner = { ...base, kind: "historyMessage", senderId: record.senderId, revision: record.revision, text: record.text, replyTo: record.replyTo, mentions: record.mentions, attachments, reference: record.contextReference ? { conversationId: record.contextReference.conversationId, sources: record.contextReference.sources } : null };
  } else {
    const record = item.record as ContactIntent;
    inner = { ...base, kind: "historyContactIntent", senderId: record.creatorId, revision: "1", text: record.content, aiId: record.aiId, dueAt: record.dueAt, expiresAt: record.expiresAt, utcOffsetMinutes: record.utcOffsetMinutes, replyTo: null, mentions: [] };
  }
  const revision = inner.revision, roomId = matrixRoom(conversation.id), deviceId = device(account);
  const encrypted = await seal(roomId, conversation.members.map(m => m.account.id), "com.llmn.content.v1", inner, false, [account], true);
  const sealed = await signEnvelope({ version: 1, roomId, sender: matrixUser(account), deviceId, eventId: `$${item.id}.${revision}.migration.${status.id}`, content: encrypted });
  return { path: `/crypto/history-migrations/${status.id}/items/${item.kind}/${item.id}`, method: "PUT", body: { requestId: crypto.randomUUID(), deviceId, expectedRevision: item.revision, sourceDigest: item.sourceDigest, sealed, ...(item.kind === "message" ? { encryptedAttachments } : {}) } };
}
export async function convertMigrationPage(account: string, conversation: Conversation) {
  const saved = await refreshMigration(account, conversation.id);
  if (saved.pending) throw new Error("请先手动重试原转换请求");
  if (!saved.status || saved.status.state !== "active") throw new Error("没有进行中的历史转换");
  let after = "0";
  for (;;) {
    const page = await migrationItems(saved.status.id, after), pending = page.items.filter(item => !item.converted);
    if (pending.length) {
      for (const item of pending) await dispatch(account, conversation.id, await encryptItem(account, conversation, saved.status, item));
      return refreshMigration(account, conversation.id);
    }
    if (!page.hasMore) return saved;
    if (page.next === after) throw new Error("历史转换分页未前进"); after = page.next;
  }
}
export async function verifyAndFinishMigration(account: string, conversation: Conversation, file: RecoveryFile, recoveryDigest: string) {
  const saved = await refreshMigration(account, conversation.id);
  if (saved.pending) throw new Error("请先确认原转换请求");
  if (!saved.status || saved.status.remaining !== 0) throw new Error("尚有未转换项目");
  if (saved.status.recoveryDigest !== recoveryDigest) throw new Error("请选择开始转换时核对的恢复文件");
  const reader = await recoveryReader(file), manifest = [];
  try {
    let after = "0";
    for (;;) {
      const page = await migrationItems(saved.status.id, after, true);
      for (const item of page.items) {
        if (!item.converted) throw new Error("验读清单仍有未转换记录");
        const record = item.record, attachmentDigests: { index: number; digest: string }[] = [], sourceAttachmentHashes: string[] = [];
        const convertedHere = record.historyMigration?.id === saved.status.id;
        let originalRecord: Message | ContactIntent = record;
        let plain: Message | ContactIntent = record;
        if (item.kind === "message") {
          plain = await projectMessage(record as Message, conversation, reader.unseal);
          if (plain.cryptoError) throw new Error(`消息 ${item.id} 独立恢复失败：${plain.cryptoError}`);
          for (const a of (plain as Message).attachments) {
            const cipher = await downloadBytes(`/chat/messages/${item.id}/attachments/${a.index}?revision=${item.revision}`);
            const bytes = await decodeAttachment(plain as Message, a, cipher);
            const plainHash = await digest(bytes);
            attachmentDigests.push({ index: a.index, digest: plainHash });
            sourceAttachmentHashes.push(convertedHere ? plainHash : await digest(cipher));
          }
          if (convertedHere) originalRecord = { ...record as Message, text: plain.text, attachments: (plain as Message).attachments.map(({ mediaEncryptionInfo: _secret, ...a }) => a), sealed: null, historyMigration: null, audienceRevision: item.originalAudienceRevision ?? null };
        } else {
          plain = await projectIntent(record as ContactIntent, conversation, reader.unseal);
          if (plain.cryptoError) throw new Error(`约定 ${item.id} 独立恢复失败：${plain.cryptoError}`);
          if (convertedHere) originalRecord = { ...record as ContactIntent, content: plain.content, sealed: null, historyMigration: null, audienceRevision: item.originalAudienceRevision ?? null };
        }
        const source = item.kind === "message" ? [originalRecord, sourceAttachmentHashes] : originalRecord;
        if (await digestJson(source) !== item.sourceDigest) throw new Error(`记录 ${item.id} 从恢复文件读取的内容与预览原内容不一致，未完成转换`);
        manifest.push({ kind: item.kind, id: item.id, revision: item.revision, sourceDigest: item.sourceDigest, textDigest: await digest(new TextEncoder().encode("text" in plain ? plain.text : plain.content)), attachments: attachmentDigests });
      }
      if (!page.hasMore) break;
      if (page.next === after) throw new Error("验读分页未前进"); after = page.next;
    }
  } finally { reader.close(); }
  const verificationDigest = await digestJson({ id: saved.status.id, items: manifest });
  return dispatch(account, conversation.id, { path: `/crypto/history-migrations/${saved.status.id}/finish`, method: "POST", body: { requestId: crypto.randomUUID(), deviceId: device(account), verificationDigest } });
}
