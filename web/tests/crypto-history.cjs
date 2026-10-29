const assert = require('node:assert/strict'), fs = require('node:fs'), vm = require('node:vm'), ts = require('typescript');
class HttpError extends Error { constructor(status) { super(`HTTP ${status}`); this.status = status; } }
const cache = new Map(), calls = [], notices = [], imports = [];
const api = { server: 'https://fixture.invalid', HttpError, ConnectionError: class extends Error {}, request: async (...args) => { calls.push(args); return api.respond(...args); }, respond: async () => ({}) };
const engine = {};
const code = ts.transpileModule(fs.readFileSync(require.resolve('../src/cloud/crypto/engine.ts'), 'utf8'), { compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 } }).outputText;
vm.runInNewContext(code + `
exports.seed = (sdk, machine) => { active = { sdk, machine, account: 'self', owner: ownerOf('self'), deviceId: 'device', cursor: '0' }; }; exports.flushTest = () => flush(active);`, { exports: engine, crypto: require('node:crypto').webcrypto, Event, CustomEvent, window: { dispatchEvent: e => notices.push(e) }, localStorage: { getItem: key => cache.get(key) ?? null, setItem: (key, value) => cache.set(key, value) }, require: name => name === '../api' ? api : {} });
const key = 'llmn-crypto-outgoing:' + JSON.stringify([api.server, 'self']);
const history = id => ({ id, kind: 'toDevice', body: { llmn_purpose: 'conversationHistory', llmn_conversation_id: 'group', llmn_expected_revision: '7', event_type: 'm.room.encrypted', txn_id: id, messages: {} } });
(async () => {
  engine.seed({}, { outgoingRequests: async () => [] });
  cache.set(key, JSON.stringify([history('stale-one'), history('stale-two'), { id: 'new-message-key', kind: 'toDevice', body: { event_type: 'm.room.encrypted', txn_id: 'new-message-key', messages: {} } }]));
  api.respond = async () => { throw new HttpError(409); };
  await assert.rejects(engine.flushTest(), e => e.status === 409);
  assert.equal(JSON.parse(cache.get(key)).length, 1, 'one definitive audience rejection removes only the expired sharing audience');
  api.respond = async () => ({}); await engine.flushTest();
  assert.equal(calls.at(-1)[2].requestId, 'new-message-key', 'membership changes do not permanently block fresh message keys');
  assert.equal(cache.get(key), '[]');
  cache.set(key, JSON.stringify([history('unknown-history')]));
  api.respond = async () => { throw Error('network lost'); };
  await assert.rejects(engine.flushTest(), /network lost/);
  assert.equal(JSON.parse(cache.get(key))[0].id, 'unknown-history');
  api.respond = async () => ({}); await engine.flushTest();
  assert.equal(calls.at(-1)[2].requestId, 'unknown-history', 'unknown delivery retries the original request');

  const processed = (revision, keys) => ({ encryptionInfo: { isSenderVerified: () => true, sender: { toString: () => '@member:llmn' } }, rawEvent: JSON.stringify({ sender: '@member:llmn', type: 'com.llmn.group-history.v1', content: { conversationId: 'group', revision, keys } }) });
  const sdk = { UserId: class { constructor(id) { this.id = id; } }, DeviceLists: class {}, DecryptionSettings: class {}, TrustRequirement: { CrossSigned: 'cross' } };
  engine.seed(sdk, { outgoingRequests: async () => [], receiveSyncChanges: async () => [processed('7', [{ room_id: '!group:llmn' }]), processed('8', [{ room_id: '!other:llmn' }]), processed('8', [{ room_id: '!group:llmn', session_id: 'accepted' }])], importExportedRoomKeys: async keys => imports.push(JSON.parse(keys)), backupRoomKeys: async () => null });
  api.respond = async path => path.startsWith('/crypto/sync') ? { next_batch: '42', to_device: { events: [] }, device_lists: { changed: [], left: [] }, device_one_time_keys_count: {}, device_unused_fallback_key_types: [] } : path === '/chat/init' ? { conversations: [{ id: 'group', revision: '8', members: [{ account: { id: 'member' } }] }] } : {};
  await engine.syncCrypto();
  assert.equal(imports.length, 1); assert.equal(imports[0][0].session_id, 'accepted');
  assert.equal(notices.filter(e => e.type === 'llmn-crypto-warning').length, 2);
  const metadata = cache.get('llmn-crypto-device:' + JSON.stringify([api.server, 'self']));
  assert.equal(JSON.parse(metadata).cursor, '42', 'rejected expired or cross-room history cannot wedge the sync cursor');
  console.log('PASS: revoked history audiences cannot block fresh message keys; unknown delivery retries and rejected received history advances sync without importing');
})().catch(error => { console.error(error); process.exitCode = 1; });
