const assert = require('node:assert/strict'), fs = require('node:fs'), vm = require('node:vm'), ts = require('typescript');
const { performance: nativePerformance } = require('node:perf_hooks');
const { webcrypto } = require('node:crypto');
const source = ts.transpileModule(fs.readFileSync(require.resolve('../src/cloud/crypto/engine.ts'), 'utf8'), { compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 } }).outputText;
const deferred = () => { let resolve, reject; const promise = new Promise((a, b) => { resolve = a; reject = b; }); return { promise, resolve, reject }; };
const tick = () => new Promise(resolve => setImmediate(resolve));
class HttpError extends Error { constructor(status) { super('permission denied'); this.status = status; } }
class ConnectionError extends Error {}
function fixture({ saved = true, trusted = true } = {}) {
  const target = new EventTarget();
  const measurements = [];
  const performance = { now: () => nativePerformance.now(), clearMeasures: name => { for (let i = measurements.length - 1; i >= 0; i--) if (measurements[i].name === name) measurements.splice(i, 1); }, measure: (name, options) => measurements.push({ name, ...options }) };
  const storage = new Map(), events = [], calls = [], gate = deferred(), registered = deferred();
  const state = { trusted, registrationError: undefined, wrong: false, receives: 0, closed: 0 };
  const api = { server: 'https://unlock.invalid', HttpError, ConnectionError, request: async path => {
    calls.push(path);
    if (path === '/crypto/devices') { registered.resolve(); if (state.registrationError) throw state.registrationError; return {}; }
    if (path.startsWith('/crypto/sync?')) return state.syncResponse ?? gate.promise;
    if (path === '/crypto/requests') { if (state.flushGate) await state.flushGate.promise; if (state.flushError) throw state.flushError; state.sent++; return {}; }
    throw new Error('unexpected request ' + path);
  } };
  const key = account => `llmn-crypto-device:${JSON.stringify([api.server, account])}`;
  if (saved) storage.set(key('a'), JSON.stringify({ deviceId: 'local-device', cursor: '0' }));
  const machine = { close() { state.closed++; }, async updateTrackedUsers() {}, async getIdentity() { return { masterKey: '{"keys":{"master":"master"}}', isVerified: () => state.trusted, trustsOurOwnDevice: async () => state.trusted }; }, async crossSigningStatus() { return {}; }, identityKeys: { ed25519: { toBase64: () => 'ed' }, curve25519: { toBase64: () => 'curve' } }, async outgoingRequests() { return state.outgoing ?? []; }, async markRequestAsSent(id) { if (state.markError) throw state.markError; state.outgoing = state.outgoing.filter(request => request.id !== id); }, async receiveSyncChanges() { state.receives++; if (state.queryOnChange && state.syncResponse.device_lists.changed.length) state.outgoing = [{ id: 'query', type: 1, body: '{}' }]; return []; }, async backupRoomKeys() { return state.backup?.promise; } };
  class Id { constructor(value) { this.value = value; } }
  const sdk = { RequestType: { ToDevice: 3, KeysBackup: 6 }, initAsync: async () => {}, UserId: Id, DeviceId: Id, DeviceLists: Id, DecryptionSettings: Id, TrustRequirement: { CrossSigned: 1 }, OlmMachine: { initialize: async (_, __, ___, pass) => { if (pass !== 'correct') throw new Error('wrong passphrase'); return machine; } } };
  const exports = {};
  vm.runInNewContext(source + '\nexports.flushForTest = () => flush(active);', { exports, Error, performance, crypto: webcrypto, TextEncoder, Event, CustomEvent: class extends Event { constructor(name, options) { super(name); this.detail = options.detail; } }, window: { dispatchEvent: event => { events.push(event); target.dispatchEvent(event); }, addEventListener: (...args) => target.addEventListener(...args), removeEventListener: (...args) => target.removeEventListener(...args) }, localStorage: { getItem: key => storage.get(key) ?? null, setItem: (key, value) => storage.set(key, value) }, require: name => name === '../api' ? api : name === '@matrix-org/matrix-sdk-crypto-wasm' ? sdk : {} });
  const finish = () => gate.resolve({ next_batch: 'done', to_device: { events: [] }, device_lists: { changed: [], left: [] }, device_one_time_keys_count: {}, device_unused_fallback_key_types: [] });
  return { exports, state, events, calls, gate, registered, storage, key, finish, api, target, measurements };
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
  assert.deepEqual(trusted.measurements.map(m => m.name.split('.').pop()).sort(), ['backgroundSync', 'deviceRegister', 'deviceStatus', 'sdkImport', 'sdkInit', 'storeOpen', 'trackedUsers', 'unlockReady'].sort());
  for (const measure of trusted.measurements) { assert.deepEqual(Object.keys(measure).sort(), ['end', 'name', 'start']); assert.ok(measure.end >= measure.start); }
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
    const before = [...h.storage], eventCount = h.events.length, measuredCount = h.measurements.length;
    h.finish(); await tick();
    assert.equal(h.state.receives, 0, 'late sync cannot enter the closed SDK');
    assert.deepEqual([...h.storage], before); assert.equal(h.events.length, eventCount); assert.equal(h.measurements.length, measuredCount);
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
  const versioned = fixture(); versioned.state.sent = 0; versioned.state.queryOnChange = true;
  const response = (version, changed = []) => ({ next_batch: 'cursor-' + version, llmn_device_lists_version: version, to_device: { events: [] }, device_lists: { changed, left: [] }, device_one_time_keys_count: {}, device_unused_fallback_key_types: [] });
  versioned.state.syncResponse = response('1', ['@synthetic:llmn']);
  await fastUnlock(versioned); await tick();
  assert.equal(JSON.parse(versioned.storage.get(versioned.key('a'))).deviceListsVersion, '1');
  assert.equal(versioned.state.sent, 1);
  versioned.state.syncResponse = response('1'); await versioned.exports.syncCrypto();
  assert.ok(versioned.calls.at(-1).includes('deviceListsVersion=1'));
  assert.equal(versioned.state.sent, 1, 'unchanged directory does not generate another key query');
  versioned.exports.lockCrypto(); await versioned.exports.unlockCrypto('a', 'correct'); await tick();
  assert.ok(versioned.calls.filter(p => p.startsWith('/crypto/sync?')).at(-1).includes('deviceListsVersion=1'), 'reopen retains confirmed directory version');
  assert.equal(versioned.measurements.filter(m => m.name.endsWith('unlockReady')).length, 1, 'only latest unlock measurements remain');
  versioned.state.syncResponse = response('2', ['@synthetic:llmn']); versioned.state.flushError = new Error('query delivery failed');
  await assert.rejects(versioned.exports.syncCrypto(), /query delivery failed/);
  assert.equal(JSON.parse(versioned.storage.get(versioned.key('a'))).deviceListsVersion, '1', 'failed query leaves acknowledged version unchanged');
  assert.equal(JSON.parse(versioned.storage.get(versioned.key('a'))).cursor, 'cursor-1');
  versioned.state.flushError = undefined; versioned.state.markError = new Error('SDK acknowledgement failed');
  await assert.rejects(versioned.exports.syncCrypto(), /SDK acknowledgement failed/);
  assert.equal(JSON.parse(versioned.storage.get(versioned.key('a'))).deviceListsVersion, '1');
  versioned.state.markError = undefined; versioned.state.flushGate = deferred();
  const delivery = versioned.exports.syncCrypto(); await tick();
  assert.equal(JSON.parse(versioned.storage.get(versioned.key('a'))).deviceListsVersion, '1', 'in-flight query is not acknowledged early');
  versioned.state.flushGate.resolve(); await delivery; versioned.state.flushGate = undefined;
  assert.equal(JSON.parse(versioned.storage.get(versioned.key('a'))).deviceListsVersion, '2');
  versioned.state.syncResponse = response(undefined); await versioned.exports.syncCrypto();
  assert.equal(JSON.parse(versioned.storage.get(versioned.key('a'))).deviceListsVersion, undefined, 'legacy server does not claim a confirmed version');
  versioned.exports.lockCrypto(); versioned.storage.set(versioned.key('b'), JSON.stringify({ deviceId: 'device-b', cursor: '0' }));
  await versioned.exports.unlockCrypto('b', 'correct'); await tick();
  assert.ok(!versioned.calls.filter(p => p.startsWith('/crypto/sync?')).at(-1).includes('deviceListsVersion='), 'another account never inherits the directory token');
  const serial = fixture(); await fastUnlock(serial); serial.state.sent = 0; serial.state.flushGate = deferred();
  serial.state.outgoing = [{ id: 'older-query', type: 1, body: '{}' }];
  const firstFlush = serial.exports.flushForTest(); await tick();
  serial.state.outgoing.push({ id: 'new-query', type: 1, body: '{}' });
  const secondFlush = serial.exports.flushForTest(); await tick();
  assert.equal(serial.calls.filter(p => p === '/crypto/requests').length, 1, 'dependent flush waits for previous delivery');
  serial.state.flushGate.resolve(); await Promise.all([firstFlush, secondFlush]);
  assert.equal(serial.state.sent, 2, 'waiting flush scans requests generated after previous snapshot');
  serial.finish(); await tick();
  const pendingVersion = fixture(); pendingVersion.state.syncResponse = response('5', ['@synthetic:llmn']); pendingVersion.state.queryOnChange = true; pendingVersion.state.flushGate = deferred();
  await fastUnlock(pendingVersion); pendingVersion.exports.lockCrypto();
  pendingVersion.state.flushGate.resolve(); await tick();
  assert.equal(JSON.parse(pendingVersion.storage.get(pendingVersion.key('a'))).deviceListsVersion, undefined, 'locking before acknowledgement cannot persist the directory version');
  const replacement = fixture(); replacement.storage.set(replacement.key('a'), JSON.stringify({ deviceId: 'local-device', cursor: 'old', deviceListsVersion: '99' }));
  replacement.state.syncResponse = response('100'); await replacement.exports.unlockCrypto('a', 'correct', 'new-device');
  assert.ok(!replacement.calls.filter(p => p.startsWith('/crypto/sync?')).at(-1).includes('deviceListsVersion='), 'replacement device starts without a directory acknowledgement');
  console.log('PASS: trusted local unlock returns before deferred sync; registration, new-device, passphrase, offline and stale-session boundaries remain enforced');
})().catch(error => { console.error(error); process.exitCode = 1; });
