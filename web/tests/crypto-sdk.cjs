const assert = require('node:assert/strict'), fs = require('node:fs'), vm = require('node:vm'), ts = require('typescript');
const canonical = {};
vm.runInNewContext(ts.transpileModule(fs.readFileSync(require.resolve('../src/cloud/crypto/canonical.ts'), 'utf8'), { compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 } }).outputText, { exports: canonical });
const envelopes = {};
vm.runInNewContext(ts.transpileModule(fs.readFileSync(require.resolve('../src/cloud/crypto/envelope.ts'), 'utf8'), { compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 } }).outputText, { exports: envelopes, TextEncoder, atob, require: name => name === './canonical' ? canonical : require(name) });
const sdk = require('@matrix-org/matrix-sdk-crypto-wasm');
require('fake-indexeddb/auto');
(async () => {
  await sdk.initAsync();
  const account = '00000000-0000-4000-8000-000000000001';
  const cid = '00000000-0000-4000-8000-000000000002';
  assert.equal(new sdk.RoomId(`!${cid}:llmn`).toString(), `!${cid}:llmn`);
  for (const kind of ['profile', 'memory', 'summary', 'summarySettings']) assert.equal(new sdk.RoomId(`!${account}.${kind}.${cid}:llmn`).toString(), `!${account}.${kind}.${cid}:llmn`);
  const store = `llmn-test-${require('node:crypto').randomUUID()}`;
  const uid = `@${account}:llmn`, did = 'isolated-sdk-device';
  const machine = await sdk.OlmMachine.initialize(new sdk.UserId(uid), new sdk.DeviceId(did), store, 'independent-local-passphrase');
  const fingerprint = machine.identityKeys.ed25519.toBase64();
  const bootstrap = await machine.bootstrapCrossSigning(false);
  const master = Object.values(JSON.parse(bootstrap.uploadSigningKeysRequest.body).master_key.keys)[0];
  const envelope = { version: 1, roomId: `!${cid}:llmn`, sender: uid, deviceId: did, eventId: "$message.1.final", content: { device_id: did, sender_key: machine.identityKeys.curve25519.toBase64(), ciphertext: "synthetic-cipher" }, deviceKeys: { ed25519: machine.identityKeys.ed25519.toBase64(), curve25519: machine.identityKeys.curve25519.toBase64() }, signatures: {} };
  envelope.signatures = JSON.parse((await machine.sign(canonical.canonicalJson(envelopes.unsignedEnvelope(envelope)))).asJSON());
  envelopes.verifyEnvelope(envelope, master);
  assert.throws(() => envelopes.verifyEnvelope({ ...envelope, eventId: "$other.1.final" }, master), /签名/);
  assert.throws(() => envelopes.verifyEnvelope({ ...envelope, signatures: {} }, master), /双签名/);
  assert.throws(() => envelopes.verifyEnvelope({ ...envelope, deviceKeys: { ...envelope.deviceKeys, ed25519: master } }, master), /签名/);
  assert.throws(() => canonical.canonicalJson({ invalid: 1.5 }), /安全整数/);
  const room = new sdk.RoomId(`!${cid}:llmn`), settings = new sdk.EncryptionSettings();
  settings.algorithm = sdk.EncryptionAlgorithm.MegolmV1AesSha2;
  settings.sharingStrategy = sdk.CollectStrategy.onlyTrustedDevices();
  await machine.shareRoomKey(room, [], settings);
  const before = JSON.parse(await machine.encryptRoomEvent(room, 'com.llmn.content.v1', JSON.stringify({ text: 'first offline message' })));
  const oldKeys = await machine.exportRoomKeys(() => true);
  await machine.invalidateGroupSession(room);
  await machine.shareRoomKey(room, [], settings);
  const after = JSON.parse(await machine.encryptRoomEvent(room, 'com.llmn.content.v1', JSON.stringify({ text: 'second offline message' })));
  assert.notEqual(before.session_id, after.session_id, 'a new offline message rotates the official Megolm sender chain');
  const revokedReader = await sdk.OlmMachine.initialize(new sdk.UserId('@old-admin:llmn'), new sdk.DeviceId('revoked-reader'));
  await revokedReader.importExportedRoomKeys(oldKeys, () => {});
  const event = content => JSON.stringify({ type: 'm.room.encrypted', event_id: '$offline', sender: uid, origin_server_ts: 1, content });
  const permissive = new sdk.DecryptionSettings(sdk.TrustRequirement.Untrusted);
  assert.ok(await revokedReader.decryptRoomEvent(event(before), room, permissive));
  await assert.rejects(revokedReader.decryptRoomEvent(event(after), room, permissive), 'an old administrator history export cannot decrypt the next chain');
  await revokedReader.importExportedRoomKeys(await machine.exportRoomKeys(() => true), () => {});
  assert.ok(await revokedReader.decryptRoomEvent(event(after), room, permissive), 'the freshly delivered official key unlocks the new chain offline');
  revokedReader.close();
  const key = sdk.BackupDecryptionKey.createRandomKey();
  await machine.saveBackupDecryptionKey(key, 'backup-version');
  const backupPublic = key.megolmV1PublicKey.publicKeyBase64;
  await machine.enableBackupV1(backupPublic, 'backup-version');
  const backup = await machine.backupRoomKeys();
  assert.ok(backup, 'the fixture must have actual encrypted room-key backup data');
  const backupBody = JSON.parse(backup.body);
  const bundle = (await machine.exportSecretsBundle()).to_json();
  assert.ok(bundle);
  machine.close();
  await assert.rejects(sdk.OlmMachine.initialize(new sdk.UserId(uid), new sdk.DeviceId(did), store, 'wrong-passphrase'));
  const reopened = await sdk.OlmMachine.initialize(new sdk.UserId(uid), new sdk.DeviceId(did), store, 'independent-local-passphrase');
  assert.equal(reopened.identityKeys.ed25519.toBase64(), fingerprint, 'encrypted IndexedDB preserves the original device identity');
  assert.equal((await reopened.getBackupKeys()).backupVersion, 'backup-version');
  assert.equal((await reopened.crossSigningStatus()).hasMaster, true);
  reopened.close();
  key.free();
  const recovery = {}, storage = new Map(), reads = [];
  let recoveryCiphertext;
  const api = { server: 'https://sdk-recovery.invalid', request: async (path, method, body) => {
    reads.push(path);
    if (path === '/crypto/recovery') return { version: 'recovery-version', ciphertext: recoveryCiphertext };
    if (path === '/crypto/backup') return { version: 'backup-version', algorithm: 'm.megolm_backup.v1.curve25519-aes-sha2', auth_data: { public_key: backupPublic } };
    if (path === '/crypto/backup/keys') return backupBody;
    if (path === '/crypto/requests') { assert.equal(body.kind, 'signatures'); return {}; }
    throw Error(`unexpected recovery request: ${path}`);
  } };
  const source = ts.transpileModule(fs.readFileSync(require.resolve('../src/cloud/crypto/engine.ts'), 'utf8'), { compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 } }).outputText;
  vm.runInNewContext(source + '\nexports.seed = machine => { active = { sdk: testSdk, machine, account: testAccount, owner:ownerOf(testAccount), deviceId:machine.deviceId.toString(), cursor:"0" }; syncCrypto = async () => {}; };', {
    exports: recovery, testSdk: sdk, testAccount: account, crypto: require('node:crypto').webcrypto, TextEncoder, TextDecoder, Uint8Array, Map, atob, btoa, Event,
    window: { dispatchEvent() {} }, localStorage: { getItem: name => storage.get(name) ?? null, setItem: (name, value) => storage.set(name, value) },
    require: name => name === '../api' ? api : name === './envelope' ? envelopes : name === './canonical' ? canonical : require(name),
  });
  const recoveryFile = data => {
    const encrypted = sdk.Attachment.encrypt(new TextEncoder().encode(JSON.stringify(data)));
    recoveryCiphertext = Buffer.from(encrypted.encryptedData).toString('base64');
    return { format: 'llmn-recovery-v1', server: api.server, accountId: account, version: 'recovery-version', mediaEncryptionInfo: encrypted.mediaEncryptionInfo };
  };
  const restored = await sdk.OlmMachine.initialize(new sdk.UserId(uid), new sdk.DeviceId('independent-recovered-device'));
  recovery.seed(restored);
  assert.equal((await restored.getBackupKeys()).decryptionKey, undefined);
  await assert.rejects(restored.decryptRoomEvent(event(after), room, permissive));
  const file = recoveryFile(bundle);
  await assert.rejects(recovery.restoreRecoveryFile({ ...file, accountId: 'another-account' }), /不属于/);
  assert.equal(reads.length, 0, 'a wrong-account recovery file must fail before cloud access');
  await recovery.restoreRecoveryFile(file);
  const restoredBackup = await restored.getBackupKeys();
  assert.equal(restoredBackup.backupVersion, 'backup-version');
  assert.equal(restoredBackup.decryptionKey.megolmV1PublicKey.publicKeyBase64, backupPublic);
  assert.equal(JSON.parse((await restored.decryptRoomEvent(event(after), room, permissive)).event).content.text, 'second offline message');
  restored.close();
  const incomplete = await sdk.OlmMachine.initialize(new sdk.UserId(uid), new sdk.DeviceId('incomplete-recovery-device'));
  recovery.seed(incomplete);
  const withoutBackup = structuredClone(bundle); delete withoutBackup.backup;
  await assert.rejects(recovery.restoreRecoveryFile(recoveryFile(withoutBackup)), /缺少历史备份密钥/);
  assert.equal((await incomplete.getBackupKeys()).decryptionKey, undefined);
  await assert.rejects(incomplete.decryptRoomEvent(event(after), room, permissive));
  incomplete.close();
  const localReceiver = await sdk.OlmMachine.initialize(new sdk.UserId(uid), new sdk.DeviceId('local-key-receiver'));
  recovery.seed(localReceiver);
  for (const outgoing of await localReceiver.outgoingRequests()) {
    if (outgoing.type === sdk.RequestType.KeysUpload) await localReceiver.markRequestAsSent(outgoing.id, outgoing.type, JSON.stringify({ one_time_key_counts: { signed_curve25519: 100 } }));
  }
  const localDelivery = [{ event_type: 'm.dummy', messages: { [uid]: { 'local-key-receiver': {} } } }];
  for (let i = 0; i < 8; i++) {
    await recovery.receiveLocalKeys(account, localDelivery);
    assert.equal((await localReceiver.outgoingRequests()).filter(request => request.type === sdk.RequestType.KeysUpload).length, 0, 'replayed local deliveries have no server key counts and must not replenish healthy one-time or fallback keys');
  }
  await localReceiver.receiveSyncChanges('[]', new sdk.DeviceLists([], []), new Map([['signed_curve25519', 0]]), undefined);
  assert.ok((await localReceiver.outgoingRequests()).some(request => request.type === sdk.RequestType.KeysUpload), 'a real cloud count of zero must still replenish one-time keys');
  localReceiver.close();
  const original = new TextEncoder().encode(JSON.stringify({ master_key: 'synthetic-secret', backup: 'synthetic-backup' }));
  const encrypted = sdk.Attachment.encrypt(original);
  const info = encrypted.mediaEncryptionInfo;
  assert.ok(!new TextDecoder().decode(encrypted.encryptedData).includes('synthetic-secret'));
  assert.deepEqual(sdk.Attachment.decrypt(new sdk.EncryptedAttachment(encrypted.encryptedData, info)), original);
  const bad = encrypted.encryptedData; bad[0] ^= 1;
  assert.throws(() => sdk.Attachment.decrypt(new sdk.EncryptedAttachment(bad, info)), /hash|decrypt|mismatch/i);
  console.log('PASS: official SDK persists device identity, rotates room keys, restores history using only a fresh device and downloaded recovery/backup, rejects missing backup keys and wrong-account files, and authenticates attachment ciphertext');
})().catch(e => { console.error(e); process.exitCode = 1; });
