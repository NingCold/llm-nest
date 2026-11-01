const assert = require('node:assert/strict'), fs = require('node:fs'), vm = require('node:vm'), ts = require('typescript');
const source = ts.transpileModule(fs.readFileSync(require.resolve('../src/cloud/crypto/engine.ts'), 'utf8'), { compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 } }).outputText;
const deferred = () => { let resolve; const promise = new Promise(r => { resolve = r; }); return { promise, resolve }; };
function fixture() {
  const storage = new Map(), events = [], calls = [];
  const state = { outgoing: [], received: [], response: { next_batch: '1', llmn_device_lists_version: 'v1', to_device: { events: [] }, device_lists: { changed: [], left: [] }, device_one_time_keys_count: {}, device_unused_fallback_key_types: [] } };
  class Id { constructor(value) { this.value = value; } }
  const machine = {
    async outgoingRequests() { return state.outgoing; },
    async markRequestAsSent(id) { state.outgoing = state.outgoing.filter(r => r.id !== id); },
    async receiveSyncChanges() { if (state.queryOnReceive) state.outgoing.push({ id: 'query', type: 1, body: '{}' }); return state.received; },
    async backupRoomKeys() { return state.backup; },
    async importExportedRoomKeys() { state.imported = true; },
    async updateTrackedUsers() {}, async getUserDevices() { return { devices: () => [] }; }, async getIdentity() { return undefined; },
  };
  const sdk = { UserId: Id, DeviceLists: Id, DecryptionSettings: Id, TrustRequirement: { CrossSigned: 1 }, RequestType: { ToDevice: 3, KeysBackup: 6 }, KeysQueryRequest: class { constructor(id, body) { this.id = id; this.body = body; this.type = 1; } } };
  const api = { server: 'https://sync.invalid', HttpError: class extends Error {}, ConnectionError: class extends Error {}, request: async (path, method, body) => {
    calls.push({ path, body });
    if (path.startsWith('/crypto/sync?')) return state.response;
    if (path === '/crypto/requests') { if (state.gate) await state.gate.promise; if (body.requestId === state.failRequest) throw new Error('delivery failed'); return {}; }
    throw new Error('unexpected request ' + path);
  } };
  const exports = {};
  vm.runInNewContext(source + '\nactive = { account:"a", owner:ownerOf("a"), deviceId:"d", cursor:"0", machine, sdk }; exports.flushForTest = () => flush(active);', { exports, machine, sdk, crypto: require('node:crypto').webcrypto, Event, window: { dispatchEvent: e => events.push(e) }, localStorage: { getItem: key => storage.get(key) ?? null, setItem: (key, value) => storage.set(key, value) }, require: name => name === '../api' ? api : {} });
  return { exports, state, storage, events, calls };
}
(async () => {
  const idle = fixture();
  await idle.exports.syncCrypto(); await idle.exports.syncCrypto();
  assert.equal(idle.events.length, 0, 'empty sync never refreshes UI');
  assert.ok(idle.calls[1].path.includes('after=1&deviceListsVersion=v1'), 'empty sync still confirms cursor and directory version');
  assert.equal(JSON.parse([...idle.storage.values()][0]).cursor, '1');
  for (const change of ['incoming', 'changed', 'left', 'beforeFlush', 'afterFlush']) {
    const h = fixture();
    if (change === 'incoming') h.state.response.to_device.events.push({ type: 'm.room_key' });
    if (change === 'changed' || change === 'left') h.state.response.device_lists[change].push('@a:llmn');
    if (change === 'beforeFlush') h.state.outgoing.push({ id: 'q', type: 1, body: '{}' });
    if (change === 'afterFlush') h.state.queryOnReceive = true;
    await h.exports.syncCrypto();
    assert.equal(h.events.length, 1, change + ' triggers a refresh');
  }
  const authorization = fixture();
  authorization.state.response.to_device.events.push({});
  authorization.state.received = [{ encryptionInfo: { isSenderVerified: () => true, sender: { toString: () => '@a:llmn' }, senderDevice: { toString: () => 'sender' } }, rawEvent: JSON.stringify({ type: 'com.llmn.recovery.v1', sender: '@a:llmn', content: { senderDevice: 'sender', secrets: {}, roomKeys: [] } }) }];
  await authorization.exports.syncCrypto(); assert.equal(authorization.events.length, 1);
  const history = fixture();
  history.state.response.to_device.events.push({});
  history.state.received = [{ encryptionInfo: { isSenderVerified: () => true, sender: { toString: () => '@ai:llmn' } }, rawEvent: JSON.stringify({ type: 'com.llmn.ai-history.v1', sender: '@ai:llmn', content: { aiId: 'ai', keys: [] } }) }];
  await history.exports.syncCrypto(); assert.equal(history.state.imported, true); assert.equal(history.events.length, 1, 'imported history keys notify');
  const queued = fixture();
  queued.storage.set('llmn-crypto-outgoing:' + JSON.stringify(['https://sync.invalid', 'a']), JSON.stringify([{ id: 'persisted', kind: 'signatures', body: {} }]));
  await queued.exports.syncCrypto(); assert.equal(queued.events.length, 1, 'persisted outgoing work counts');
  const queried = fixture(); await queried.exports.devices('other');
  assert.equal(queried.events.length, 1, 'explicit tracked-user query notifies even after empty sync');
  const concurrent = fixture(); concurrent.state.gate = deferred(); concurrent.state.outgoing.push({ id: 'q', type: 1, body: '{}' });
  const first = concurrent.exports.flushForTest(), second = concurrent.exports.flushForTest();
  concurrent.state.gate.resolve();
  await Promise.all([first, second]);
  assert.equal(concurrent.calls.filter(c => c.path === '/crypto/requests' && c.body.requestId === 'q').length, 1, 'concurrent flush sends each request once');
  await concurrent.exports.syncCrypto();
  assert.equal(concurrent.events.length, 1, 'empty sync notifies work completed during concurrent flush');
  await concurrent.exports.syncCrypto();
  assert.equal(concurrent.events.length, 1, 'the notification is consumed once');
  const backup = fixture(); backup.state.backup = { id: 'backup', type: 6, body: '{}', version: 'v' };
  await backup.exports.syncCrypto(); assert.equal(backup.events.length, 0, 'backup delivery alone does not refresh UI');
  for (const failure of ['backup', 'partialFlush']) {
    const retry = fixture();
    retry.state.outgoing.push({ id: 'query', type: 1, body: '{}' });
    if (failure === 'backup') retry.state.backup = { id: 'failed', type: 6, body: '{}', version: 'v' };
    else retry.state.outgoing.push({ id: 'failed', type: 1, body: '{}' });
    retry.state.failRequest = 'failed';
    await assert.rejects(retry.exports.syncCrypto(), /delivery failed/);
    assert.equal(retry.events.length, 0, failure + ': failed round does not notify');
    assert.equal(retry.state.outgoing.some(r => r.id === 'query'), false, 'successful query was already acknowledged');
    // The failed request is no longer pending: the retry itself can be completely empty.
    retry.state.outgoing = []; retry.state.backup = undefined; retry.state.failRequest = undefined;
    await retry.exports.syncCrypto();
    assert.equal(retry.events.length, 1, failure + ': empty retry retains already applied changes');
    await retry.exports.syncCrypto();
    assert.equal(retry.events.length, 1, failure + ': successful notification clears the pending change');
  }
  console.log('PASS: idle crypto sync stays quiet while keys, directory changes, explicit queries and concurrent flush work notify');
})().catch(error => { console.error(error); process.exitCode = 1; });
