import { request, server, HttpError, ConnectionError } from "../api";
import { ed25519 } from "@noble/curves/ed25519.js";
import type * as Matrix from "@matrix-org/matrix-sdk-crypto-wasm";
import { canonicalJson } from "./canonical";
import { verifyEnvelope, unsignedEnvelope, type SignedEnvelope } from "./envelope";

type Sdk = typeof Matrix;
type Session = { authorized?: boolean; localVerified?: boolean; liveBackup?: string | null; sdk: Sdk; machine: Matrix.OlmMachine; account: string; owner: string; deviceId: string; cursor: string; deviceListsVersion?: string; pendingWarning?: string; pendingChange?: boolean; flushing?: Promise<void>; authorization?: { secrets: unknown; roomKeys: unknown; senderDevice: string } };
let active: Session | undefined;
let generation = 0;
let timingAttempt = 0;
let syncing: Promise<void> | undefined;
type BackupInfo = {version: string; algorithm: string; sealed?: boolean; auth_data: {public_key: string}};
let preparedRecovery: { session: Session; version: string; command: Record<string,unknown>; key: string; backupVersion: string } | undefined;
const ownerOf = (account: string) => JSON.stringify([server, account]);
const metadataKey = (account: string) => `llmn-crypto-device:${ownerOf(account)}`;
const pendingDeviceKey = (account: string) => `llmn-crypto-pending-device:${ownerOf(account)}`;
const replacements = new WeakSet<Session>();
export function hasPendingCryptoDevice(account: string) { return localStorage.getItem(pendingDeviceKey(account)) !== null; }
function persistDevice(s: Session) {
  current(s);
  const primary = metadataKey(s.account), pending = pendingDeviceKey(s.account);
  const metadata = JSON.stringify({deviceId: s.deviceId, cursor: s.cursor, deviceListsVersion: s.deviceListsVersion});
  if (replacements.has(s)) {
    const stored = localStorage.getItem(pending);
    if (!stored || JSON.parse(stored).deviceId !== s.deviceId) return;
    if (trustedCryptoDevice(s.account) !== s.deviceId) { localStorage.setItem(pending, metadata); return; }
    localStorage.setItem(primary, metadata);
    localStorage.removeItem(pending);
    replacements.delete(s);
  } else if (localCryptoDevice(s.account) === s.deviceId) localStorage.setItem(primary, metadata);
}
export const matrixUser = (account: string) => `@${account}:llmn`;
export const matrixRoom = (conversation: string) => `!${conversation}:llmn`;
const changed = () => window.dispatchEvent(new Event("llmn-crypto-change"));
const masterFingerprint = (value: string) => { try { return String(Object.values(JSON.parse(value).keys)[0]); } catch { return value; } };

export function lockCrypto() {
  generation++;
  const wasUnlocked = !!active;
  active?.machine.close();
  active = undefined;
  syncing = undefined;
  preparedRecovery = undefined;
  if (wasUnlocked) changed();
}
function session() {
  if (!active || active.owner !== ownerOf(active.account)) throw new Error("请先在可信设备与恢复中解锁本机加密资料");
  return active;
}
export class CryptoSessionChanged extends Error {}
function current(s: Session) { if (active !== s || s.owner !== ownerOf(s.account)) throw new CryptoSessionChanged("加密设备已锁定或账号已切换"); }
export function cryptoDevice(account: string) { return active?.owner === ownerOf(account) ? active.deviceId : undefined; }
export function trustedCryptoDevice(account: string): string | null {
  return active?.owner === ownerOf(account) && active.authorized === true && active.localVerified === true ? active.deviceId : null;
}
function admissionKey(s: Session) { return `llmn-device-admission:${s.owner}:${s.deviceId}`; }
function admission(s: Session, allowed: boolean) {
  current(s);
  s.authorized = allowed;
  localStorage.setItem(admissionKey(s), allowed ? s.machine.identityKeys.ed25519.toBase64() : "");
}
async function deviceSignature(s: Session, statement: string): Promise<string> {
  current(s);
  const signed = await s.machine.sign(statement);
  current(s);
  const signature = JSON.parse(signed.asJSON())[matrixUser(s.account)]?.[`ed25519:${s.deviceId}`];
  if (typeof signature !== "string") throw new Error("当前设备不能签署授权");
  return signature;
}
const statement = (account: string, action: string, data: unknown) => "llmn.authority.v1\n" + canonicalJson([account, action, data]);
const proof = (s: Session, action: string, body: unknown) => deviceSignature(s, statement(s.account, action, [s.deviceId, body]));
async function signedCommand(s: Session, action: string, body: Record<string, unknown>) {
  const command = { ...body, deviceId: s.deviceId };
  return { ...command, proof: await proof(s, action, command) };
}
export async function signDeviceCommand(accountId: string, action: string, body: Record<string,unknown>) {
  const s = session();
  if (s.account !== accountId || trustedCryptoDevice(accountId) !== s.deviceId || (body.deviceId !== undefined && body.deviceId !== s.deviceId)) {
    throw new Error("当前可信设备与待签署的命令不匹配");
  }
  return signedCommand(s, action, body);
}
export async function signLocalExportChallenge(accountId: string, value: string) {
  const s = session();
  if (!value.startsWith("llmnest.local-ai.recovery-export.v1\n") || trustedCryptoDevice(accountId) !== s.deviceId) throw new Error("仅可信设备可导出恢复资料");
  const signature = await deviceSignature(s, value);
  if (trustedCryptoDevice(accountId) !== s.deviceId) throw new CryptoSessionChanged("设备授权已变化");
  return signature;
}
export async function revokeCryptoDevice(accountId: string, deviceId: string) {
  const s = session();
  await request(`/crypto/devices/${encodeURIComponent(deviceId)}/revoke?accountId=${encodeURIComponent(accountId)}`, "POST", await signedCommand(s, "revoke", { accountId, target: deviceId }));
  await syncCrypto();
}
export function consumeCryptoWarning(account: string) {
  if (active?.owner !== ownerOf(account)) return undefined;
  const warning = active.pendingWarning;
  active.pendingWarning = undefined;
  return warning;
}
export function hasCryptoDevice(account: string) { return localStorage.getItem(metadataKey(account)) !== null; }
export function localCryptoDevice(account: string): string | undefined {
  const saved = localStorage.getItem(metadataKey(account));
  return saved ? (JSON.parse(saved) as { deviceId: string }).deviceId : undefined;
}
export function cryptoGeneration() { return generation; }
async function cryptoStoreName(account: string, deviceId: string) {
  const digest = await crypto.subtle.digest("SHA-256", new TextEncoder().encode(JSON.stringify([ownerOf(account), deviceId])));
  return `llmn-crypto-${Array.from(new Uint8Array(digest), b => b.toString(16).padStart(2, "0")).join("")}`;
}
export function prewarmCrypto(account: string): () => void {
  if (!hasCryptoDevice(account)) return () => {};
  let cancelled = false, connection: IDBDatabase | undefined;
  const owner = ownerOf(account);
  const clock = typeof performance === "undefined" ? undefined : performance;
  const started = clock?.now() ?? 0;
  const names = ["sdkReady", "indexedDbReady", "holdReady", "failed"] as const;
  for (const name of names) clock?.clearMeasures(`llmn.crypto.prewarm.${name}`);
  const current = () => !cancelled && ownerOf(account) === owner;
  const measured = (name: typeof names[number], start: number) => {
    if (current()) clock?.measure(`llmn.crypto.prewarm.${name}`, { start, end: clock.now() });
  };
  const warmSdk = async () => {
    const sdk = await import("@matrix-org/matrix-sdk-crypto-wasm");
    await sdk.initAsync();
    measured("sdkReady", started);
  };
  const warmIndexedDb = async () => {
    const deviceId = localCryptoDevice(account);
    if (!deviceId) return;
    const name = `${await cryptoStoreName(account, deviceId)}::matrix-sdk-crypto-meta`;
    if (!current()) return;
    const existing = await indexedDB.databases();
    measured("indexedDbReady", started);
    if (!current() || !existing.some(db => db.name === name)) return;
    await new Promise<void>((resolve, reject) => {
      const request = indexedDB.open(name);
      // A deletion between enumeration and open must not recreate an SDK store.
      request.onupgradeneeded = () => request.transaction!.abort();
      request.onerror = () => reject(request.error);
      request.onsuccess = () => {
        const db = request.result;
        if (!current()) { db.close(); resolve(); return; }
        // Keep the browser's backing store alive without reading cipher or key records.
        connection = db;
        db.onversionchange = () => { db.close(); if (connection === db) connection = undefined; };
        measured("holdReady", started);
        resolve();
      };
    });
  };
  // Speculative preparation never changes lock state; real unlock keeps its own error path.
  void Promise.all([warmSdk(), warmIndexedDb()]).catch(() => measured("failed", started));
  return () => { cancelled = true; connection?.close(); connection = undefined; };
}
async function requireCryptoStore(storeName: string) {
  const names = new Set((await indexedDB.databases()).map(db => db.name));
  if (!names.has(`${storeName}::matrix-sdk-crypto-meta`) || !names.has(`${storeName}::matrix-sdk-crypto`)) throw new Error("本机加密库已不存在，请关闭自动解锁并使用原恢复流程");
}
export async function verifyCryptoPassphrase(account: string, passphrase: string) {
  const s = session();
  if (s.account !== account) throw new Error("账号已切换，请重新解锁");
  const storeName = await cryptoStoreName(account, s.deviceId);
  await requireCryptoStore(storeName);
  const handle = await s.sdk.StoreHandle.open(storeName, passphrase);
  handle.free();
  current(s);
}

async function sendOutgoing(s: Session, outgoing: Matrix.OutgoingRequest) {
  current(s);
  const body = JSON.parse(outgoing.body);
  if (outgoing.type === s.sdk.RequestType.ToDevice) {
    const toDevice = outgoing as Matrix.ToDeviceRequest;
    body.event_type = toDevice.event_type;
    body.txn_id = toDevice.txn_id;
  }
  const kinds = ["keysUpload", "keysQuery", "keysClaim", "toDevice", "signatures", "roomMessage", "keysBackup"];
  if (outgoing.type === s.sdk.RequestType.KeysBackup) body.version = (outgoing as Matrix.KeysBackupRequest).version;
  const response = await request("/crypto/requests", "POST", { deviceId: s.deviceId, requestId: outgoing.id ?? crypto.randomUUID(), kind: kinds[outgoing.type], body, ...([s.sdk.RequestType.KeysBackup,s.sdk.RequestType.SignatureUpload].includes(outgoing.type) ? {proof:await proof(s,kinds[outgoing.type],body)} : {}) });
  current(s);
  if (outgoing.id) await s.machine.markRequestAsSent(outgoing.id, outgoing.type, JSON.stringify(response));
  current(s);
  if (outgoing.type !== s.sdk.RequestType.KeysBackup) s.pendingChange = true;
}
async function flushInner(s: Session) {
  const queueKey = `llmn-crypto-outgoing:${s.owner}`;
  let queued = JSON.parse(localStorage.getItem(queueKey) ?? "[]") as { id: string; type?: Matrix.RequestType; body: Record<string, unknown>; kind: string }[];
  while (queued.length) {
    const outgoing = queued[0];
    let response: unknown;
    try { response = await request("/crypto/requests", "POST", { deviceId: s.deviceId, requestId: outgoing.id, kind: outgoing.kind, body: outgoing.body, ...(["signingKeys","signatures"].includes(outgoing.kind) ? {proof:await proof(s,outgoing.kind,outgoing.body)} : {}) }); }
    catch (error) {
      if (outgoing.body.llmn_purpose === "conversationHistory" && error instanceof HttpError && [403, 404, 409].includes(error.status)) {
        current(s);
        const latest = JSON.parse(localStorage.getItem(queueKey) ?? "[]") as typeof queued;
        localStorage.setItem(queueKey, JSON.stringify(latest.filter(item => item.body.llmn_purpose !== "conversationHistory" || item.body.llmn_conversation_id !== outgoing.body.llmn_conversation_id || item.body.llmn_expected_revision !== outgoing.body.llmn_expected_revision)));
      }
      throw error;
    }
    current(s);
    if (outgoing.type !== undefined) await s.machine.markRequestAsSent(outgoing.id, outgoing.type, JSON.stringify(response));
    current(s);
    s.pendingChange = true;
    queued = (JSON.parse(localStorage.getItem(queueKey) ?? "[]") as typeof queued).filter(item => item.id !== outgoing.id);
    localStorage.setItem(queueKey, JSON.stringify(queued));
  }
  // SDK acknowledgements can produce a subsequent key query; drain it before confirming sync.
  while (true) {
    current(s);
    const outgoing = await s.machine.outgoingRequests();
    if (!outgoing.length) break;
    for (const request of outgoing) await sendOutgoing(s, request);
  }
}
async function flush(s: Session) {
  while (s.flushing) await s.flushing;
  current(s);
  const work = flushInner(s).finally(() => { if (s.flushing === work) s.flushing = undefined; });
  s.flushing = work;
  return work;
}
function queueRequest(s: Session, request: { id: string; type?: Matrix.RequestType; kind: string; body: Record<string, unknown> }) {
  const key = `llmn-crypto-outgoing:${s.owner}`;
  const queued = JSON.parse(localStorage.getItem(key) ?? "[]") as { id: string }[];
  if (!queued.some(q => q.id === request.id)) queued.push(request);
  localStorage.setItem(key, JSON.stringify(queued));
}
function queueToDevice(s: Session, outgoing: Matrix.ToDeviceRequest) {
  queueRequest(s, { id: outgoing.id, type: outgoing.type, kind: "toDevice", body: { ...JSON.parse(outgoing.body), event_type: outgoing.event_type, txn_id: outgoing.txn_id } });
}
export async function syncCrypto() {
  if (syncing) return syncing;
  const s = session();
  const work = (async () => {
    const sync = await request<{ next_batch: string; to_device: { events: unknown[] }; device_lists: { changed: string[]; left: string[] }; device_one_time_keys_count: Record<string, number>; device_unused_fallback_key_types: string[]; llmn_authorized?: boolean; llmn_fingerprint?: string | null; llmn_backup_retired?: boolean; llmn_active_backup?: {version:string;publicKey:string} | null; llmn_device_lists_version?: string }>(`/crypto/sync?deviceId=${encodeURIComponent(s.deviceId)}&after=${encodeURIComponent(s.cursor)}${s.deviceListsVersion === undefined ? "" : `&deviceListsVersion=${encodeURIComponent(s.deviceListsVersion)}`}`).catch(error => {
      if (error instanceof HttpError && [401, 403].includes(error.status) && active === s) lockCrypto();
      throw error;
    });
    current(s);
    if (sync.llmn_authorized !== undefined) {
      const allowed = sync.llmn_authorized && sync.llmn_fingerprint === s.machine.identityKeys.ed25519.toBase64();
      if (s.authorized !== allowed) s.pendingChange = true;
      admission(s, allowed);
    }
    if (sync.llmn_active_backup !== undefined) {
      await selectBackup(s, sync.llmn_active_backup, sync.llmn_backup_retired === true);
    }
    await flush(s);
    current(s);
    if (sync.to_device.events.length || sync.device_lists.changed.length || sync.device_lists.left.length) s.pendingChange = true;
    const events = await s.machine.receiveSyncChanges(JSON.stringify(sync.to_device.events), new s.sdk.DeviceLists(sync.device_lists.changed.map(id => new s.sdk.UserId(id)), sync.device_lists.left.map(id => new s.sdk.UserId(id))), new Map(Object.entries(sync.device_one_time_keys_count)), new Set(sync.device_unused_fallback_key_types), new s.sdk.DecryptionSettings(s.sdk.TrustRequirement.CrossSigned));
    current(s);
    for (const processed of events) {
      current(s);
      if (!("encryptionInfo" in processed) || !("rawEvent" in processed) || !processed.encryptionInfo.isSenderVerified()) continue;
      const event = JSON.parse(processed.rawEvent);
      if (processed.encryptionInfo.sender.toString() !== event.sender) continue;
      if (event.type === "com.llmn.recovery.v1" && event.sender === matrixUser(s.account)) {
        if (processed.encryptionInfo.senderDevice?.toString() === event.content.senderDevice) s.authorization = event.content;
      } else if (event.type === "com.llmn.group-history.v1") {
        const group = await request<import("../types").Snapshot>("/chat/init");
        current(s);
        const conversation = group.conversations.find(c => c.id === event.content.conversationId);
        if (!conversation || conversation.revision !== event.content.revision || !conversation.members.some(m => matrixUser(m.account.id) === event.sender)) { window.dispatchEvent(new CustomEvent("llmn-crypto-warning", { detail: "群历史授权已变化，过期密钥分享已拒绝；请按当前成员重新分享" })); continue; }
        if (!Array.isArray(event.content.keys) || event.content.keys.some((key: { room_id: string }) => key.room_id !== matrixRoom(conversation.id))) { window.dispatchEvent(new CustomEvent("llmn-crypto-warning", { detail: "历史密钥包含其他范围，已拒绝导入" })); continue; }
        await s.machine.importExportedRoomKeys(JSON.stringify(event.content.keys), () => {});
      } else if (event.type === "com.llmn.ai-history.v1" && event.sender === matrixUser(event.content.aiId)) {
        const { chatRevisions, keys, audienceRevision } = event.content;
        const rooms = new Set<string>();
        if (chatRevisions && typeof chatRevisions === "object" && !Array.isArray(chatRevisions)) {
          for (const [id, revision] of Object.entries(chatRevisions)) {
            if (/^[a-f\d]{8}(?:-[a-f\d]{4}){3}-[a-f\d]{12}$/i.test(id) && typeof revision === "string" && /^[1-9]\d*$/.test(revision)) rooms.add(matrixRoom(id));
          }
        }
        if (!audienceRevision || !rooms.size || !Array.isArray(keys) || keys.some(key => !rooms.has(key?.room_id))) {
          window.dispatchEvent(new CustomEvent("llmn-crypto-warning", { detail: "AI 历史分享没有有效的聊天范围，请让节点重新分享" }));
          continue;
        }
        await s.machine.importExportedRoomKeys(JSON.stringify(event.content.keys), () => {});
      }
    }
    current(s);
    await flush(s);
    current(s);
    const backup = s.liveBackup===null || s.authorized===false ? null : await s.machine.backupRoomKeys();
    current(s);
    if (backup) await sendOutgoing(s, backup);
    current(s);
    s.cursor = sync.next_batch;
    s.deviceListsVersion = sync.llmn_device_lists_version;
    persistDevice(s);
    if (s.pendingChange) { s.pendingChange = false; changed(); }
  })().finally(() => { if (syncing === work) syncing = undefined; });
  syncing = work;
  return work;
}

export async function unlockCrypto(account: string, passphrase: string, create: boolean | "new-device" | "resume-device" = false, expected?: { generation: number; deviceId: string; isCurrent?: () => boolean }) {
  const attempt = ++timingAttempt;
  const clock = typeof performance === "undefined" ? undefined : performance;
  const timingNames = ["sdkImport", "sdkInit", "storeOpen", "storePrepare", "storeMetadataRead", "storeMetadata", "storeCipher", "storeDatabase", "machineLoad", "deviceRegister", "trackedUsers", "deviceStatus", "statusIdentity", "statusCrossSigning", "statusOwnDeviceTrust", "unlockReady", "backgroundSync"] as const;
  for (const name of timingNames) clock?.clearMeasures(`llmn.crypto.unlock.${name}`);
  const started = clock?.now() ?? 0;
  if (!passphrase.trim()) throw new Error("请输入本机加密库口令；它与登录密码和恢复材料分开保管");
  const saved = localStorage.getItem(create === "resume-device" ? pendingDeviceKey(account) : metadataKey(account));
  if (create === "resume-device" && !saved) throw new Error("未找到待授权设备，请重新建立设备");
  if (!saved && !create) throw new Error("此账号在当前设备没有加密资料，请明确创建新设备，再由可信设备或恢复包授权");
  if (saved && create === true) throw new Error("本机已有设备身份，请解锁，不能覆盖或重置");
  // Automatic unlock must never create a replacement SDK identity when its local store was removed.
  if (expected) {
    if (generation !== expected.generation || localCryptoDevice(account) !== expected.deviceId || expected.isCurrent?.() === false) throw new CryptoSessionChanged("账号或加密设备已变化，请重新解锁");
    await requireCryptoStore(await cryptoStoreName(account, expected.deviceId));
    if (generation !== expected.generation || localCryptoDevice(account) !== expected.deviceId || expected.isCurrent?.() === false) throw new CryptoSessionChanged("账号或加密设备已变化，请重新解锁");
  }
  lockCrypto();
  const epoch = generation;
  const elapsed = (name: typeof timingNames[number], start: number) => {
    if (clock && generation === epoch && attempt === timingAttempt) clock.measure(`llmn.crypto.unlock.${name}`, { start, end: clock.now() });
  };
  let step = clock?.now() ?? 0;
  const sdk = await import("@matrix-org/matrix-sdk-crypto-wasm");
  elapsed("sdkImport", step);
  step = clock?.now() ?? 0;
  await sdk.initAsync();
  elapsed("sdkInit", step);
  const metadata = saved && create !== "new-device" ? JSON.parse(saved) as { deviceId: string; cursor: string; deviceListsVersion?: string } : { deviceId: crypto.randomUUID(), cursor: "0" };
  const storeName = await cryptoStoreName(account, metadata.deviceId);
  if (epoch !== generation || expected?.isCurrent?.() === false) throw new CryptoSessionChanged("账号已切换，请重新解锁");
  step = clock?.now() ?? 0;
  const storeStart = step;
  let metadataStart: number | undefined, cipherStart: number | undefined, databaseStart: number | undefined;
  // Only fixed stage names and durations survive; SDK log arguments are never retained.
  const storeLogger = {
    debug(...args: unknown[]) {
      if (args.some(value => typeof value === "string" && value.includes("IndexedDbCryptoStore: Opening meta-store"))) {
        elapsed("storePrepare", storeStart); metadataStart = clock?.now() ?? 0;
      }
      if (args.some(value => typeof value === "string" && value.includes("IndexedDbCryptoStore: decrypting store cipher"))) {
        if (metadataStart !== undefined) elapsed("storeMetadataRead", metadataStart);
        elapsed("storeMetadata", storeStart); cipherStart = clock?.now() ?? 0;
      }
      if (args.some(value => typeof value === "string" && value.includes("IndexedDbCryptoStore: opening main store"))) {
        if (cipherStart !== undefined) elapsed("storeCipher", cipherStart);
        databaseStart = clock?.now() ?? 0;
      }
    },
    info() {}, warn() {}, error() {},
  };
  const handle = await sdk.StoreHandle.open(storeName, passphrase, storeLogger);
  let machine: Matrix.OlmMachine;
  try {
    elapsed("storeOpen", storeStart);
    if (databaseStart !== undefined) elapsed("storeDatabase", databaseStart);
    if (epoch !== generation || expected?.isCurrent?.() === false) throw new CryptoSessionChanged("账号已切换，请重新解锁");
    step = clock?.now() ?? 0;
    machine = await sdk.OlmMachine.initFromStore(new sdk.UserId(matrixUser(account)), new sdk.DeviceId(metadata.deviceId), handle);
    elapsed("machineLoad", step);
  } finally { handle.free(); }
  if (epoch !== generation || expected?.isCurrent?.() === false) { machine.close(); throw new CryptoSessionChanged("账号已切换，请重新解锁"); }
  const unlocked: Session = { sdk, machine, account, owner: ownerOf(account), deviceId: metadata.deviceId, cursor: metadata.cursor, deviceListsVersion: metadata.deviceListsVersion };
  const measuredSync = async () => { const start = clock?.now() ?? 0; await syncCrypto(); elapsed("backgroundSync", start); };
  active = unlocked;
  if (saved && (create === "new-device" || create === "resume-device")) {
    replacements.add(unlocked);
    localStorage.setItem(pendingDeviceKey(account), JSON.stringify(metadata));
  } else if (!saved) localStorage.setItem(metadataKey(account), JSON.stringify(metadata));
  let offlineStatus: Awaited<ReturnType<typeof deviceStatus>> | undefined;
  try {
    const [registration, local] = await Promise.allSettled([
      (async () => {
        const start = clock?.now() ?? 0;
        const grant=await request<{authorized:boolean;fingerprint:string|null}>("/crypto/devices", "POST", { deviceId: metadata.deviceId, name: "LLM Nest 客户端" });
        current(unlocked);
        admission(unlocked, grant.authorized===true && grant.fingerprint===machine.identityKeys.ed25519.toBase64());
        elapsed("deviceRegister", start);
      })(),
      (async () => {
        let start = clock?.now() ?? 0;
        await machine.updateTrackedUsers([new sdk.UserId(matrixUser(account))]);
        current(unlocked);
        elapsed("trackedUsers", start);
        start = clock?.now() ?? 0;
        const status = await deviceStatus(elapsed);
        elapsed("deviceStatus", start);
        return status;
      })(),
    ]);
    current(unlocked);
    // Permission failures take precedence even when the local read also failed.
    if (registration.status === "rejected" && !(registration.reason instanceof ConnectionError)) throw registration.reason;
    if (local.status === "rejected") { lockCrypto(); throw local.reason; }
    if (registration.status === "rejected" && saved && create !== "new-device") unlocked.authorized = localStorage.getItem(admissionKey(unlocked))===machine.identityKeys.ed25519.toBase64();
    const localStatus = {...local.value, trusted:!!trustedCryptoDevice(account)};
    if (registration.status === "rejected") { offlineStatus = localStatus; throw registration.reason; }
    if (expected?.isCurrent?.() === false) throw new CryptoSessionChanged("自动解锁已取消");
    // A persisted trusted identity is usable after the server permission check.
    // Inbox, key queries and backup delivery can continue without holding the lock screen.
    if (saved && create !== "new-device" && localStatus.trusted) {
      void measuredSync().catch(error => {
        if (error instanceof CryptoSessionChanged || active !== unlocked || unlocked.owner !== ownerOf(account)) return;
        unlocked.pendingWarning = error instanceof Error ? error.message : String(error);
        window.dispatchEvent(new CustomEvent("llmn-crypto-warning", { detail: unlocked.pendingWarning }));
      });
      changed();
      elapsed("unlockReady", started);
      return localStatus;
    }
    await measuredSync();
  } catch (error) {
    if (active !== unlocked || !(error instanceof ConnectionError) || !saved || create === "new-device") {
      if (active === unlocked) lockCrypto();
      throw error;
    }
    // An existing encrypted store can be unlocked without promoting a new device.
  }
  current(unlocked);
  const status = offlineStatus ?? await deviceStatus();
  if (expected?.isCurrent?.() === false) { if (active === unlocked) lockCrypto(); throw new CryptoSessionChanged("自动解锁已取消"); }
  current(unlocked); changed();
  elapsed("unlockReady", started);
  return status;
}

export async function deviceStatus(measure?: (name: "statusIdentity" | "statusCrossSigning" | "statusOwnDeviceTrust", start: number) => void) {
  const s = session();
  let start = typeof performance === "undefined" ? 0 : performance.now();
  const identity = await s.machine.getIdentity(new s.sdk.UserId(matrixUser(s.account)));
  current(s);
  measure?.("statusIdentity", start);
  start = typeof performance === "undefined" ? 0 : performance.now();
  const cross = await s.machine.crossSigningStatus();
  current(s);
  measure?.("statusCrossSigning", start);
  start = typeof performance === "undefined" ? 0 : performance.now();
  const ownsDevice = identity && "trustsOurOwnDevice" in identity ? await identity.trustsOurOwnDevice() : false;
  current(s);
  measure?.("statusOwnDeviceTrust", start);
  const wasTrusted=!!trustedCryptoDevice(s.account);
  s.localVerified = !!identity?.isVerified() && ownsDevice;
  if (replacements.has(s) && trustedCryptoDevice(s.account)) persistDevice(s);
  if(wasTrusted!==!!trustedCryptoDevice(s.account)) changed();
  return { deviceId: s.deviceId, fingerprint: s.machine.identityKeys.ed25519.toBase64(), curve: s.machine.identityKeys.curve25519.toBase64(), master: identity ? masterFingerprint(identity.masterKey) : null, trusted: !!trustedCryptoDevice(s.account), hasRecoverySecrets: cross.hasMaster && cross.hasSelfSigning && cross.hasUserSigning, authorizationAvailable: !!s.authorization };
}
export async function initializeIdentity() {
  const s = session();
  await syncCrypto();
  await sendOutgoing(s, new s.sdk.KeysQueryRequest(crypto.randomUUID(), JSON.stringify({ device_keys: { [matrixUser(s.account)]: [] } })));
  if (await s.machine.getIdentity(new s.sdk.UserId(matrixUser(s.account)))) {
    if ((await s.machine.crossSigningStatus()).hasMaster) { current(s); changed(); return deviceStatus(); }
    throw new Error("账号已经有加密身份，请使用恢复包或现有可信设备授权；不能重置身份");
  }
  const bootstrap = await s.machine.bootstrapCrossSigning(false);
  if (bootstrap.uploadKeysRequest) { const upload = bootstrap.uploadKeysRequest as Matrix.KeysUploadRequest; queueRequest(s, { id: upload.id, type: upload.type, kind: "keysUpload", body: JSON.parse(upload.body) }); }
  queueRequest(s, { id: crypto.randomUUID(), kind: "signingKeys", body: JSON.parse(bootstrap.uploadSigningKeysRequest.body) });
  queueRequest(s, { id: bootstrap.uploadSignaturesRequest.id ?? crypto.randomUUID(), type: bootstrap.uploadSignaturesRequest.id ? bootstrap.uploadSignaturesRequest.type : undefined, kind: "signatures", body: JSON.parse(bootstrap.uploadSignaturesRequest.body) });
  await flush(s);
  await syncCrypto();
  current(s); changed();
  return deviceStatus();
}

export async function devices(account: string) {
  const s = session(), user = matrixUser(account);
  await s.machine.updateTrackedUsers([new s.sdk.UserId(user)]);
  await syncCrypto();
  await sendOutgoing(s, new s.sdk.KeysQueryRequest(crypto.randomUUID(), JSON.stringify({ device_keys: { [user]: [] } })));
  const devices = await s.machine.getUserDevices(new s.sdk.UserId(user));
  const identity = await s.machine.getIdentity(new s.sdk.UserId(user));
  current(s); changed();
  return devices.devices().map(d => ({ deviceId: d.deviceId.toString(), name: d.displayName ?? "设备", fingerprint: d.ed25519Key?.toBase64() ?? "", curve: d.curve25519Key?.toBase64() ?? "", master: identity ? masterFingerprint(identity.masterKey) : "", trusted: d.isVerified() && d.isCrossSignedByOwner() && !!identity?.isVerified(), own: account === s.account }));
}
export async function trustDevice(account: string, deviceId: string, fingerprint: string, curve: string, master: string) {
  const s = session();
  await sendOutgoing(s, new s.sdk.KeysQueryRequest(crypto.randomUUID(), JSON.stringify({ device_keys: { [matrixUser(account)]: [] } })));
  const device = await s.machine.getDevice(new s.sdk.UserId(matrixUser(account)), new s.sdk.DeviceId(deviceId));
  const identity = await s.machine.getIdentity(new s.sdk.UserId(matrixUser(account)));
  if (!device || device.ed25519Key?.toBase64() !== fingerprint || device.curve25519Key?.toBase64() !== curve || !identity || masterFingerprint(identity.masterKey) !== master) throw new Error("设备或账号主指纹已变化，请重新核对");
  if (account===s.account && deviceId!==s.deviceId) await request("/crypto/authority/enroll","POST",await signedCommand(s,"enroll",{target:deviceId,fingerprint}));
  await sendOutgoing(s, await identity.verify());
  if (account === s.account && (await s.machine.crossSigningStatus()).hasSelfSigning) await sendOutgoing(s, await device.verify());
  else if (!device.isCrossSignedByOwner()) throw new Error("此设备尚未被其账号的可信身份签名，请先在该账号可信设备上授权");
  await device.setLocalTrust(s.sdk.LocalTrust.Verified);
  await sendOutgoing(s, new s.sdk.KeysQueryRequest(crypto.randomUUID(), JSON.stringify({ device_keys: { [matrixUser(account)]: [] } })));
  current(s);
  changed();
}

export async function prepareEncryption(room: string, users: string[], offline = false, requiredUsers = users) {
  const s = session();
  const status = await deviceStatus();
  if (!status.trusted) throw new Error("当前设备尚未获得可信授权，不能发送加密内容");
  await s.machine.updateTrackedUsers(users.map(u => new s.sdk.UserId(matrixUser(u))));
  if (!offline) await syncCrypto();
  const roster: string[] = [];
  for (const user of users) {
    const listed = await s.machine.getUserDevices(new s.sdk.UserId(matrixUser(user)));
    const trusted = listed.devices().filter(d => d.isVerified() && d.isCrossSignedByOwner() && !d.isDeleted());
    if (requiredUsers.includes(user) && !trusted.length) throw new Error(`账号 ${user} 没有已核对并由账号签名的可信设备，请先在可信设备中核对指纹`);
    roster.push(...trusted.map(d => `${user}/${d.deviceId.toString()}/${d.ed25519Key?.toBase64()}/${d.curve25519Key?.toBase64()}`));
  }
  const rosterKey = `llmn-crypto-room-devices:${s.owner}:${room}`, nextRoster = JSON.stringify(roster.sort());
  const previousRoster = localStorage.getItem(rosterKey);
  if (previousRoster && previousRoster !== nextRoster) await s.machine.invalidateGroupSession(new s.sdk.RoomId(room));
  localStorage.setItem(rosterKey, nextRoster);
  const claim = await s.machine.getMissingSessions(users.map(u => new s.sdk.UserId(matrixUser(u))));
  if (claim) { if (offline) throw new Error("此设备尚无该范围的加密会话，请先联网准备可信设备"); await sendOutgoing(s, claim); }
  const settings = new s.sdk.EncryptionSettings();
  settings.algorithm = s.sdk.EncryptionAlgorithm.MegolmV1AesSha2;
  settings.sharingStrategy = s.sdk.CollectStrategy.onlyTrustedDevices();
  const roomSettings = new s.sdk.RoomSettings();
  roomSettings.algorithm = s.sdk.EncryptionAlgorithm.MegolmV1AesSha2;
  roomSettings.onlyAllowTrustedDevices = true;
  await s.machine.setRoomSettings(new s.sdk.RoomId(room), roomSettings);
  for (const outgoing of await s.machine.shareRoomKey(new s.sdk.RoomId(room), users.map(u => new s.sdk.UserId(matrixUser(u))), settings)) queueToDevice(s, outgoing);
  if (!offline) await flush(s);
}
export async function seal(room: string, users: string[], eventType: string, content: unknown, offline = false, requiredUsers = users, rotate = false): Promise<Record<string, unknown>> {
  if (rotate) await session().machine.invalidateGroupSession(new (session().sdk.RoomId)(room));
  await prepareEncryption(room, users, offline, requiredUsers);
  const s = session();
  const encrypted = await s.machine.encryptRoomEvent(new s.sdk.RoomId(room), eventType, JSON.stringify(content));
  current(s);
  return JSON.parse(encrypted);
}
export function localKeyMessages(users: string[]): Record<string, unknown>[] {
  const s = session();
  const queued = JSON.parse(localStorage.getItem(`llmn-crypto-outgoing:${s.owner}`) ?? "[]") as { kind: string; body: { event_type: string; txn_id: string; messages: Record<string, unknown> } }[];
  return queued.filter(q => q.kind === "toDevice").map(q => ({ ...q.body, messages: Object.fromEntries(Object.entries(q.body.messages).filter(([id]) => users.some(user => matrixUser(user) === id))) })).filter(q => Object.keys(q.messages).length);
}
export async function receiveLocalKeys(sender: string, messages: Record<string, unknown>[]) {
  const s = session();
  const events = messages.flatMap(body => {
    const recipients = (body.messages as Record<string, Record<string, unknown>>)[matrixUser(s.account)];
    const content = recipients?.[s.deviceId];
    return content ? [{ type: body.event_type, sender: matrixUser(sender), content }] : [];
  });
  // Local delivery has no server key counts; missing counts must remain unknown.
  if (events.length) await s.machine.receiveSyncChangesMsc4186(JSON.stringify(events), new s.sdk.DeviceLists([], []), new Map(), undefined, new s.sdk.DecryptionSettings(s.sdk.TrustRequirement.CrossSigned));
  current(s);
}
export async function signEnvelope(base: Omit<SignedEnvelope, "deviceKeys" | "signatures">): Promise<SignedEnvelope> {
  const s = session();
  if (base.sender !== matrixUser(s.account) || base.deviceId !== s.deviceId) throw new Error("签名发送设备与当前身份不匹配");
  const envelope: SignedEnvelope = { ...base, deviceKeys: { ed25519: s.machine.identityKeys.ed25519.toBase64(), curve25519: s.machine.identityKeys.curve25519.toBase64() }, signatures: {} };
  envelope.signatures = JSON.parse((await s.machine.sign(canonicalJson(unsignedEnvelope(envelope)))).asJSON());
  const identity = await s.machine.getIdentity(new s.sdk.UserId(base.sender));
  if (!identity?.isVerified()) throw new Error("账号主身份尚未可信确认");
  verifyEnvelope(envelope, masterFingerprint(identity.masterKey));
  current(s); return envelope;
}
export async function unseal(envelope: SignedEnvelope) {
  const s = session();
  let identity = await s.machine.getIdentity(new s.sdk.UserId(envelope.sender));
  if (!identity) {
    await s.machine.updateTrackedUsers([new s.sdk.UserId(envelope.sender)]);
    await syncCrypto();
    identity = await s.machine.getIdentity(new s.sdk.UserId(envelope.sender));
  }
  if (!identity?.isVerified()) throw new Error("发送账号主指纹尚未通过独立可信核对");
  verifyEnvelope(envelope, masterFingerprint(identity.masterKey));
  const event = { type: "m.room.encrypted", event_id: envelope.eventId, sender: envelope.sender, origin_server_ts: 0, content: envelope.content };
  const decrypted = await s.machine.decryptRoomEvent(JSON.stringify(event), new s.sdk.RoomId(envelope.roomId), new s.sdk.DecryptionSettings(s.sdk.TrustRequirement.Untrusted));
  current(s);
  return JSON.parse(decrypted.event) as { type: string; content: Record<string, unknown> };
}

export async function exportRecoverySecrets() {
  const s = session();
  const bundle = await s.machine.exportSecretsBundle();
  const roomKeys = await s.machine.exportRoomKeys(() => true);
  current(s);
  return { secrets: bundle.to_json(), roomKeys: JSON.parse(roomKeys) };
}
export async function importRecoverySecrets(secrets: unknown, roomKeys: unknown) {
  const s = session();
  await s.machine.importSecretsBundle(s.sdk.SecretsBundle.from_json(secrets));
  await s.machine.importExportedRoomKeys(JSON.stringify(roomKeys), () => {});
  await storedBackup(s);
  await flush(s);
  const device = await s.machine.getDevice(new s.sdk.UserId(matrixUser(s.account)), new s.sdk.DeviceId(s.deviceId));
  if (device) await sendOutgoing(s, await device.verify());
  await syncCrypto();
  current(s); changed();
  return deviceStatus();
}
export async function authorizeDevice(deviceId: string) {
  const s = session();
  const device = await s.machine.getDevice(new s.sdk.UserId(matrixUser(s.account)), new s.sdk.DeviceId(deviceId));
  if (!device?.isVerified()) throw new Error("请先核对并信任目标设备指纹");
  const missing = await s.machine.getMissingSessions([new s.sdk.UserId(matrixUser(s.account))]);
  if (missing) await sendOutgoing(s, missing);
  const bundle = await exportRecoverySecrets();
  const content = JSON.parse(await device.encryptToDeviceEvent("com.llmn.recovery.v1", { ...bundle, senderDevice: s.deviceId }, s.sdk.CollectStrategy.onlyTrustedDevices()));
  const id = crypto.randomUUID();
  queueToDevice(s, new s.sdk.ToDeviceRequest(id, "m.room.encrypted", id, JSON.stringify({ messages: { [matrixUser(s.account)]: { [deviceId]: content } } })));
  await flush(s);
}
export async function acceptAuthorization() {
  const s = session(), authorization = s.authorization;
  if (!authorization) throw new Error("尚未收到可信设备的加密授权");
  await importRecoverySecrets(authorization.secrets, authorization.roomKeys);
  s.authorization = undefined;
  confirmRecovery();
  changed();
}

export interface RecoveryFile { format: "llmn-recovery-v1" | "llmn-recovery-v2"; server: string; accountId: string; version: string; mediaEncryptionInfo: string; enrollmentSeed?: string }
const bytes64 = (bytes: Uint8Array) => { let binary = ""; for (const b of bytes) binary += String.fromCharCode(b); return btoa(binary); };
const unpadded = (bytes: Uint8Array) => bytes64(bytes).replace(/=+$/, "");
const from64 = (value: string) => Uint8Array.from(atob(value), c => c.charCodeAt(0));
export function recoveryConfirmed(account: string) { return localStorage.getItem(`llmn-crypto-recovery:${ownerOf(account)}`) === "confirmed"; }
export function confirmRecovery() { const s = session(); localStorage.setItem(`llmn-crypto-recovery:${s.owner}`, "confirmed"); changed(); }
const backupAlgorithm = "m.megolm_backup.v1.curve25519-aes-sha2";
const atVersion = (path: string, version: string) => path + "?version=" + encodeURIComponent(version);

type BackupPointer = { version: string; publicKey: string };
const retiredKey = (s: Session, version: string) => `llmn-backup-retired:${s.owner}:${version}`;
function backupWarning(s: Session, message: string) {
  if (s.pendingWarning === message) return;
  s.pendingWarning = message;
  window.dispatchEvent(new CustomEvent("llmn-crypto-warning", { detail: message }));
}
async function selectBackup(s: Session, next: BackupPointer | null, retired: boolean) {
  if (retired && s.authorized && recoveryConfirmed(s.account)) {
    localStorage.setItem(`llmn-crypto-recovery:${s.owner}`, "retired");
    s.pendingChange = true;
    backupWarning(s, "旧备份已封存。请在可信设备与恢复中保存新的恢复文件，再继续备份。");
  }
  const version = next?.version ?? null;
  if (s.liveBackup === version) return;
  if (await s.machine.isBackupEnabled()) await s.machine.disableBackup();
  current(s);
  const local = await s.machine.getBackupKeys();
  const previous = local.backupVersion;
  if (previous && previous !== version && (version || retired)) localStorage.setItem(retiredKey(s, previous), "1");
  s.liveBackup = null;
  if (!next || !s.authorized) return;

  const pinned = local.decryptionKey?.megolmV1PublicKey.publicKeyBase64;
  if (previous === version && pinned === next.publicKey && !localStorage.getItem(retiredKey(s, next.version))) {
    const info = await request<BackupInfo>(atVersion("/crypto/backup", next.version));
    const matches = !info.sealed && info.version === next.version && info.algorithm === backupAlgorithm && info.auth_data.public_key === pinned;
    if (matches && (await s.machine.verifyBackup(info)).trusted()) {
      current(s);
      await s.machine.enableBackupV1(next.publicKey, next.version);
      s.liveBackup = version;
      return;
    }
  }
  backupWarning(s, "请从当前恢复文件或可信设备同步新的备份密钥");
}
async function storedBackup(s: Session) {
  const keys = await s.machine.getBackupKeys();
  if (!keys.decryptionKey || !keys.backupVersion) throw new Error("恢复材料缺少历史备份密钥");
  const info = await request<BackupInfo>(atVersion("/crypto/backup", keys.backupVersion));
  if (info.version !== keys.backupVersion || info.algorithm !== backupAlgorithm || info.auth_data.public_key !== keys.decryptionKey.megolmV1PublicKey.publicKeyBase64)
    throw new Error("云端历史备份与恢复密钥不匹配");
  return { decryptionKey:keys.decryptionKey, backupVersion:keys.backupVersion, sealed:info.sealed === true };
}
async function readBackup(s: Session, key: Matrix.BackupDecryptionKey, version: string) {
  const backup = await request<{ rooms: Record<string, { sessions: Record<string, { session_data: { ephemeral: string; mac: string; ciphertext: string } }> }> }>(atVersion("/crypto/backup/keys", version));
  const rooms = new Map<Matrix.RoomId, Map<string, unknown>>();
  for (const [room, content] of Object.entries(backup.rooms)) {
    const sessions = new Map<string, unknown>();
    for (const [id, { session_data }] of Object.entries(content.sessions)) sessions.set(id, JSON.parse(key.decryptV1(session_data.ephemeral, session_data.mac, session_data.ciphertext)));
    rooms.set(new s.sdk.RoomId(room), sessions);
  }
  await s.machine.importBackedUpRoomKeys(rooms, () => { }, version);
}
async function latest<T>(path: string): Promise<T | null> {
  try { return await request<T>(path); }
  catch (error) {
    if (error instanceof HttpError && error.status === 404) return null;
    throw error;
  }
}
async function prepareBackup(s: Session, old: BackupInfo | null, existing: Matrix.BackupKeys) {
  const previous = existing.decryptionKey;
  const matches = old && existing.backupVersion === old.version &&
    previous?.megolmV1PublicKey.publicKeyBase64 === old.auth_data.public_key;
  if (old && !old.sealed) {
    if (!matches || !previous) throw new Error("请导入当前恢复文件或从持有该备份的可信设备取得授权；现有备份不会被覆盖");
    return { key: previous, backupVersion: old.version };
  }
  if (old && matches && previous) await readBackup(s, previous, old.version);
  current(s);

  const key = s.sdk.BackupDecryptionKey.createRandomKey();
  const backupVersion = crypto.randomUUID();
  const publicKey = { public_key: key.megolmV1PublicKey.publicKeyBase64 };
  const signatures = await s.machine.sign(canonicalJson(publicKey));
  const newBackup = {
    version: backupVersion, expectedVersion: old?.version ?? null, algorithm: backupAlgorithm,
    auth_data: { ...publicKey, signatures: JSON.parse(signatures.asJSON()) }
  };
  return { key, backupVersion, newBackup };
}
export async function createRecoveryFile(): Promise<RecoveryFile> {
  const s = session(), status = await deviceStatus();
  if (!status.trusted || !status.hasRecoverySecrets) throw new Error("请先用可信设备或恢复文件授权本机");
  const [old, recovery, existing] = await Promise.all([latest<BackupInfo>("/crypto/backup"), latest<{ version: string }>("/crypto/recovery"), s.machine.getBackupKeys()]);
  current(s);
  const { key, backupVersion, newBackup } = await prepareBackup(s, old, existing);
  const bundle = (await s.machine.exportSecretsBundle()).to_json() as Record<string, unknown>;
  bundle.backup = { algorithm: backupAlgorithm, key: key.toBase64(), backup_version: backupVersion };
  const bytes = new TextEncoder().encode(JSON.stringify(bundle));
  const encrypted = s.sdk.Attachment.encrypt(bytes);
  bytes.fill(0);
  const version = crypto.randomUUID(), seed = ed25519.utils.randomSecretKey();
  const enrollmentKey = unpadded(ed25519.getPublicKey(seed));
  current(s);
  const command = {
    expectedVersion: recovery?.version ?? null, version, backupVersion,
    ciphertext: bytes64(encrypted.encryptedData), enrollmentKey,
    ...(newBackup ? { newBackup } : {})
  };
  preparedRecovery = { session: s, version, backupVersion, key: key.toBase64(), command };
  return { format: "llmn-recovery-v2", server, accountId: s.account, version, mediaEncryptionInfo: encrypted.mediaEncryptionInfo!, enrollmentSeed: unpadded(seed) };
}
export async function commitRecoveryFile(file: RecoveryFile) {
  const s = session(), pending = preparedRecovery;
  if (!pending || pending.session !== s || pending.version !== file.version) throw new Error("请重新生成并保存当前恢复文件");
  await request("/crypto/recovery", "PUT", await signedCommand(s, "recovery", pending.command));
  current(s);
  const key = s.sdk.BackupDecryptionKey.fromBase64(pending.key);
  const before = await s.machine.getBackupKeys();
  if (before.backupVersion && before.backupVersion !== pending.backupVersion) await s.machine.disableBackup();
  await s.machine.saveBackupDecryptionKey(key, pending.backupVersion);
  await s.machine.enableBackupV1(key.megolmV1PublicKey.publicKeyBase64, pending.backupVersion);
  current(s);
  s.liveBackup = pending.backupVersion;
  preparedRecovery = undefined;
  confirmRecovery();
  await syncCrypto();
}
function validateFile(s: Session, file: RecoveryFile) {
  if (!["llmn-recovery-v1", "llmn-recovery-v2"].includes(file.format) || file.server !== server || file.accountId !== s.account) throw new Error("恢复文件不属于当前服务器与真人账号");
}
async function openFile(s: Session, file: RecoveryFile) {
  validateFile(s, file);
  const recovery = await request<{ version: string; ciphertext: string }>(atVersion("/crypto/recovery", file.version));
  if (recovery.version !== file.version) throw new Error("恢复文件版本不匹配");
  const bytes = s.sdk.Attachment.decrypt(new s.sdk.EncryptedAttachment(from64(recovery.ciphertext), file.mediaEncryptionInfo));
  try { return JSON.parse(new TextDecoder().decode(bytes)); } finally { bytes.fill(0); }
}
async function confirmFileAdmission(s: Session, file: RecoveryFile) {
  const granted = await request<{ authorized: boolean; fingerprint: string | null }>(`/crypto/authority/devices/${encodeURIComponent(s.deviceId)}`);
  current(s);
  admission(s, granted.authorized && granted.fingerprint === s.machine.identityKeys.ed25519.toBase64());
  if (file.format === "llmn-recovery-v2" && !s.authorized) {
    if (!file.enrollmentSeed) throw new Error("文件缺少恢复授权密钥");
    const fingerprint = s.machine.identityKeys.ed25519.toBase64();
    const grant = [file.version, s.deviceId, fingerprint];
    const recoveryProof = unpadded(ed25519.sign(new TextEncoder().encode(statement(s.account, "recover", grant)), from64(file.enrollmentSeed)));
    await request("/crypto/authority/enroll", "POST", { deviceId: s.deviceId, target: s.deviceId, fingerprint, recoveryVersion: file.version, recoveryProof, proof: await proof(s, "accept", grant) });
    current(s);
    admission(s, true);
  } else {
    if (!s.authorized) throw new Error("旧历史密钥已导入本机。旧文件不能授权新设备；请由可信设备或经独立身份核验的运维人员核对本机设备指纹后，再次导入旧文件");
  }
}

export async function restoreRecoveryFile(file: RecoveryFile) {
  const s = session(), bundle = await openFile(s, file);
  const machine = await s.sdk.OlmMachine.initialize(new s.sdk.UserId(matrixUser(s.account)), new s.sdk.DeviceId(crypto.randomUUID()));
  try {
    const archive = { ...s, machine };
    await machine.importSecretsBundle(s.sdk.SecretsBundle.from_json(bundle));
    const keys = await storedBackup(archive);
    await readBackup(archive, keys.decryptionKey!, keys.backupVersion!);
    current(s);
    const history = await machine.exportRoomKeys(() => true);
    await s.machine.importExportedRoomKeys(history, () => { });
  } finally { machine.close(); }
  current(s);
  await confirmFileAdmission(s, file);
  const localKeys = await s.machine.getBackupKeys();
  if (localKeys.backupVersion && localKeys.decryptionKey && localKeys.backupVersion !== bundle.backup?.backup_version) {
    const live = await latest<BackupInfo>("/crypto/backup");
    if (live?.version === localKeys.backupVersion) bundle.backup = { algorithm: backupAlgorithm, key: localKeys.decryptionKey.toBase64(), backup_version: localKeys.backupVersion };
  }
  await s.machine.importSecretsBundle(s.sdk.SecretsBundle.from_json(bundle));
  const device = await s.machine.getDevice(new s.sdk.UserId(matrixUser(s.account)), new s.sdk.DeviceId(s.deviceId));
  if (device) await sendOutgoing(s, await device.verify());
  if (file.format === "llmn-recovery-v2") confirmRecovery();
  await syncCrypto();
  return deviceStatus();
}

export function encryptAttachment(data: Uint8Array) {
  const encrypted = session().sdk.Attachment.encrypt(data);
  return { data: bytes64(encrypted.encryptedData), mediaEncryptionInfo: encrypted.mediaEncryptionInfo! };
}
export function decryptAttachment(data: Uint8Array, mediaEncryptionInfo: string) {
  const s = session();
  return s.sdk.Attachment.decrypt(new s.sdk.EncryptedAttachment(data, mediaEncryptionInfo));
}
export async function shareConversationHistory(conversation: import("../types").Conversation) {
  const s = session(), room = matrixRoom(conversation.id);
  const users = conversation.members.map(m => m.account.id);
  await prepareEncryption(room, users, false, conversation.encryption?.historyComplete ? [s.account] : users);
  const keys = JSON.parse(await s.machine.exportRoomKeys(key => key.roomId.toString() === room));
  for (const member of conversation.members) {
    const devices = await s.machine.getUserDevices(new s.sdk.UserId(matrixUser(member.account.id)));
    for (const device of devices.devices().filter(d => d.isVerified() && d.isCrossSignedByOwner() && !d.isDeleted())) {
      if (member.account.id === s.account && device.deviceId.toString() === s.deviceId) continue;
      const encrypted = JSON.parse(await device.encryptToDeviceEvent("com.llmn.group-history.v1", { conversationId: conversation.id, revision: conversation.revision, keys }, s.sdk.CollectStrategy.onlyTrustedDevices()));
      const id = crypto.randomUUID();
      queueRequest(s, { id, kind: "toDevice", body: { event_type: "m.room.encrypted", txn_id: id, llmn_purpose: "conversationHistory", llmn_conversation_id: conversation.id, llmn_expected_revision: conversation.revision, messages: { [matrixUser(member.account.id)]: { [device.deviceId.toString()]: encrypted } } } });
    }
  }
  await flush(s);
}

// The reader has no current room keys or Olm sessions: only the selected recovery bundle and downloaded backup.
export async function recoveryReader(file: RecoveryFile, archivedOnly = false) {
  const s = session();
  validateFile(s, file);
  if (!archivedOnly) {
    await syncCrypto();
    if (s.liveBackup !== null && s.authorized !== false) {
      for (let backup = await s.machine.backupRoomKeys(); backup; backup = await s.machine.backupRoomKeys())
        await sendOutgoing(s, backup);
    }
  }
  const bundle = await openFile(s, file);
  const machine = await s.sdk.OlmMachine.initialize(new s.sdk.UserId(matrixUser(s.account)), new s.sdk.DeviceId(crypto.randomUUID()));
  try {
    const archive = { ...s, machine };
    await machine.importSecretsBundle(s.sdk.SecretsBundle.from_json(bundle));
    const keys = await storedBackup(archive);
    await readBackup(archive, keys.decryptionKey!, keys.backupVersion!);
    current(s);
    const lookedUp = new Set<string>();
    return {
      sealed: keys.sealed,
      lacksSession: (error: unknown) => error instanceof s.sdk.MegolmDecryptionError &&
        (error.code === s.sdk.DecryptionErrorCode.MissingRoomKey || error.code === s.sdk.DecryptionErrorCode.UnknownMessageIndex),
      check: () => current(s),
      close: () => machine.close(),
      unseal: async (envelope: SignedEnvelope) => {
        current(s);
        if (archivedOnly && !lookedUp.has(envelope.sender)) {
          // Populate this isolated reader's public directory, never its room keys, from the server.
          // The file's user-signing secret anchors existing contact verifications.
          const ids = [...new Set([matrixUser(s.account), envelope.sender])], id = crypto.randomUUID();
          const body = { device_keys: Object.fromEntries(ids.map(user => [user, []])) };
          const response = await request("/crypto/requests", "POST", { deviceId: s.deviceId, requestId: id, kind: "keysQuery", body });
          current(s);
          await machine.markRequestAsSent(id, s.sdk.RequestType.KeysQuery, JSON.stringify(response));
          lookedUp.add(envelope.sender);
        }
        const identity = await (archivedOnly ? machine : s.machine).getIdentity(new s.sdk.UserId(envelope.sender));
        if (!identity?.isVerified()) throw new Error("历史签名账号主指纹尚未核验");
        verifyEnvelope(envelope, masterFingerprint(identity.masterKey));
        const event = { type: "m.room.encrypted", event_id: envelope.eventId, sender: envelope.sender, origin_server_ts: 0, content: envelope.content };
        const decoded = await machine.decryptRoomEvent(JSON.stringify(event), new s.sdk.RoomId(envelope.roomId), new s.sdk.DecryptionSettings(s.sdk.TrustRequirement.Untrusted));
        current(s);
        return JSON.parse(decoded.event) as { type: string; content: Record<string, unknown> };
      },
    };
  } catch (error) { machine.close(); throw error; }
}
