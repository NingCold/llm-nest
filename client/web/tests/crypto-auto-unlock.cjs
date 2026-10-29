const assert = require('node:assert/strict'), fs = require('node:fs'), vm = require('node:vm'), ts = require('typescript');
const { webcrypto } = require('node:crypto');
const { IDBFactory } = require('fake-indexeddb');
const source = ts.transpileModule(fs.readFileSync(require.resolve('../src/cloud/crypto/auto-unlock.ts'), 'utf8'), { compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 } }).outputText;
class ConnectionError extends Error {}
class HttpError extends Error {}
class CryptoSessionChanged extends Error {}
function fixture(desktop = false) {
  const state = { epoch: 0, now: 1800000000000, account: 'a', active: 'device-a', trusted: true, confirmed: true, calls: [], storage: new Map(), devices: new Map([['a', 'device-a'], ['b', 'device-b']]), native: new Map() };
  const api = { server: 'https://auto.invalid', ConnectionError, HttpError };
  const indexedDB = new IDBFactory();
  const key = device => JSON.stringify([api.server, state.account, device]);
  const engine = {
    CryptoSessionChanged,
    cryptoGeneration: () => state.epoch,
    localCryptoDevice: account => state.devices.get(account),
    cryptoDevice: account => state.account === account ? state.active : undefined,
    deviceStatus: async () => ({ trusted: state.trusted }),
    recoveryConfirmed: () => state.confirmed,
    verifyCryptoPassphrase: async (_, passphrase) => { if (passphrase !== 'fixture-pass') throw new Error('SDK diagnostic contains ' + passphrase); },
    lockCrypto: () => { state.epoch++; state.active = undefined; },
    unlockCrypto: async (account, passphrase, create, expected) => {
      state.calls.push({ account, create, expected });
      if (create !== undefined) assert.equal(create, false, 'automatic unlock must never create or restore');
      if (expected) { assert.equal(expected.generation, state.epoch); assert.equal(expected.deviceId, state.devices.get(account)); }
      if (state.opening) { await state.opening.promise; if (expected?.isCurrent?.() === false) throw new CryptoSessionChanged('cancelled'); }
      if (state.fail) throw state.fail;
      if (passphrase !== 'fixture-pass') throw new Error('SDK diagnostic contains ' + passphrase);
      state.epoch++;
      state.account = account;
      state.active = state.devices.get(account);
      return { trusted: state.trusted };
    },
  };
  async function invoke(command, args) {
    state.commands ??= [];
    state.commands.push(command);
    if (command === 'cloud_unlock_support') return state.support ?? true;
    assert.equal(args.expectedServer, api.server);
    assert.equal(args.expectedAccountId, state.account);
    if (command === 'cloud_unlock_save') { const material = { secret: args.secret, expiresAt: state.now + 30 * 86400000 }; state.native.set(key(args.deviceId), material); return { expiresAt: material.expiresAt }; }
    if (command === 'cloud_unlock_load') { if (state.load) return state.load(args.deviceId); return state.native.get(key(args.deviceId)) ?? null; }
    if (command === 'cloud_unlock_remove') { state.native.delete(key(args.deviceId)); return; }
    throw new Error('unexpected IPC');
  }
  function restart() {
    const exports = {};
    vm.runInNewContext(source, { exports, indexedDB, crypto: webcrypto, TextEncoder, TextDecoder, Event, Date: class extends Date { static now() { return state.now; } }, window: { dispatchEvent() {} }, localStorage: { getItem: name => state.storage.get(name) ?? null, setItem: (name, value) => state.storage.set(name, value), removeItem: name => state.storage.delete(name) }, require: name => ({ '@tauri-apps/api/core': { invoke }, '@/lib/desktop': { IS_DESKTOP: desktop }, '../api': api, './engine': engine })[name] });
    return exports;
  }
  return { state, api, indexedDB, engine, restart, exports: restart() };
}
async function savedRecords(indexedDB) {
  const db = await new Promise((resolve, reject) => { const open = indexedDB.open('llmn-crypto-auto-unlock'); open.onsuccess = () => resolve(open.result); open.onerror = () => reject(open.error); });
  const records = await new Promise((resolve, reject) => { const req = db.transaction('materials').objectStore('materials').getAll(); req.onsuccess = () => resolve(req.result); req.onerror = () => reject(req.error); });
  db.close(); return records;
}
async function web() {
  const h = fixture(), a = h.exports;
  assert.equal((await a.autoUnlockInfo('a')).enabled, false);
  assert.equal(await a.tryAutoUnlock('a'), false);
  assert.equal(h.state.calls.length, 0);
  await assert.rejects(a.enableAutoUnlock('a', 'wrong-super-secret'), error => !error.message.includes('wrong-super-secret'));
  assert.equal(h.state.active, 'device-a', 'verifying a mistaken passphrase must not close the active engine');
  h.state.trusted = false;
  await assert.rejects(a.enableAutoUnlock('a', 'fixture-pass'));
  h.state.trusted = true; h.state.confirmed = false;
  await assert.rejects(a.enableAutoUnlock('a', 'fixture-pass'));
  h.state.confirmed = true;
  await a.enableAutoUnlock('a', 'fixture-pass');
  const info = await a.autoUnlockInfo('a'), records = await savedRecords(h.indexedDB);
  assert.equal(info.expiresAt, h.state.now + 30 * 86400000);
  assert.equal(records.length, 1);
  assert.equal(records[0].key.extractable, false);
  assert.equal(records[0].secret, undefined);
  assert.equal(JSON.stringify([...h.state.storage]).includes('fixture-pass'), false, 'no passphrase in localStorage');
  await assert.rejects(webcrypto.subtle.exportKey('raw', records[0].key));
  a.pauseAutoUnlock('a'); h.engine.lockCrypto();
  const reopened = h.restart();
  assert.equal((await reopened.autoUnlockInfo('a')).paused, true);
  assert.equal(await reopened.tryAutoUnlock('a'), false, 'manual lock survives reload');
  await assert.rejects(reopened.unlockManually('a', 'wrong-super-secret'), error => !error.message.includes('wrong-super-secret'));
  assert.equal((await reopened.autoUnlockInfo('a')).paused, true);
  await reopened.unlockManually('a', 'fixture-pass');
  assert.equal((await reopened.autoUnlockInfo('a')).paused, false);
  h.engine.lockCrypto(); h.state.now += 1000;
  assert.equal(await h.restart().tryAutoUnlock('a'), true, 'a fresh module can unwrap the persisted WebCrypto key');
  assert.equal((await reopened.autoUnlockInfo('a')).expiresAt, info.expiresAt, 'unlock never renews the expiration');
  assert.equal(await reopened.tryAutoUnlock('b'), false, 'account scope is isolated');
  h.api.server = 'https://other.invalid';
  assert.equal(await reopened.tryAutoUnlock('a'), false, 'server scope is isolated');
  h.api.server = 'https://auto.invalid'; h.state.devices.set('a', 'replacement');
  assert.equal(await reopened.tryAutoUnlock('a'), false, 'device scope is isolated');
  h.state.devices.set('a', 'device-a');
  h.state.now = info.expiresAt;
  await assert.rejects(reopened.tryAutoUnlock('a'), /已过期/);
  assert.equal((await reopened.autoUnlockInfo('a')).enabled, false);
  h.state.active = 'device-a';
  await reopened.enableAutoUnlock('a', 'fixture-pass');
  await reopened.disableAutoUnlock('a');
  assert.equal(await reopened.tryAutoUnlock('a'), false);
  await reopened.enableAutoUnlock('a', 'fixture-pass');
  h.state.fail = new ConnectionError('offline fixture-pass');
  await assert.rejects(reopened.tryAutoUnlock('a'), error => /联网/.test(error.message) && !error.message.includes('fixture-pass'));
  assert.equal((await reopened.autoUnlockInfo('a')).enabled, true, 'temporary network failure does not discard valid local material');
  h.state.fail = new Error('invalid SDK secret fixture-pass');
  await assert.rejects(reopened.tryAutoUnlock('a'), error => !error.message.includes('fixture-pass'));
  assert.equal((await reopened.autoUnlockInfo('a')).enabled, false, 'bad material is removed');
  delete h.state.fail;
  await reopened.enableAutoUnlock('a', 'fixture-pass');
  h.state.trusted = false;
  await assert.rejects(reopened.tryAutoUnlock('a'), /权限/);
  assert.equal(h.state.active, undefined);
  assert.equal((await reopened.autoUnlockInfo('a')).enabled, false, 'revoked/untrusted device disables convenience material');
  h.state.trusted = true; h.state.active = 'device-a';
  await reopened.enableAutoUnlock('a', 'fixture-pass');
  reopened.pauseAutoUnlock('a');
  await reopened.forgetAutoUnlock('a');
  assert.equal((await reopened.autoUnlockInfo('a')).enabled, false);
  assert.equal((await reopened.autoUnlockInfo('a')).paused, false);
  assert.equal(h.state.devices.get('a'), 'device-a', 'forgetting never removes the SDK device metadata');
  await reopened.enableAutoUnlock('a', 'fixture-pass');
  h.state.devices.set('a', 'replacement'); h.state.active = 'replacement';
  await reopened.enableAutoUnlock('a', 'fixture-pass');
  assert.equal((await savedRecords(h.indexedDB)).length, 2);
  h.state.devices.delete('a');
  await reopened.forgetAutoUnlock('a');
  assert.equal((await savedRecords(h.indexedDB)).length, 0, 'logout clears older device material even if current SDK metadata disappeared');
}
async function native() {
  const h = fixture(true), a = h.exports;
  await a.enableAutoUnlock('a', 'fixture-pass');
  assert.ok(h.state.commands.includes('cloud_unlock_save'));
  assert.equal((await a.autoUnlockInfo('a')).expiresAt, h.state.now + 30 * 86400000);
  let finishSupport;
  h.state.support = new Promise(resolve => { finishSupport = resolve; });
  const early = a.tryAutoUnlock('a');
  a.pauseAutoUnlock('a'); h.engine.lockCrypto(); finishSupport(true);
  assert.equal(await early, false, 'manual lock while support detection is pending also invalidates the attempt');
  delete h.state.support;
  await a.unlockManually('a', 'fixture-pass');
  h.state.calls.length = 0;
  let finishLoad;
  h.state.load = () => new Promise(resolve => { finishLoad = resolve; });
  const pending = a.tryAutoUnlock('a');
  while (!finishLoad) await new Promise(resolve => setImmediate(resolve));
  h.engine.lockCrypto();
  finishLoad({ secret: 'fixture-pass', expiresAt: h.state.now + 30 * 86400000 });
  assert.equal(await pending, false, 'manual lock during vault read prevents a late engine reopen');
  assert.equal(h.state.calls.length, 0);
  delete h.state.load;
  h.state.fail = new CryptoSessionChanged('switched');
  assert.equal(await a.tryAutoUnlock('a'), false);
  assert.equal((await a.autoUnlockInfo('a')).enabled, true, 'account-switch cancellation retains its owner setting');
  delete h.state.fail;
  h.state.opening = {};
  h.state.opening.promise = new Promise(resolve => { h.state.opening.resolve = resolve; });
  const earlierCalls = h.state.calls.length;
  const lateOpening = a.tryAutoUnlock('a');
  while (h.state.calls.length === earlierCalls) await new Promise(resolve => setImmediate(resolve));
  await a.disableAutoUnlock('a');
  h.state.opening.resolve();
  assert.equal(await lateOpening, false, 'disabling while SDK opening cancels its final commit');
  delete h.state.opening;
  h.state.active = 'device-a';
  await a.enableAutoUnlock('a', 'fixture-pass');
  await a.forgetAutoUnlock('a');
  assert.equal(h.state.native.size, 0);
}
async function sdkVerification() {
  require('fake-indexeddb/auto');
  const sdk = require('@matrix-org/matrix-sdk-crypto-wasm');
  await sdk.initAsync();
  const api = { server: 'https://verification.invalid' }, account = 'fixture', deviceId = 'fixture-device', passphrase = 'sdk-fixture-pass';
  const owner = JSON.stringify([api.server, account]);
  const digest = await webcrypto.subtle.digest('SHA-256', new TextEncoder().encode(JSON.stringify([owner, deviceId])));
  const name = `llmn-crypto-${Buffer.from(digest).toString('hex')}`;
  const machine = await sdk.OlmMachine.initialize(new sdk.UserId('@fixture:llmn'), new sdk.DeviceId(deviceId), name, passphrase);
  const fingerprint = machine.identityKeys.ed25519.toBase64(), exports = {};
  let selectedSdk = sdk;
  const code = ts.transpileModule(fs.readFileSync(require.resolve('../src/cloud/crypto/engine.ts'), 'utf8'), { compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 } }).outputText;
  vm.runInNewContext(code + '\nexports.seed = value => { active = value; };', { exports, crypto: webcrypto, TextEncoder, indexedDB: global.indexedDB, localStorage: { getItem: () => JSON.stringify({ deviceId, cursor: '0' }) }, require: name => name === '../api' ? api : name === '@matrix-org/matrix-sdk-crypto-wasm' ? selectedSdk : {}, Event, window: { dispatchEvent() {} } });
  exports.seed({ sdk, machine, account, deviceId, owner });
  await exports.verifyCryptoPassphrase(account, passphrase);
  await assert.rejects(exports.verifyCryptoPassphrase(account, 'wrong-passphrase'));
  assert.equal(exports.cryptoDevice(account), deviceId);
  assert.equal(machine.identityKeys.ed25519.toBase64(), fingerprint, 'validating while open preserves the SDK machine identity');
  const missing = 'missing-device';
  await assert.rejects(exports.unlockCrypto(account, passphrase, false, { generation: exports.cryptoGeneration(), deviceId: missing }), /变化/);
  exports.lockCrypto();
  let current = true, finish, entered = false, closed = 0;
  selectedSdk = { ...sdk, OlmMachine: { initialize: async () => { entered = true; await new Promise(resolve => { finish = resolve; }); return { close() { closed++; } }; } } };
  const opening = exports.unlockCrypto(account, passphrase, false, { generation: exports.cryptoGeneration(), deviceId, isCurrent: () => current });
  while (!entered) await new Promise(resolve => setImmediate(resolve));
  current = false; finish();
  await assert.rejects(opening, exports.CryptoSessionChanged);
  assert.equal(closed, 1, 'cancelled initialization closes its machine before it can become active');
  assert.equal(exports.cryptoDevice(account), undefined);
  let initialized = 0, sdkReady, sdkEntered = false;
  current = true;
  selectedSdk = { ...sdk, initAsync: async () => { sdkEntered = true; await new Promise(resolve => { sdkReady = resolve; }); }, OlmMachine: { initialize: async () => { initialized++; throw new Error('must not initialize a stale scope'); } } };
  const loadingSdk = exports.unlockCrypto(account, passphrase, false, { generation: exports.cryptoGeneration(), deviceId, isCurrent: () => current });
  while (!sdkEntered) await new Promise(resolve => setImmediate(resolve));
  current = false; sdkReady();
  await assert.rejects(loadingSdk, exports.CryptoSessionChanged);
  assert.equal(initialized, 0, 'a scope invalidated while loading WASM cannot create or open any SDK machine');
  await new Promise((resolve, reject) => { const req = indexedDB.deleteDatabase(`${name}::matrix-sdk-crypto`); req.onsuccess = resolve; req.onerror = reject; });
  await assert.rejects(exports.unlockCrypto(account, passphrase, false, { generation: exports.cryptoGeneration(), deviceId }), /不存在/);
  assert.equal((await indexedDB.databases()).some(db => db.name === `${name}::matrix-sdk-crypto`), false, 'automatic opening cannot recreate a missing SDK database');
}
(async () => { await web(); await native(); await sdkVerification(); console.log('PASS: optional auto unlock uses scoped non-exportable browser wrapping/native vault, fixed expiry, durable pause, manual resume, cancellation, and existing SDK identity only'); })().catch(error => { console.error(error); process.exitCode = 1; });
