import { request, server, HttpError, ConnectionError } from "../api";
import type * as Matrix from "@matrix-org/matrix-sdk-crypto-wasm";
import { canonicalJson } from "./canonical";
import { verifyEnvelope, unsignedEnvelope, type SignedEnvelope } from "./envelope";

type Sdk = typeof Matrix;
type Session = { sdk: Sdk; machine: Matrix.OlmMachine; account: string; owner: string; deviceId: string; cursor: string; flushing?: Promise<void>; authorization?: { secrets: unknown; roomKeys: unknown; senderDevice: string } };
let active: Session | undefined;
let generation = 0;
let syncing: Promise<void> | undefined;
let preparedRecovery: { owner: string; version: string; expectedVersion: string | null; ciphertext: string } | undefined;
const ownerOf = (account: string) => JSON.stringify([server, account]);
const metadataKey = (account: string) => `llmn-crypto-device:${ownerOf(account)}`;
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
function current(s: Session) { if (active !== s) throw new Error("加密设备已锁定或账号已切换"); }
export function cryptoDevice(account: string) { return active?.owner === ownerOf(account) ? active.deviceId : undefined; }
export function hasCryptoDevice(account: string) { return localStorage.getItem(metadataKey(account)) !== null; }

async function sendOutgoing(s: Session, outgoing: Matrix.OutgoingRequest) {
  const body = JSON.parse(outgoing.body);
  if (outgoing.type === s.sdk.RequestType.ToDevice) {
    const toDevice = outgoing as Matrix.ToDeviceRequest;
    body.event_type = toDevice.event_type;
    body.txn_id = toDevice.txn_id;
  }
  const kinds = ["keysUpload", "keysQuery", "keysClaim", "toDevice", "signatures", "roomMessage", "keysBackup"];
  if (outgoing.type === s.sdk.RequestType.KeysBackup) body.version = (outgoing as Matrix.KeysBackupRequest).version;
  const response = await request("/crypto/requests", "POST", { deviceId: s.deviceId, requestId: outgoing.id ?? crypto.randomUUID(), kind: kinds[outgoing.type], body });
  current(s);
  if (outgoing.id) await s.machine.markRequestAsSent(outgoing.id, outgoing.type, JSON.stringify(response));
}
async function flushInner(s: Session) {
  const queueKey = `llmn-crypto-outgoing:${s.owner}`;
  let queued = JSON.parse(localStorage.getItem(queueKey) ?? "[]") as { id: string; type?: Matrix.RequestType; body: Record<string, unknown>; kind: string }[];
  while (queued.length) {
    const outgoing = queued[0];
    let response: unknown;
    try { response = await request("/crypto/requests", "POST", { deviceId: s.deviceId, requestId: outgoing.id, kind: outgoing.kind, body: outgoing.body }); }
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
    queued = (JSON.parse(localStorage.getItem(queueKey) ?? "[]") as typeof queued).filter(item => item.id !== outgoing.id);
    localStorage.setItem(queueKey, JSON.stringify(queued));
  }
  for (const outgoing of await s.machine.outgoingRequests()) await sendOutgoing(s, outgoing);
}
function flush(s: Session) {
  if (s.flushing) return s.flushing;
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
    const sync = await request<{ next_batch: string; to_device: { events: unknown[] }; device_lists: { changed: string[]; left: string[] }; device_one_time_keys_count: Record<string, number>; device_unused_fallback_key_types: string[] }>(`/crypto/sync?deviceId=${encodeURIComponent(s.deviceId)}&after=${encodeURIComponent(s.cursor)}`).catch(error => {
      if (error instanceof HttpError && [401, 403].includes(error.status) && active === s) lockCrypto();
      throw error;
    });
    current(s);
    await flush(s);
    const events = await s.machine.receiveSyncChanges(JSON.stringify(sync.to_device.events), new s.sdk.DeviceLists(sync.device_lists.changed.map(id => new s.sdk.UserId(id)), sync.device_lists.left.map(id => new s.sdk.UserId(id))), new Map(Object.entries(sync.device_one_time_keys_count)), new Set(sync.device_unused_fallback_key_types), new s.sdk.DecryptionSettings(s.sdk.TrustRequirement.CrossSigned));
    for (const processed of events) {
      if (!("encryptionInfo" in processed) || !("rawEvent" in processed) || !processed.encryptionInfo.isSenderVerified()) continue;
      const event = JSON.parse(processed.rawEvent);
      if (processed.encryptionInfo.sender.toString() !== event.sender) continue;
      if (event.type === "com.llmn.recovery.v1" && event.sender === matrixUser(s.account)) {
        if (processed.encryptionInfo.senderDevice?.toString() === event.content.senderDevice) s.authorization = event.content;
      } else if (event.type === "com.llmn.group-history.v1") {
        const group = await request<import("../types").Snapshot>("/chat/init");
        const conversation = group.conversations.find(c => c.id === event.content.conversationId);
        if (!conversation || conversation.revision !== event.content.revision || !conversation.members.some(m => matrixUser(m.account.id) === event.sender)) { window.dispatchEvent(new CustomEvent("llmn-crypto-warning", { detail: "群历史授权已变化，过期密钥分享已拒绝；请按当前成员重新分享" })); continue; }
        if (!Array.isArray(event.content.keys) || event.content.keys.some((key: { room_id: string }) => key.room_id !== matrixRoom(conversation.id))) { window.dispatchEvent(new CustomEvent("llmn-crypto-warning", { detail: "历史密钥包含其他范围，已拒绝导入" })); continue; }
        await s.machine.importExportedRoomKeys(JSON.stringify(event.content.keys), () => {});
      } else if (event.type === "com.llmn.ai-history.v1" && event.sender === matrixUser(event.content.aiId)) {
        await s.machine.importExportedRoomKeys(JSON.stringify(event.content.keys), () => {});
      }
    }
    current(s);
    s.cursor = sync.next_batch;
    localStorage.setItem(metadataKey(s.account), JSON.stringify({ deviceId: s.deviceId, cursor: s.cursor }));
    await flush(s);
    const backup = await s.machine.backupRoomKeys();
    if (backup) await sendOutgoing(s, backup);
    changed();
  })().finally(() => { if (syncing === work) syncing = undefined; });
  syncing = work;
  return work;
}

export async function unlockCrypto(account: string, passphrase: string, create: boolean | "new-device" = false) {
  if (!passphrase.trim()) throw new Error("请输入本机加密库口令；它与登录密码和恢复材料分开保管");
  const saved = localStorage.getItem(metadataKey(account));
  if (!saved && !create) throw new Error("此账号在当前设备没有加密资料，请明确创建新设备，再由可信设备或恢复包授权");
  if (saved && create === true) throw new Error("本机已有设备身份，请解锁，不能覆盖或重置");
  lockCrypto();
  const epoch = generation;
  const sdk = await import("@matrix-org/matrix-sdk-crypto-wasm");
  await sdk.initAsync();
  const metadata = saved && create !== "new-device" ? JSON.parse(saved) as { deviceId: string; cursor: string } : { deviceId: crypto.randomUUID(), cursor: "0" };
  const digest = await crypto.subtle.digest("SHA-256", new TextEncoder().encode(JSON.stringify([ownerOf(account), metadata.deviceId])));
  const storeName = `llmn-crypto-${Array.from(new Uint8Array(digest), b => b.toString(16).padStart(2, "0")).join("")}`;
  const machine = await sdk.OlmMachine.initialize(new sdk.UserId(matrixUser(account)), new sdk.DeviceId(metadata.deviceId), storeName, passphrase);
  if (epoch !== generation) { machine.close(); throw new Error("账号已切换，请重新解锁"); }
  const unlocked: Session = { sdk, machine, account, owner: ownerOf(account), deviceId: metadata.deviceId, cursor: metadata.cursor };
  active = unlocked;
  localStorage.setItem(metadataKey(account), JSON.stringify(metadata));
  try {
    await request("/crypto/devices", "POST", { deviceId: metadata.deviceId, name: "LLM Nest 客户端" });
    await machine.updateTrackedUsers([new sdk.UserId(matrixUser(account))]);
    await syncCrypto();
  } catch (error) {
    if (!(error instanceof ConnectionError) || !saved || create === "new-device") {
      if (active === unlocked) lockCrypto();
      throw error;
    }
    // An existing encrypted store can be unlocked without promoting a new device.
  }
  current(unlocked); changed();
  return deviceStatus();
}

export async function deviceStatus() {
  const s = session();
  const identity = await s.machine.getIdentity(new s.sdk.UserId(matrixUser(s.account)));
  const cross = await s.machine.crossSigningStatus();
  current(s);
  const ownsDevice = identity && "trustsOurOwnDevice" in identity ? await identity.trustsOurOwnDevice() : false;
  return { deviceId: s.deviceId, fingerprint: s.machine.identityKeys.ed25519.toBase64(), curve: s.machine.identityKeys.curve25519.toBase64(), master: identity ? masterFingerprint(identity.masterKey) : null, trusted: !!identity?.isVerified() && ownsDevice, hasRecoverySecrets: cross.hasMaster && cross.hasSelfSigning && cross.hasUserSigning, authorizationAvailable: !!s.authorization };
}
export async function initializeIdentity() {
  const s = session();
  await syncCrypto();
  await sendOutgoing(s, new s.sdk.KeysQueryRequest(crypto.randomUUID(), JSON.stringify({ device_keys: { [matrixUser(s.account)]: [] } })));
  if (await s.machine.getIdentity(new s.sdk.UserId(matrixUser(s.account)))) {
    if ((await s.machine.crossSigningStatus()).hasMaster) return deviceStatus();
    throw new Error("账号已经有加密身份，请使用恢复包或现有可信设备授权；不能重置身份");
  }
  const bootstrap = await s.machine.bootstrapCrossSigning(false);
  if (bootstrap.uploadKeysRequest) { const upload = bootstrap.uploadKeysRequest as Matrix.KeysUploadRequest; queueRequest(s, { id: upload.id, type: upload.type, kind: "keysUpload", body: JSON.parse(upload.body) }); }
  queueRequest(s, { id: crypto.randomUUID(), kind: "signingKeys", body: JSON.parse(bootstrap.uploadSigningKeysRequest.body) });
  queueRequest(s, { id: bootstrap.uploadSignaturesRequest.id ?? crypto.randomUUID(), type: bootstrap.uploadSignaturesRequest.id ? bootstrap.uploadSignaturesRequest.type : undefined, kind: "signatures", body: JSON.parse(bootstrap.uploadSignaturesRequest.body) });
  await flush(s);
  await syncCrypto();
  return deviceStatus();
}

export async function devices(account: string) {
  const s = session(), user = matrixUser(account);
  await s.machine.updateTrackedUsers([new s.sdk.UserId(user)]);
  await syncCrypto();
  await sendOutgoing(s, new s.sdk.KeysQueryRequest(crypto.randomUUID(), JSON.stringify({ device_keys: { [user]: [] } })));
  const devices = await s.machine.getUserDevices(new s.sdk.UserId(user));
  const identity = await s.machine.getIdentity(new s.sdk.UserId(user));
  return devices.devices().map(d => ({ deviceId: d.deviceId.toString(), name: d.displayName ?? "设备", fingerprint: d.ed25519Key?.toBase64() ?? "", curve: d.curve25519Key?.toBase64() ?? "", master: identity ? masterFingerprint(identity.masterKey) : "", trusted: d.isVerified() && d.isCrossSignedByOwner() && !!identity?.isVerified(), own: account === s.account }));
}
export async function trustDevice(account: string, deviceId: string, fingerprint: string, curve: string, master: string) {
  const s = session();
  await sendOutgoing(s, new s.sdk.KeysQueryRequest(crypto.randomUUID(), JSON.stringify({ device_keys: { [matrixUser(account)]: [] } })));
  const device = await s.machine.getDevice(new s.sdk.UserId(matrixUser(account)), new s.sdk.DeviceId(deviceId));
  const identity = await s.machine.getIdentity(new s.sdk.UserId(matrixUser(account)));
  if (!device || device.ed25519Key?.toBase64() !== fingerprint || device.curve25519Key?.toBase64() !== curve || !identity || masterFingerprint(identity.masterKey) !== master) throw new Error("设备或账号主指纹已变化，请重新核对");
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
  if (events.length) await s.machine.receiveSyncChanges(JSON.stringify(events), new s.sdk.DeviceLists([], []), new Map(), new Set(), new s.sdk.DecryptionSettings(s.sdk.TrustRequirement.CrossSigned));
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
  await enableStoredBackup(s);
  await flush(s);
  const device = await s.machine.getDevice(new s.sdk.UserId(matrixUser(s.account)), new s.sdk.DeviceId(s.deviceId));
  if (device) await sendOutgoing(s, await device.verify());
  await syncCrypto();
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

export interface RecoveryFile { format: "llmn-recovery-v1"; server: string; accountId: string; version: string; mediaEncryptionInfo: string }
const bytes64 = (bytes: Uint8Array) => { let binary = ""; for (const b of bytes) binary += String.fromCharCode(b); return btoa(binary); };
const from64 = (value: string) => Uint8Array.from(atob(value), c => c.charCodeAt(0));
export function recoveryConfirmed(account: string) { return localStorage.getItem(`llmn-crypto-recovery:${ownerOf(account)}`) === "confirmed"; }
export function confirmRecovery() { const s = session(); localStorage.setItem(`llmn-crypto-recovery:${s.owner}`, "confirmed"); changed(); }

async function enableStoredBackup(s: Session) {
  const keys = await s.machine.getBackupKeys();
  if (!keys.decryptionKey || !keys.backupVersion) throw new Error("恢复材料缺少历史备份密钥");
  const info = await request<{ version: string; algorithm: string; auth_data: { public_key: string } }>("/crypto/backup");
  if (info.version !== keys.backupVersion || info.auth_data.public_key !== keys.decryptionKey.megolmV1PublicKey.publicKeyBase64 || info.algorithm !== "m.megolm_backup.v1.curve25519-aes-sha2") throw new Error("云端历史备份身份与本机恢复密钥不一致，请核对，不能使用替换的备份");
  await s.machine.enableBackupV1(info.auth_data.public_key, info.version);
  return keys;
}
export async function createRecoveryFile(): Promise<RecoveryFile> {
  const s = session(), status = await deviceStatus();
  if (!status.hasRecoverySecrets) throw new Error("此设备没有完整恢复秘密，请先由可信设备或恢复文件授权");
  const existing = await s.machine.getBackupKeys();
  const backupCommandKey = `llmn-crypto-backup-publish:${s.owner}`;
  if (!existing.decryptionKey) {
    try { await request("/crypto/backup"); throw new Error("账号已有历史备份，请使用现有恢复材料，不能替换备份密钥"); } catch (error) { if (!(error instanceof HttpError && error.status === 404)) throw error; }
    const key = s.sdk.BackupDecryptionKey.createRandomKey(), version = crypto.randomUUID();
    const auth_data = { public_key: key.megolmV1PublicKey.publicKeyBase64, signatures: {} as unknown };
    auth_data.signatures = JSON.parse((await s.machine.sign(JSON.stringify({ public_key: auth_data.public_key }))).asJSON());
    await s.machine.saveBackupDecryptionKey(key, version);
    localStorage.setItem(backupCommandKey, JSON.stringify({ deviceId: s.deviceId, expectedVersion: null, version, algorithm: "m.megolm_backup.v1.curve25519-aes-sha2", auth_data }));
  }
  const backupCommand = localStorage.getItem(backupCommandKey);
  if (backupCommand) { await request("/crypto/backup", "PUT", JSON.parse(backupCommand)); current(s); localStorage.removeItem(backupCommandKey); }
  await enableStoredBackup(s);
  const bundle = await s.machine.exportSecretsBundle();
  const encrypted = s.sdk.Attachment.encrypt(new TextEncoder().encode(JSON.stringify(bundle.to_json())));
  const version = crypto.randomUUID();
  let expectedVersion: string | null;
  try { expectedVersion = (await request<{ version: string }>("/crypto/recovery")).version; } catch (error) { if (error instanceof HttpError && error.status === 404) expectedVersion = null; else throw error; }
  preparedRecovery = { owner: s.owner, version, expectedVersion, ciphertext: bytes64(encrypted.encryptedData) };
  current(s);
  await syncCrypto();
  return { format: "llmn-recovery-v1", server, accountId: s.account, version, mediaEncryptionInfo: encrypted.mediaEncryptionInfo! };
}
export async function commitRecoveryFile(file: RecoveryFile) {
  const s = session();
  if (!preparedRecovery || preparedRecovery.owner !== s.owner || preparedRecovery.version !== file.version) throw new Error("这份恢复文件不再是当前待确认版本，请重新生成并保存");
  await request("/crypto/recovery", "PUT", { deviceId: s.deviceId, expectedVersion: preparedRecovery.expectedVersion, version: preparedRecovery.version, ciphertext: preparedRecovery.ciphertext });
  current(s); preparedRecovery = undefined; confirmRecovery();
}
export async function restoreRecoveryFile(file: RecoveryFile) {
  const s = session();
  if ((file.format && file.format !== "llmn-recovery-v1") || file.server !== server || file.accountId !== s.account) throw new Error("恢复文件不属于当前服务器与真人账号");
  const recovery = await request<{ version: string; ciphertext: string }>("/crypto/recovery");
  if (file.version !== recovery.version) throw new Error("恢复文件版本与云端不一致，请使用当前版本文件");
  const plaintext = s.sdk.Attachment.decrypt(new s.sdk.EncryptedAttachment(from64(recovery.ciphertext), file.mediaEncryptionInfo));
  await s.machine.importSecretsBundle(s.sdk.SecretsBundle.from_json(JSON.parse(new TextDecoder().decode(plaintext))));
  plaintext.fill(0);
  const keys = await enableStoredBackup(s);
  const backup = await request<{ rooms: Record<string, { sessions: Record<string, { session_data: { ephemeral: string; mac: string; ciphertext: string } }> }> }>("/crypto/backup/keys");
  const rooms = new Map<Matrix.RoomId, Map<string, unknown>>();
  for (const [room, content] of Object.entries(backup.rooms)) {
    const sessions = new Map<string, unknown>();
    for (const [id, value] of Object.entries(content.sessions)) sessions.set(id, JSON.parse(keys.decryptionKey!.decryptV1(value.session_data.ephemeral, value.session_data.mac, value.session_data.ciphertext)));
    rooms.set(new s.sdk.RoomId(room), sessions);
  }
  await s.machine.importBackedUpRoomKeys(rooms, () => {}, keys.backupVersion!);
  const device = await s.machine.getDevice(new s.sdk.UserId(matrixUser(s.account)), new s.sdk.DeviceId(s.deviceId));
  if (device) await sendOutgoing(s, await device.verify());
  confirmRecovery();
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
export async function recoveryReader(file: RecoveryFile) {
  const s = session();
  if (file.format !== "llmn-recovery-v1" || file.server !== server || file.accountId !== s.account) throw new Error("恢复文件不属于当前服务器与账号");
  await syncCrypto();
  for (let backup = await s.machine.backupRoomKeys(); backup; backup = await s.machine.backupRoomKeys()) await sendOutgoing(s, backup);
  const recovery = await request<{ version: string; ciphertext: string }>("/crypto/recovery");
  if (recovery.version !== file.version) throw new Error("恢复文件版本已变化，请选择当前文件");
  const machine = await s.sdk.OlmMachine.initialize(new s.sdk.UserId(matrixUser(s.account)), new s.sdk.DeviceId(crypto.randomUUID()));
  try {
    const bytes = s.sdk.Attachment.decrypt(new s.sdk.EncryptedAttachment(from64(recovery.ciphertext), file.mediaEncryptionInfo));
    try { await machine.importSecretsBundle(s.sdk.SecretsBundle.from_json(JSON.parse(new TextDecoder().decode(bytes)))); } finally { bytes.fill(0); }
    const keys = await enableStoredBackup({ ...s, machine });
    const backup = await request<{ rooms: Record<string, { sessions: Record<string, { session_data: { ephemeral: string; mac: string; ciphertext: string } }> }> }>("/crypto/backup/keys");
    const rooms = new Map<Matrix.RoomId, Map<string, unknown>>();
    for (const [room, value] of Object.entries(backup.rooms)) {
      const sessions = new Map<string, unknown>();
      for (const [id, entry] of Object.entries(value.sessions)) sessions.set(id, JSON.parse(keys.decryptionKey!.decryptV1(entry.session_data.ephemeral, entry.session_data.mac, entry.session_data.ciphertext)));
      rooms.set(new s.sdk.RoomId(room), sessions);
    }
    await machine.importBackedUpRoomKeys(rooms, () => {}, keys.backupVersion!);
    current(s);
    return {
      close: () => machine.close(),
      unseal: async (envelope: SignedEnvelope) => {
        current(s);
        const identity = await s.machine.getIdentity(new s.sdk.UserId(envelope.sender));
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
