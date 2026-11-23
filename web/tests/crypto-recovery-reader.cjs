const assert = require('node:assert/strict'), fs = require('node:fs'), vm = require('node:vm'), ts = require('typescript');
const recovered = {}, reads = [], effects = [];
let imported = false, closed = false;
const temporary = {
  importSecretsBundle: async value => { assert.equal(value.source, 'downloaded recovery'); effects.push('bundle'); },
  getBackupKeys: async () => ({ backupVersion: 'backup', decryptionKey: { megolmV1PublicKey: { publicKeyBase64: 'backup-public' }, decryptV1: () => JSON.stringify({ room_id: '!room:llmn', session_key: 'from-downloaded-backup' }) } }),
  enableBackupV1: async () => {}, importBackedUpRoomKeys: async rooms => { assert.equal(rooms.get([...rooms.keys()][0]).get('session').session_key, 'from-downloaded-backup'); imported = true; },
  decryptRoomEvent: async () => { assert.ok(imported); effects.push('temporary-decrypt'); return { event: JSON.stringify({ type: 'com.llmn.content.v1', content: { text: 'recovered' } }) }; },
  close: () => { closed = true; },
};
class Id { constructor(id) { this.id = id; } toString() { return this.id; } }
const sdk = { UserId: Id, DeviceId: Id, RoomId: Id, DecryptionSettings: class {}, TrustRequirement: { Untrusted: 'untrusted' }, OlmMachine: { initialize: async (...args) => { assert.equal(args.length, 2, 'new in-memory instance never receives an active store name or copied database'); return temporary; } }, Attachment: { decrypt: () => new TextEncoder().encode(JSON.stringify({ source: 'downloaded recovery' })) }, EncryptedAttachment: class {}, SecretsBundle: { from_json: value => value } };
const main = { backupRoomKeys: async () => null, getIdentity: async () => ({ isVerified: () => true, masterKey: 'verified-master' }), decryptRoomEvent: () => { throw Error('must never borrow active room keys'); }, close() {} };
const api = { server: 'https://recovery.invalid', request: async path => { reads.push(path); if (path === '/crypto/recovery?version=recovery') return { version: 'recovery', ciphertext: 'YQ==' }; if (path === '/crypto/backup?version=backup') return { version: 'backup', algorithm: 'm.megolm_backup.v1.curve25519-aes-sha2', auth_data: { public_key: 'backup-public' } }; if (path === '/crypto/backup/keys?version=backup') return { rooms: { '!room:llmn': { sessions: { session: { session_data: {} } } } } }; throw Error(path); } };
const code = ts.transpileModule(fs.readFileSync(require.resolve('../src/cloud/crypto/engine.ts'), 'utf8'), { compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 } }).outputText;
vm.runInNewContext(code + `
exports.prepare = (old, keys) => prepareBackup(active, old, keys);
exports.seed = (sdk, machine) => { active = { sdk, machine, account:'human', owner:ownerOf('human'), deviceId:'original', cursor:'0' }; syncCrypto = async () => {}; };`, { exports: recovered, TextEncoder, TextDecoder, Uint8Array, Map, atob, crypto: require('node:crypto').webcrypto, require: name => name === '../api' ? api : name === './canonical' ? { canonicalJson: JSON.stringify } : name === './envelope' ? { verifyEnvelope: () => effects.push('verify-signature') } : {} });
(async () => {
  recovered.seed(sdk, main);
  const file = { format: 'llmn-recovery-v1', server: api.server, accountId: 'human', version: 'recovery', mediaEncryptionInfo: 'local-only' };
  await assert.rejects(recovered.recoveryReader({ ...file, accountId: 'other' }), /不属于/);
  const reader = await recovered.recoveryReader(file);
  const result = await reader.unseal({ sender: '@migrator:llmn', eventId: '$old.1.migration.id', roomId: '!room:llmn', content: {} });
  assert.equal(result.content.text, 'recovered');
  assert.deepEqual(effects, ['bundle', 'verify-signature', 'temporary-decrypt']);
  assert.deepEqual(reads, ['/crypto/recovery?version=recovery', '/crypto/backup?version=backup', '/crypto/backup/keys?version=backup']);
  reader.close(); assert.ok(closed);
  // A sealed backup is immutable history. Its key is optional for opening a fresh
  // generation, but a healthy active generation must never be replaced without it.
  let minted = 0;
  sdk.BackupDecryptionKey = { createRandomKey: () => ({ megolmV1PublicKey: { publicKeyBase64: 'fresh-' + (++minted) } }) };
  main.sign = async () => ({ asJSON: () => '{}' });
  main.importBackedUpRoomKeys = temporary.importBackedUpRoomKeys;
  const old = { version: 'backup', auth_data: { public_key: 'backup-public' } };
  const matching = await temporary.getBackupKeys();
  const other = { ...matching, backupVersion: 'different' };
  reads.length = 0;
  await assert.rejects(recovered.prepare(old, {}), /不会被覆盖/);
  await assert.rejects(recovered.prepare(old, other), /不会被覆盖/);
  assert.equal((await recovered.prepare(old, matching)).key, matching.decryptionKey);
  assert.equal(minted, 0);
  for (const unavailable of [{}, other]) {
    const fresh = await recovered.prepare({ ...old, sealed: true }, unavailable);
    assert.equal(fresh.newBackup.expectedVersion, old.version);
    assert.notEqual(fresh.newBackup.version, old.version);
    assert.notEqual(fresh.newBackup.auth_data.public_key, old.auth_data.public_key);
  }
  assert.deepEqual(reads, [], 'never fetch the archived keys with a mismatched private key');
  await recovered.prepare({ ...old, sealed: true }, matching);
  assert.deepEqual(reads, ['/crypto/backup/keys?version=backup']);
  console.log('PASS: independent recovery creates a new ephemeral SDK instance and imports only recovery bundle plus downloaded backup; active keys cannot satisfy verification');
})().catch(error => { console.error(error); process.exitCode = 1; });
