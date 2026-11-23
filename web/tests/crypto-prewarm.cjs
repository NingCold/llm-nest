const assert = require('node:assert/strict'), fs = require('node:fs'), vm = require('node:vm'), ts = require('typescript');
const { webcrypto } = require('node:crypto');
const { performance } = require('node:perf_hooks');
const { IDBFactory } = require('fake-indexeddb');
const code = ts.transpileModule(fs.readFileSync(require.resolve('../src/cloud/crypto/engine.ts'), 'utf8'), { compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 } }).outputText;
const tick = () => new Promise(resolve => setImmediate(resolve));
const deferred = () => { let resolve; const promise = new Promise(yes => { resolve = yes; }); return { promise, resolve }; };
const requestDone = request => new Promise((resolve, reject) => { request.onsuccess = () => resolve(request.result); request.onerror = () => reject(request.error); });
async function until(test) {
  // WebCrypto runs off-thread. Event loop turns are not a unit of elapsed time.
  const deadline = performance.now() + 5000;
  while (!test()) {
    assert.ok(performance.now() < deadline, `Timed out waiting for ${test}`);
    await new Promise(resolve => setTimeout(resolve, 5));
  }
}
function fixture({ known = true } = {}) {
  const factory = new IDBFactory(), storage = new Map(), measurements = [];
  const api = { server: 'https://warm.invalid', request: () => assert.fail('prewarming must not contact the server') };
  const state = { imports: 0, inits: 0, enumerations: 0, opens: [], connections: [], closed: 0 };
  const sdk = { initAsync: async () => { state.inits++; if (state.initGate) await state.initGate.promise; if (state.initError) throw state.initError; }, StoreHandle: { open: () => assert.fail('prewarming must not unlock a store') }, OlmMachine: { initFromStore: () => assert.fail('prewarming must not load an identity') } };
  const indexedDB = {
    async databases() {
      state.enumerations++; if (state.enumerateGate) await state.enumerateGate.promise;
      if (state.enumerateError) throw state.enumerateError;
      const names = await factory.databases(); return state.listHook ? state.listHook(names) : names;
    },
    open(...args) {
      assert.equal(args.length, 1, 'prewarm does not specify or change the database version');
      state.opens.push(args[0]);
      const actual = factory.open(...args);
      const request = { get result() { return actual.result; }, get error() { return actual.error; }, get transaction() { return actual.transaction; } };
      actual.onupgradeneeded = event => request.onupgradeneeded?.(event);
      actual.onerror = event => request.onerror?.(event);
      actual.onsuccess = async event => {
        const db = actual.result, close = db.close.bind(db);
        state.connections.push(db);
        db.transaction = () => assert.fail('prewarm must not read cipher or key records');
        db.close = () => { state.closed++; close(); };
        if (state.successGate) await state.successGate.promise;
        request.onsuccess?.(event);
      };
      return request;
    },
  };
  const exports = {};
  const key = account => `llmn-crypto-device:${JSON.stringify([api.server, account])}`;
  if (known) storage.set(key('a'), JSON.stringify({ deviceId: 'device-a', cursor: '0' }));
  vm.runInNewContext(code, { exports, crypto: webcrypto, TextEncoder, indexedDB, localStorage: { getItem: key => storage.get(key) ?? null, setItem: (key, value) => storage.set(key, value) }, performance: { now: () => performance.now(), clearMeasures(name) { for (let i = measurements.length - 1; i >= 0; i--) if (measurements[i].name === name) measurements.splice(i, 1); }, measure(name, options) { measurements.push({ name, ...options }); } }, require(name) { if (name === '../api') return api; if (name === '@matrix-org/matrix-sdk-crypto-wasm') { state.imports++; return sdk; } return {}; } });
  async function seed(account = 'a', deviceId = 'device-a', version = 1) {
    storage.set(key(account), JSON.stringify({ deviceId, cursor: '0' }));
    const digest = await webcrypto.subtle.digest('SHA-256', new TextEncoder().encode(JSON.stringify([JSON.stringify([api.server, account]), deviceId])));
    const name = `llmn-crypto-${Buffer.from(digest).toString('hex')}::matrix-sdk-crypto-meta`;
    const opening = factory.open(name, version);
    opening.onupgradeneeded = () => opening.result.createObjectStore('matrix-sdk-crypto');
    (await requestDone(opening)).close(); return name;
  }
  return { exports, api, state, storage, key, factory, measurements, seed };
}
(async () => {
  const warm = fixture(), name = await warm.seed(); await warm.seed('other-account', 'other-device');
  warm.state.initGate = deferred(); warm.state.enumerateGate = deferred();
  const cancel = warm.exports.prewarmCrypto('a');
  assert.equal(typeof cancel, 'function', 'prewarm returns cleanup immediately without blocking the lock screen');
  await until(() => warm.state.inits === 1 && warm.state.enumerations === 1);
  assert.equal(warm.measurements.length, 0, 'SDK and IndexedDB preparation are independent');
  warm.state.enumerateGate.resolve(); await until(() => warm.measurements.some(m => m.name.endsWith('holdReady')));
  assert.deepEqual(warm.state.opens, [name], 'only the current account metadata database is held');
  await new Promise(resolve => setTimeout(resolve, 2100));
  assert.equal(warm.state.closed, 0, 'the empty connection remains held beyond the Chromium two-second grace period');
  assert.equal(warm.exports.cryptoDevice('a'), undefined, 'prewarming never activates a crypto identity');
  warm.state.initGate.resolve(); await until(() => warm.measurements.some(m => m.name.endsWith('sdkReady')));
  cancel(); assert.equal(warm.state.closed, 1, 'unlock/session cleanup releases the held connection');
  assert.ok(warm.measurements.every(m => /^llmn\.crypto\.prewarm\.(sdkReady|indexedDbReady|holdReady|failed)$/.test(m.name)));

  const fresh = fixture({ known: false }); fresh.exports.prewarmCrypto('new-account')(); await tick();
  assert.equal(fresh.state.inits, 0); assert.equal(fresh.state.enumerations, 0);
  const absent = fixture(); absent.exports.prewarmCrypto('a');
  await until(() => absent.measurements.some(m => m.name.endsWith('indexedDbReady')));
  assert.equal(absent.state.opens.length, 0); assert.equal((await absent.factory.databases()).length, 0, 'a stale local device marker cannot create a missing database');
  const damaged = fixture(); damaged.storage.set(damaged.key('a'), '{damaged metadata');
  assert.doesNotThrow(() => damaged.exports.prewarmCrypto('a'), 'metadata parsing is asynchronous and cannot crash the lock-screen effect');
  await until(() => damaged.measurements.some(m => m.name.endsWith('failed')));

  const race = fixture(), deleted = await race.seed();
  race.state.listHook = async names => { await requestDone(race.factory.deleteDatabase(deleted)); return names; };
  race.exports.prewarmCrypto('a'); await until(() => race.measurements.some(m => m.name.endsWith('failed')));
  assert.equal((await race.factory.databases()).length, 0, 'upgrade abort prevents recreating a store deleted after enumeration');

  for (const change of ['cancel', 'server']) {
    const late = fixture(); await late.seed(); late.state.successGate = deferred();
    const stop = late.exports.prewarmCrypto('a'); await until(() => late.state.connections.length === 1);
    if (change === 'cancel') stop(); else late.api.server = 'https://replacement.invalid';
    late.state.successGate.resolve(); await until(() => late.state.closed === 1);
    assert.equal(late.measurements.some(m => m.name.endsWith('holdReady')), false, 'a cancelled or wrong-server late connection is closed without retaining timings');
  }
  const cancelled = fixture(); await cancelled.seed(); cancelled.state.enumerateGate = deferred();
  const stop = cancelled.exports.prewarmCrypto('a'); await until(() => cancelled.state.enumerations === 1);
  stop(); cancelled.state.enumerateGate.resolve(); await tick(); await tick();
  assert.equal(cancelled.state.opens.length, 0, 'cleanup before enumeration completes prevents opening a connection');

  const version = fixture(), versionedName = await version.seed('a', 'device-a', 2);
  version.exports.prewarmCrypto('a'); await until(() => version.measurements.some(m => m.name.endsWith('holdReady')));
  await requestDone(version.factory.deleteDatabase(versionedName));
  assert.equal(version.state.closed, 1, 'versionchange releases the hold so schema changes or deletion are not blocked');

  for (const stage of ['init', 'enumerate']) {
    const failed = fixture(); failed.state[`${stage}Error`] = new Error('private SDK diagnostic');
    failed.exports.prewarmCrypto('a'); await until(() => failed.measurements.some(m => m.name.endsWith('failed')));
    assert.equal(JSON.stringify(failed.measurements).includes('private SDK diagnostic'), false);
    assert.equal(failed.exports.cryptoDevice('a'), undefined);
    if (stage === 'init') {
      await assert.rejects(failed.exports.unlockCrypto('a', 'passphrase'), /private SDK diagnostic/, 'real unlock exposes its initialization error');
      assert.equal(failed.state.inits, 2, 'prewarming adds no initialization cache');
    }
  }
  console.log('PASS: locked metadata connection survives idle, reads no records, never creates a store, closes on cleanup/versionchange and preserves unlock errors');
})().catch(error => { console.error(error); process.exitCode = 1; });
