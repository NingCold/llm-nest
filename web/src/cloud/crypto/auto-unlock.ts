import { invoke } from "@tauri-apps/api/core";
import { IS_DESKTOP } from "@/lib/desktop";
import { server, ConnectionError, HttpError } from "../api";
import { cryptoDevice, cryptoGeneration, CryptoSessionChanged, deviceStatus, localCryptoDevice, lockCrypto, recoveryConfirmed, unlockCrypto, verifyCryptoPassphrase } from "./engine";

const lifetime = 30 * 24 * 60 * 60 * 1000;
const databaseName = "llmn-crypto-auto-unlock";
type Scope = { owner: string; id: string; deviceId: string; account: string; ownerRevision: number };
type Saved = { id: string; owner: string; expiresAt: number; key: CryptoKey; iv: Uint8Array; ciphertext: ArrayBuffer };
type Material = { expiresAt: number; secret: string };
const revisions = new Map<string, number>();
const ownerRevisions = new Map<string, number>();
const writes = new Map<string, Promise<unknown>>();
const changed = () => window.dispatchEvent(new Event("llmn-crypto-change"));
const ownerOf = (account: string) => JSON.stringify([server, account]);
const pauseKey = (scope: Scope) => `llmn-crypto-auto-paused:${scope.id}`;
function scopeOf(account: string): Scope | undefined {
  const deviceId = localCryptoDevice(account);
  if (!deviceId) return;
  const owner = ownerOf(account);
  return { owner, id: JSON.stringify([server, account, deviceId]), deviceId, account, ownerRevision: ownerRevisions.get(owner) ?? 0 };
}
function bump(scope: Scope) { const revision = (revisions.get(scope.id) ?? 0) + 1; revisions.set(scope.id, revision); return revision; }
function scopeCurrent(scope: Scope, revision: number) {
  return revisions.get(scope.id) === revision && (ownerRevisions.get(scope.owner) ?? 0) === scope.ownerRevision && ownerOf(scope.account) === scope.owner && localCryptoDevice(scope.account) === scope.deviceId;
}
function unchanged(scope: Scope, revision: number, epoch: number) {
  return scopeCurrent(scope, revision) && cryptoGeneration() === epoch;
}
function write<T>(scope: Scope, work: () => Promise<T>) {
  const pending = (writes.get(scope.id) ?? Promise.resolve()).catch(() => {}).then(work);
  writes.set(scope.id, pending);
  void pending.finally(() => { if (writes.get(scope.id) === pending) writes.delete(scope.id); }).catch(() => {});
  return pending;
}
async function database() {
  return new Promise<IDBDatabase>((resolve, reject) => {
    const open = indexedDB.open(databaseName, 1);
    open.onupgradeneeded = () => open.result.createObjectStore("materials", { keyPath: "id" });
    open.onsuccess = () => resolve(open.result);
    open.onerror = () => reject(new Error("无法读取本机自动解锁存储"));
  });
}
async function transaction<T>(mode: IDBTransactionMode, work: (store: IDBObjectStore) => IDBRequest<T>): Promise<T> {
  const db = await database();
  try {
    return await new Promise<T>((resolve, reject) => {
      const tx = db.transaction("materials", mode), request = work(tx.objectStore("materials"));
      tx.oncomplete = () => resolve(request.result);
      tx.onerror = tx.onabort = () => reject(new Error("本机自动解锁存储操作失败"));
    });
  } finally { db.close(); }
}
async function supported() {
  if (IS_DESKTOP) return invoke<boolean>("cloud_unlock_support");
  return typeof indexedDB !== "undefined" && typeof indexedDB.databases === "function" && !!globalThis.crypto?.subtle;
}
async function read(scope: Scope): Promise<Saved | Material | undefined> {
  if (IS_DESKTOP) return (await invoke<Material | null>("cloud_unlock_load", nativeScope(scope))) ?? undefined;
  return transaction<Saved | undefined>("readonly", store => store.get(scope.id));
}
async function remove(scope: Scope) {
  if (IS_DESKTOP) await invoke("cloud_unlock_remove", nativeScope(scope));
  else await transaction("readwrite", store => store.delete(scope.id));
}
function nativeScope(scope: Scope) { return { deviceId: scope.deviceId, expectedServer: JSON.parse(scope.owner)[0] as string, expectedAccountId: scope.account }; }
function expired(saved: Saved | Material) { return !Number.isFinite(saved.expiresAt) || saved.expiresAt <= Date.now(); }
async function secretOf(scope: Scope, saved: Saved | Material) {
  if (IS_DESKTOP) return (saved as Material).secret;
  const material = saved as Saved;
  const value = await crypto.subtle.decrypt({ name: "AES-GCM", iv: material.iv as Uint8Array<ArrayBuffer>, additionalData: new TextEncoder().encode(JSON.stringify([scope.id, material.expiresAt])) }, material.key, material.ciphertext);
  return new TextDecoder().decode(value);
}

export async function autoUnlockInfo(accountId: string): Promise<{ supported: boolean; enabled: boolean; paused: boolean; expiresAt?: number }> {
  const available = await supported(), scope = scopeOf(accountId);
  if (!available || !scope) return { supported: available, enabled: false, paused: false };
  const saved = await read(scope);
  if (saved && expired(saved)) { await write(scope, async () => { const latest = await read(scope); if (latest && expired(latest)) await remove(scope); }); return { supported: true, enabled: false, paused: localStorage.getItem(pauseKey(scope)) === "1" }; }
  return { supported: true, enabled: !!saved, paused: localStorage.getItem(pauseKey(scope)) === "1", ...(saved ? { expiresAt: saved.expiresAt } : {}) };
}

export async function enableAutoUnlock(accountId: string, passphrase: string): Promise<void> {
  const scope = scopeOf(accountId);
  if (!scope || cryptoDevice(accountId) !== scope.deviceId) throw new Error("请先手动解锁现有可信设备");
  const revision = bump(scope), epoch = cryptoGeneration();
  try {
    if (!await supported()) throw new Error();
    const status = await deviceStatus();
    if (!status.trusted || !recoveryConfirmed(accountId)) throw new Error();
    await verifyCryptoPassphrase(accountId, passphrase);
    if (!unchanged(scope, revision, epoch)) throw new Error();
    await write(scope, async () => {
      if (!unchanged(scope, revision, epoch)) throw new Error();
      if (IS_DESKTOP) await invoke("cloud_unlock_save", { ...nativeScope(scope), secret: passphrase });
      else {
        const key = await crypto.subtle.generateKey({ name: "AES-GCM", length: 256 }, false, ["encrypt", "decrypt"]);
        const iv = crypto.getRandomValues(new Uint8Array(12)), expiresAt = Date.now() + lifetime;
        const ciphertext = await crypto.subtle.encrypt({ name: "AES-GCM", iv, additionalData: new TextEncoder().encode(JSON.stringify([scope.id, expiresAt])) }, key, new TextEncoder().encode(passphrase));
        if (!unchanged(scope, revision, epoch)) throw new Error();
        await transaction("readwrite", store => store.put({ id: scope.id, owner: scope.owner, key, iv, ciphertext, expiresAt } satisfies Saved));
      }
      if (!unchanged(scope, revision, epoch)) { await remove(scope); throw new Error(); }
    });
    localStorage.removeItem(pauseKey(scope));
    changed();
  } catch { throw new Error("无法启用自动解锁，请核对本机口令、可信设备和已确认的恢复材料"); }
}

export async function disableAutoUnlock(accountId: string): Promise<void> {
  const scope = scopeOf(accountId);
  if (!scope) return;
  bump(scope);
  try { await write(scope, () => remove(scope)); }
  catch { throw new Error("关闭自动解锁失败，请重试"); }
  changed();
}

export async function tryAutoUnlock(accountId: string): Promise<boolean> {
  const scope = scopeOf(accountId);
  if (!scope || localStorage.getItem(pauseKey(scope)) === "1") return false;
  const revision = bump(scope), epoch = cryptoGeneration();
  if (!await supported() || !unchanged(scope, revision, epoch)) return false;
  let saved: Saved | Material | undefined;
  try {
    saved = await read(scope);
    if (!unchanged(scope, revision, epoch)) return false;
    if (!saved) return false;
    if (expired(saved)) throw new Error();
    const passphrase = await secretOf(scope, saved);
    if (!unchanged(scope, revision, epoch)) return false;
    const status = await unlockCrypto(accountId, passphrase, false, { generation: epoch, deviceId: scope.deviceId, isCurrent: () => scopeCurrent(scope, revision) });
    if (!status.trusted || !recoveryConfirmed(accountId)) { lockCrypto(); throw new Error(); }
    return true;
  } catch (error) {
    // An account switch or manual lock invalidates in-flight work without deleting its owner's setting.
    if (error instanceof CryptoSessionChanged || (ownerRevisions.get(scope.owner) ?? 0) !== scope.ownerRevision || ownerOf(accountId) !== scope.owner || localCryptoDevice(accountId) !== scope.deviceId || revisions.get(scope.id) !== revision) return false;
    if (error instanceof ConnectionError) throw new Error("自动解锁暂不可用，请在联网后重试或手动解锁");
    await write(scope, () => remove(scope));
    changed();
    throw new Error("自动解锁材料已过期、失效或设备权限已变化，已关闭自动解锁；请手动解锁");
  }
}

export async function unlockManually(accountId: string, passphrase: string): Promise<void> {
  const scope = scopeOf(accountId);
  if (scope) bump(scope);
  try { await unlockCrypto(accountId, passphrase); }
  catch (error) {
    if (error instanceof CryptoSessionChanged) throw error;
    if (error instanceof ConnectionError) throw new Error("连接暂不可用，请重试解锁");
    if (error instanceof HttpError) throw new Error("设备权限验证失败，请重新登录或使用恢复文件");
    throw new Error("本机解锁失败，请核对本机口令；资料缺失时请使用恢复文件");
  }
  if (scope && ownerOf(accountId) === scope.owner && localCryptoDevice(accountId) === scope.deviceId) localStorage.removeItem(pauseKey(scope));
}
export function pauseAutoUnlock(accountId: string): void {
  const scope = scopeOf(accountId);
  if (!scope) return;
  bump(scope);
  localStorage.setItem(pauseKey(scope), "1");
  changed();
}
export async function forgetAutoUnlock(accountId: string): Promise<void> {
  const scope = scopeOf(accountId);
  const owner = ownerOf(accountId);
  ownerRevisions.set(owner, (ownerRevisions.get(owner) ?? 0) + 1);
  if (scope) bump(scope);
  try {
    if (IS_DESKTOP) { if (scope) await write(scope, () => remove(scope)); }
    else {
      // Device replacement must not leave an older browser convenience key behind on logout.
      const saved = await transaction<Saved[]>("readonly", store => store.getAll());
      for (const material of saved.filter(item => item.owner === owner)) {
        const previous = { ...scope, owner, account: accountId, ownerRevision: 0, id: material.id, deviceId: JSON.parse(material.id)[2] };
        bump(previous);
        await write(previous, () => remove(previous));
        localStorage.removeItem(pauseKey(previous));
      }
      if (scope) await write(scope, () => remove(scope));
    }
  }
  catch { throw new Error("清除自动解锁材料失败，请重试"); }
  if (scope) localStorage.removeItem(pauseKey(scope));
  changed();
}
