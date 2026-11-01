const assert = require('node:assert/strict'), fs = require('node:fs'), vm = require('node:vm'), ts = require('typescript');
const { webcrypto } = require('node:crypto');
const source = ts.transpileModule(fs.readFileSync(require.resolve('../src/cloud/crypto/engine.ts'), 'utf8'), { compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 } }).outputText;
const deferred = () => { let resolve, reject; const promise = new Promise((a, b) => { resolve = a; reject = b; }); return { promise, resolve, reject }; };
const tick = () => new Promise(resolve => setImmediate(resolve));
class HttpError extends Error { constructor(status) { super('permission denied'); this.status = status; } }
class ConnectionError extends Error {}
function fixture({ saved = true, trusted = true } = {}) {
  const target = new EventTarget();
  const storage = new Map(), events = [], calls = [], gate = deferred(), registered = deferred();
  const state = { trusted, registrationError: undefined, wrong: false, receives: 0, closed: 0 };
  const api = { server: 'https://unlock.invalid', HttpError, ConnectionError, request: async path => {
    calls.push(path);
    if (path === '/crypto/devices') { registered.resolve(); if (state.registrationError) throw state.registrationError; return {}; }
    if (path.startsWith('/crypto/sync?')) return gate.promise;
    throw new Error('unexpected request ' + path);
  } };
  const key = account => `llmn-crypto-device:${JSON.stringify([api.server, account])}`;
  if (saved) storage.set(key('a'), JSON.stringify({ deviceId: 'local-device', cursor: '0' }));
  const machine = { close() { state.closed++; }, async updateTrackedUsers() {}, async getIdentity() { return { masterKey: '{"keys":{"master":"master"}}', isVerified: () => state.trusted, trustsOurOwnDevice: async () => state.trusted }; }, async crossSigningStatus() { return {}; }, identityKeys: { ed25519: { toBase64: () => 'ed' }, curve25519: { toBase64: () => 'curve' } }, async outgoingRequests() { return []; }, async receiveSyncChanges() { state.receives++; return []; }, async backupRoomKeys() { return state.backup?.promise; } };
  class Id { constructor(value) { this.value = value; } }
  const sdk = { initAsync: async () => {}, UserId: Id, DeviceId: Id, DeviceLists: Id, DecryptionSettings: Id, TrustRequirement: { CrossSigned: 1 }, OlmMachine: { initialize: async (_, __, ___, pass) => { if (pass !== 'correct') throw new Error('wrong passphrase'); return machine; } } };
  const exports = {};
  vm.runInNewContext(source, { exports, Error, crypto: webcrypto, TextEncoder, Event, CustomEvent: class extends Event { constructor(name, options) { super(name); this.detail = options.detail; } }, window: { dispatchEvent: event => { events.push(event); target.dispatchEvent(event); }, addEventListener: (...args) => target.addEventListener(...args), removeEventListener: (...args) => target.removeEventListener(...args) }, localStorage: { getItem: key => storage.get(key) ?? null, setItem: (key, value) => storage.set(key, value) }, require: name => name === '../api' ? api : name === '@matrix-org/matrix-sdk-crypto-wasm' ? sdk : {} });
  const finish = () => gate.resolve({ next_batch: 'done', to_device: { events: [] }, device_lists: { changed: [], left: [] }, device_one_time_keys_count: {}, device_unused_fallback_key_types: [] });
  return { exports, state, events, calls, gate, registered, storage, key, finish, api, target };
}
async function fastUnlock(h) {
  let settled = false;
  const opening = h.exports.unlockCrypto('a', 'correct').then(value => { settled = true; return value; });
  await h.registered.promise; await tick();
  assert.equal(settled, true, 'trusted unlock must settle before deferred network sync');
  return opening;
}
function subscribeWarnings(h, reports) {
  const app = fs.readFileSync(require.resolve('../src/cloud/ChatApp.tsx'), 'utf8').replace(/\r\n/g, '\n');
  const start = app.indexOf('  useEffect(() => {\n    if (!me) return;\n    const update =');
  const end = app.indexOf('  }, [me?.id]);', start) + '  }, [me?.id]);'.length;
  assert.ok(start >= 0 && end > start);
  const code = ts.transpileModule(app.slice(start, end), { compilerOptions: { target: ts.ScriptTarget.ES2022 } }).outputText;
  let cleanup;
  vm.runInNewContext(code, { useEffect: fn => { cleanup = fn(); }, me: { id: 'a' }, consumeCryptoWarning: h.exports.consumeCryptoWarning, cryptoDevice: h.exports.cryptoDevice, window: h.target, setInterval: () => 1, clearInterval() {}, report: error => reports.push(error.message), Error });
  return cleanup;
}
(async () => {
  const trusted = fixture();
  const status = await fastUnlock(trusted);
  assert.equal(status.trusted, true);
  assert.equal(trusted.calls[0], '/crypto/devices');
  assert.ok(trusted.calls.some(path => path.startsWith('/crypto/sync?')));
  assert.equal(trusted.state.receives, 0, 'unlock returns while full sync remains deferred');
  trusted.finish(); await tick();
  assert.equal(trusted.state.receives, 1);
  assert.equal(JSON.parse(trusted.storage.get(trusted.key('a'))).cursor, 'done');
  for (const status of [401, 403]) {
    const h = fixture(); h.state.registrationError = new HttpError(status);
    await assert.rejects(h.exports.unlockCrypto('a', 'correct'), error => error.status === status);
    assert.equal(h.exports.cryptoDevice('a'), undefined);
    assert.equal(h.calls.length, 1, 'rejected registration never starts background sync');
  }
  for (const options of [{ saved: false }, { trusted: false }]) {
    const h = fixture(options); let settled = false;
    const opening = h.exports.unlockCrypto('a', 'correct', !options.saved && options.saved !== undefined).then(value => { settled = true; return value; });
    await h.registered.promise; await tick();
    assert.equal(settled, false, 'new or untrusted devices await full sync');
    h.finish(); await opening;
  }
  const wrong = fixture();
  await assert.rejects(wrong.exports.unlockCrypto('a', 'wrong'), /wrong passphrase/);
  assert.equal(wrong.exports.cryptoDevice('a'), undefined); assert.equal(wrong.calls.length, 0);
  const failure = fixture(); await fastUnlock(failure);
  failure.gate.reject(new Error('background unavailable')); await tick();
  assert.equal(failure.events.find(event => event.type === 'llmn-crypto-warning')?.detail, 'background unavailable');
  const reports = [];
  const detach = subscribeWarnings(failure, reports);
  assert.deepEqual(reports, ['background unavailable'], 'first subscription consumes failure dispatched before mounting');
  detach(); subscribeWarnings(failure, reports)();
  assert.equal(reports.length, 1, 'remount does not repeat consumed warning');
  const live = fixture(); await fastUnlock(live);
  const liveReports = [], detachLive = subscribeWarnings(live, liveReports);
  live.gate.reject(new Error('live failure')); await tick(); detachLive();
  subscribeWarnings(live, liveReports)();
  assert.deepEqual(liveReports, ['live failure'], 'live event consumes pending warning before remount');
  const discarded = fixture(); await fastUnlock(discarded);
  discarded.gate.reject(new Error('old secret-free error')); await tick(); discarded.exports.lockCrypto();
  assert.equal(discarded.exports.consumeCryptoWarning('a'), undefined, 'locking discards queued warning');
  discarded.storage.set(discarded.key('b'), JSON.stringify({ deviceId: 'device-b', cursor: '0' }));
  discarded.state.registrationError = new ConnectionError('offline'); await discarded.exports.unlockCrypto('b', 'correct');
  assert.equal(discarded.exports.consumeCryptoWarning('b'), undefined, 'queued warning never leaks into next account');
  const offline = fixture(); offline.state.registrationError = new ConnectionError('offline');
  assert.equal((await offline.exports.unlockCrypto('a', 'correct')).trusted, true);
  for (const switchAccount of [false, true]) {
    const h = fixture(); await fastUnlock(h); h.exports.lockCrypto();
    if (switchAccount) { h.state.registrationError = new ConnectionError('offline'); h.storage.set(h.key('b'), JSON.stringify({ deviceId: 'device-b', cursor: '0' })); await h.exports.unlockCrypto('b', 'correct'); }
    const before = [...h.storage], eventCount = h.events.length;
    h.finish(); await tick();
    assert.equal(h.state.receives, 0, 'late sync cannot enter the closed SDK');
    assert.deepEqual([...h.storage], before); assert.equal(h.events.length, eventCount);
  }
  const lateBackup = fixture(); lateBackup.state.backup = deferred();
  await fastUnlock(lateBackup); lateBackup.finish(); await tick();
  lateBackup.exports.lockCrypto(); const requestCount = lateBackup.calls.length;
  lateBackup.state.backup.resolve({ id: 'backup', body: '{}', type: 6 }); await tick();
  assert.equal(lateBackup.calls.length, requestCount, 'late backup does not dispatch another network request');
  const revokedOld = fixture(); await fastUnlock(revokedOld); revokedOld.exports.lockCrypto();
  revokedOld.storage.set(revokedOld.key('b'), JSON.stringify({ deviceId: 'device-b', cursor: '0' }));
  revokedOld.state.registrationError = new ConnectionError('offline'); await revokedOld.exports.unlockCrypto('b', 'correct');
  revokedOld.gate.reject(new HttpError(403)); await tick();
  assert.equal(revokedOld.exports.cryptoDevice('b'), 'device-b', 'late permission failure cannot revoke the next account session');
  const cancelled = fixture(); await fastUnlock(cancelled); cancelled.exports.lockCrypto();
  const eventCount = cancelled.events.length; cancelled.gate.reject(new Error('late failure')); await tick();
  assert.equal(cancelled.events.length, eventCount, 'old session failures do not warn in the new UI');
  console.log('PASS: trusted local unlock returns before deferred sync; registration, new-device, passphrase, offline and stale-session boundaries remain enforced');
})().catch(error => { console.error(error); process.exitCode = 1; });
