import { request, server, HttpError, ConnectionError } from "../api";
import { cryptoDevice, matrixUser, prepareEncryption, seal, unseal, signEnvelope, syncCrypto } from "./engine";
import type { Sealed } from "./content";
import type { SourceRevision } from "../types";
export type ResourceKind = "profile" | "memory" | "summary" | "summarySettings";
export interface Resource { id: string; aiId: string; conversationId: string | null; kind: ResourceKind; revision: string; sealed: Sealed | null; sources: SourceRevision[]; tombstone: "forgotten" | "stale" | null; applied: { bindingVersion: string; revision: string; status: "applied" | "rejected" } | null }
interface Command { owner: string; id: string; body: { requestId: string; deviceId: string; aiId: string; conversationId: string | null; kind: ResourceKind; expectedRevision: string; expectedAudienceRevision: string; sealed: Sealed | null; sources: SourceRevision[]; tombstone: Resource["tombstone"] }; conflict?: string }
const scope = (ai: string, kind: string, cid: string | null) => `${ai}.${kind}.${cid ?? "global"}`;
const owner = (account: string) => JSON.stringify([server, account]);
let database: Promise<IDBDatabase> | undefined;
function db() {
  return database ??= new Promise((resolve, reject) => {
    const open = indexedDB.open("llmn-encrypted-resources", 1);
    open.onupgradeneeded = () => { open.result.createObjectStore("cache"); open.result.createObjectStore("pending"); };
    open.onerror = () => reject(open.error); open.onsuccess = () => resolve(open.result);
  });
}
async function storage<T>(table: "cache" | "pending", mode: IDBTransactionMode, action: (store: IDBObjectStore) => IDBRequest<T>): Promise<T> {
  const transaction = (await db()).transaction(table, mode);
  const result = action(transaction.objectStore(table));
  return new Promise((resolve, reject) => { transaction.oncomplete = () => resolve(result.result); transaction.onerror = () => reject(transaction.error); transaction.onabort = () => reject(transaction.error); });
}
const cacheKey = (account: string, ai: string, kind: string, cid: string | null) => JSON.stringify([owner(account), ai, kind, cid]);
async function cacheList(account: string, ai: string, kind: ResourceKind, cid: string | null, items: Resource[]) { await storage("cache", "readwrite", s => s.put(items, cacheKey(account, ai, kind, cid))); }
export async function loadResources(account: string, aiId: string, kind: ResourceKind, conversationId: string | null) {
  const path = `/crypto/resources?aiId=${aiId}&kind=${kind}${conversationId ? `&conversationId=${conversationId}` : ""}`;
  try {
    const { items } = await request<{ items: Resource[] }>(path);
    await cacheList(account, aiId, kind, conversationId, items);
    return { items, offline: false };
  } catch (error) {
    if (error instanceof HttpError) throw error;
    const items = await storage<Resource[] | undefined>("cache", "readonly", s => s.get(cacheKey(account, aiId, kind, conversationId)));
    if (!items) throw error;
    return { items, offline: true };
  }
}
export async function decryptResource<T>(resource: Resource): Promise<T | null> {
  if (!resource.sealed || resource.tombstone) return null;
  const roomId = `!${scope(resource.aiId, resource.kind, resource.conversationId)}:llmn`;
  const encrypted = resource.sealed;
  if (encrypted.roomId !== roomId || encrypted.version !== 1) throw new Error("资料密文范围不匹配");
  const sender = encrypted.sender.match(/^@([^:]+):llmn$/)?.[1];
  if (!sender) throw new Error("资料发送设备身份无效");
  const event = await unseal(encrypted);
  const content = event.content;
  if (event.type !== "com.llmn.content.v1" || content.version !== 1 || content.server !== server || content.kind !== "resource" || content.scope !== scope(resource.aiId, resource.kind, resource.conversationId) || content.id !== resource.id || content.messageId !== resource.id || content.revision !== resource.revision || typeof content.expectedRevision !== "string" || (BigInt(content.expectedRevision) + 1n).toString() !== resource.revision || content.conversationId !== resource.conversationId || content.senderId !== sender || encrypted.deviceId !== encrypted.content.device_id || encrypted.eventId !== `$${resource.id}.${resource.revision}.final`) throw new Error("资料完整性或版本校验失败");
  return content.data as T;
}
export async function pendingResources(account: string) { return (await storage<Command[]>("pending", "readonly", s => s.getAll())).filter(c => c.owner === owner(account)); }
async function remember(account: string, resource: Resource) {
  const key = cacheKey(account, resource.aiId, resource.kind, resource.conversationId);
  const tx = (await db()).transaction("cache", "readwrite"), table = tx.objectStore("cache"), read = table.get(key);
  read.onsuccess = () => {
    const items: Resource[] = read.result ?? [];
    const current = items.find(r => r.id === resource.id);
    if (!current || BigInt(current.revision) <= BigInt(resource.revision)) table.put([...items.filter(r => r.id !== resource.id), resource], key);
  };
  await new Promise<void>((resolve, reject) => { tx.oncomplete = () => resolve(); tx.onabort = tx.onerror = () => reject(tx.error); });
}
export async function synchronizeResources(account: string) {
  const identity = owner(account);
  await syncCrypto();
  for (const command of await pendingResources(account)) {
    if (command.conflict) continue;
    try {
      const result = await request<Resource>(`/crypto/resources/${command.id}`, "PUT", command.body);
      if (owner(account) !== identity || !cryptoDevice(account)) return;
      await remember(account, result);
      await storage("pending", "readwrite", s => s.delete(JSON.stringify([identity, command.id])));
    } catch (error) {
      if (error instanceof HttpError && [400, 403, 404, 409, 422].includes(error.status)) {
        command.conflict = error.message;
        await storage("pending", "readwrite", s => s.put(command, JSON.stringify([identity, command.id])));
      } else throw error;
    }
  }
}
export async function saveResource(account: string, target: { id: string; aiId: string; kind: ResourceKind; conversationId: string | null; expectedRevision: string; expectedAudienceRevision: string; users: string[]; sources?: SourceRevision[]; tombstone?: Resource["tombstone"] }, data: unknown) {
  const deviceId = cryptoDevice(account);
  const identity = owner(account);
  if (!deviceId) throw new Error("请先解锁已授权的加密设备");
  const existing = (await pendingResources(account)).find(c => c.id === target.id);
  if (existing) throw new Error("该条资料尚有待同步或冲突的保存，请先核对原意图");
  const resourceScope = scope(target.aiId, target.kind, target.conversationId), roomId = `!${resourceScope}:llmn`, revision = (BigInt(target.expectedRevision) + 1n).toString();
  let sealed: Sealed | null = null;
  if (!target.tombstone) {
    let offline = false;
    try { await syncCrypto(); } catch (error) { if (!(error instanceof ConnectionError)) throw error; offline = true; }
    await prepareEncryption(roomId, target.users, offline, [target.aiId, account]);
    if (!offline) {
      const current = await request<{ accounts: string[]; revision: string }>(`/crypto/resource-participants?aiId=${target.aiId}&kind=${target.kind}${target.conversationId ? `&conversationId=${target.conversationId}` : ""}`);
      if (current.revision !== target.expectedAudienceRevision) throw new Error("资料授权名单或设备已变化，请重新读取并核对后保存；编辑内容仍保留");
    }
    const content = { version: 1, server, kind: "resource", conversationId: target.conversationId, messageId: target.id, senderId: account, revision, text: "", replyTo: null, mentions: [], runId: null, sequence: null, scope: resourceScope, id: target.id, expectedRevision: target.expectedRevision, expectedAudienceRevision: target.expectedAudienceRevision, data };
    const cipher = await seal(roomId, target.users, "com.llmn.content.v1", content, true, [target.aiId, account]);
    sealed = await signEnvelope({ version: 1, roomId, sender: matrixUser(account), deviceId, eventId: `$${target.id}.${revision}.final`, content: cipher });
  }
  if (owner(account) !== identity || cryptoDevice(account) !== deviceId) throw new Error("加密设备已锁定或账号已切换，未保存此编辑");
  const command: Command = { owner: identity, id: target.id, body: { requestId: crypto.randomUUID(), deviceId, aiId: target.aiId, conversationId: target.conversationId, kind: target.kind, expectedRevision: target.expectedRevision, expectedAudienceRevision: target.expectedAudienceRevision, sealed, sources: target.sources ?? [], tombstone: target.tombstone ?? null } };
  await storage("pending", "readwrite", s => s.put(command, JSON.stringify([command.owner, command.id])));
  try { await synchronizeResources(account); } catch (error) { if (error instanceof HttpError) throw error; }
}
export async function discardResourceIntent(account: string, id: string) { await storage("pending", "readwrite", s => s.delete(JSON.stringify([owner(account), id]))); }
export function resourceStatus(resource: Resource) {
  if (resource.tombstone === "forgotten") return resource.applied?.revision === resource.revision && resource.applied.status === "applied" ? "平台已停用，节点副本已清理" : "平台已停用，待节点清理";
  if (resource.tombstone) return "来源已失效";
  if (resource.applied?.revision !== resource.revision) return "云端已保存，待节点应用";
  return resource.applied.status === "applied" ? "节点已应用当前版本" : "云端已保存，节点无法应用；新运行已暂停";
}
