const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const path = require('node:path');
const ts = require('../../web/node_modules/typescript');
const random = require('node:crypto').webcrypto;
function load(file, imports, globals = {}) {
  const exports = {};
  vm.runInNewContext(ts.transpileModule(fs.readFileSync(path.resolve(__dirname, file), 'utf8'), { compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 } }).outputText, {
    exports, TextEncoder, crypto: random, ...globals,
    require(name) { if (name in imports) return imports[name]; throw Error(name); },
  });
  return exports;
}
(async () => {
  const sent = [], window = { NativeCrypto: { context: () => JSON.stringify({ server: 'https://fixture.invalid', account: 'self' }), request: (...args) => sent.push(args) } };
  const api = load('api.ts', {}, { window });
  let settlements = 0;
  const first = api.request('/crypto/backup').then(() => settlements++);
  window.cryptoReply(sent[0][0], { value: { version: '1' } });
  window.cryptoReply(sent[0][0], { error: { status: 403, message: 'late' } });
  await first; assert.equal(settlements, 1, 'bridge promises settle once');
  const failure = api.request('/crypto/recovery');
  window.cryptoReply(sent[1][0], { error: { status: 404, message: 'missing', code: 'not_found' } });
  await assert.rejects(failure, e => e instanceof api.HttpError && e.status === 404 && e.code === 'not_found');
  const invalid = api.request('/crypto/sync');
  window.cryptoReply(sent[2][0], { error: { status: 0, message: 'invalid', code: 'invalid_response' } });
  await assert.rejects(invalid, e => e instanceof api.HttpError && !(e instanceof api.ConnectionError));
  const offline = api.request('/crypto/sync');
  window.cryptoReply(sent[3][0], { error: { status: 0, message: 'offline', code: 'connection_error' } });
  await assert.rejects(offline, e => e instanceof api.ConnectionError && e.code === 'connection_error');

  let unlocked = false, calls = 0;
  const engine = {
    cryptoDevice: () => unlocked ? 'local' : undefined,
    hasCryptoDevice: () => true, recoveryConfirmed: () => true,
    deviceStatus: async () => ({ deviceId: 'local', trusted: false }),
    matrixRoom: id => `!${id}:llmn`, matrixUser: id => `@${id}:llmn`,
    unseal: async () => { if (!unlocked) throw Error('locked'); return { type: 'com.llmn.content.v1', content: payload }; },
    seal: async () => ({ device_id: 'local', ciphertext: 'synthetic-test-ciphertext' }),
    signEnvelope: async base => base,
    unlockCrypto: async (account, passphrase, create) => { calls++; assert.equal(account, 'self'); assert.equal(create, false); unlocked = true; },
    syncCrypto: async () => {},
  };
  const cache = new Map();
  const content = load('../../web/src/cloud/crypto/content.ts', { './engine': engine, '../api': { server: 'https://fixture.invalid' } }, { localStorage: { getItem: key => cache.get(key) ?? null, setItem: (key, value) => cache.set(key, value) } });
  const listeners = {};
  const ops = load('operations.ts', { '../../web/src/cloud/crypto/engine': engine, '../../web/src/cloud/crypto/content': content, './api': { context: { account: 'self' } } }, { window: { addEventListener: (name, fn) => { listeners[name] = fn; } } });
  const conversation = { id: 'room', revision: '7', encryption: { version: 1, activationPosition: '0' }, members: [{ account: { id: 'self' } }], lastMessage: null };
  const message = { id: 'message', conversationId: 'room', senderId: 'self', revision: '1', position: '1', status: 'committed', text: 'injected', mentions: [], replyTo: null, attachments: [], sealed: { version: 1, roomId: '!room:llmn', sender: '@self:llmn', deviceId: 'local', eventId: '$message.1.final', content: { device_id: 'local' } } };
  const payload = { version: 1, server: 'https://fixture.invalid', kind: 'message', conversationId: 'room', messageId: 'message', senderId: 'self', revision: '1', text: 'authenticated', mentions: [], replyTo: null };
  assert.equal((await ops.dispatch('state', {})).locked, true); assert.equal(calls, 0, 'reading state must not create/unlock a device');
  const locked = await ops.dispatch('projectPage', { conversation, page: { messages: [message] } });
  assert.equal(locked.messages[0].text, ''); assert.equal(locked.messages[0]._decrypted, undefined);
  await ops.dispatch('unlock', { passphrase: 'independent-passphrase' });
  assert.equal((await ops.dispatch('sync', {})).changed, false);
  listeners['llmn-crypto-change']();
  assert.equal((await ops.dispatch('sync', {})).changed, true);
  assert.equal((await ops.dispatch('sync', {})).changed, false);
  assert.equal((await ops.dispatch('state', {})).trusted, false, 'unlocked does not imply trusted');
  const projected = await ops.dispatch('projectPage', { conversation, page: { messages: [message] } });
  assert.equal(projected.messages[0].text, 'authenticated'); assert.equal(projected.messages[0]._decrypted, true);
  const downgraded = await ops.dispatch('projectPage', { conversation: { ...conversation, encryption: null }, page: { messages: [{ ...message, sealed: undefined }] } });
  assert.equal(downgraded.messages[0].text, ''); assert.match(downgraded.messages[0].cryptoError, /明文回退/);
  const command = await ops.dispatch('prepareSend', { conversation, text: 'private input', requestId: 'same-request' });
  assert.equal(command.text, ''); assert.equal(command.requestId, 'same-request'); assert.equal(command.expectedConversationRevision, '7');
  assert.equal(command.sealed.content.ciphertext, 'synthetic-test-ciphertext');
  assert.equal(JSON.stringify(command).includes('private input'), false, 'prepared outer command contains no plaintext');
  await assert.rejects(ops.dispatch('prepareSend', { conversation: { ...conversation, id: 'new', encryption: null }, text: 'private', requestId: 'r' }), /尚未开启加密/);
  await assert.rejects(ops.dispatch('projectSnapshot', { snapshot: { self: { id: 'other' }, conversations: [] } }), /账号已切换/);
  console.log('PASS: Android crypto bridge preserves HTTP errors/once settlement; shared projection refuses downgrade, binds account and marks decryption; prepared send is sealed-only');
})().catch(error => { console.error(error); process.exitCode = 1; });
