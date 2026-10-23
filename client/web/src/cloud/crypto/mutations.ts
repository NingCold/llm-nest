import { request, server, HttpError } from "../api";
import { encryptMessage, projectMessage, type Sealed } from "./content";
import type { Conversation, Message } from "../types";
interface Command { requestId: string; expectedRevision: string; text?: string; sealed?: Sealed }
let database: Promise<IDBDatabase> | undefined;
function db() { return database ??= new Promise((resolve, reject) => { const r = indexedDB.open("llmn-encrypted-mutations", 1); r.onupgradeneeded = () => r.result.createObjectStore("commands"); r.onsuccess = () => resolve(r.result); r.onerror = () => reject(r.error); }); }
async function record<T>(mode: IDBTransactionMode, action: (s: IDBObjectStore) => IDBRequest<T>) { const tx = (await db()).transaction("commands", mode); const r = action(tx.objectStore("commands")); return new Promise<T>((resolve, reject) => { tx.oncomplete = () => resolve(r.result); tx.onabort = tx.onerror = () => reject(tx.error); }); }
export async function mutateEncrypted(conversation: Conversation, message: Message, account: string, withdraw: boolean, text?: string) {
  const key = JSON.stringify([server, account, message.id, withdraw ? "withdraw" : "edit"]);
  let body = await record<Command | undefined>("readonly", s => s.get(key));
  if (body?.sealed && message.sealed && JSON.stringify(body.sealed) === JSON.stringify(message.sealed)) {
    // The refreshed authoritative message proves this exact edit was applied.
    await record("readwrite", s => s.delete(key)); body = undefined;
  }
  if (body?.sealed && !withdraw) {
    const frozen = await projectMessage({ ...message, revision: (BigInt(body.expectedRevision) + 1n).toString(), sealed: body.sealed });
    if (frozen.cryptoError) throw new Error("原加密修改尚未确认且当前无法解密。请解锁后重试，不能用新修改替换原命令。");
    if (text !== frozen.text) throw new Error(`上次修改结果尚未确认，已保留原修改，不能用新输入覆盖。请刷新核对，或使用原修改重试：${frozen.text}`);
  }
  const first = !body;
  if (!body) {
    body = { requestId: crypto.randomUUID(), expectedRevision: message.revision };
    if (!withdraw) { body.text = ""; body.sealed = await encryptMessage(conversation, account, message.id, text ?? "", (BigInt(message.revision) + 1n).toString(), message.replyTo, message.mentions); }
    await record("readwrite", s => s.put(body, key));
  }
  try { await request(`/chat/messages/${message.id}${withdraw ? "/withdraw" : ""}`, withdraw ? "POST" : "PATCH", body); }
  catch (error) { if (first && error instanceof HttpError && [400, 403, 404, 409, 413, 422].includes(error.status)) await record("readwrite", s => s.delete(key)); throw error; }
  await record("readwrite", s => s.delete(key));
}
