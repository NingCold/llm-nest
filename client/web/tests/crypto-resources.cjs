const assert = require('node:assert/strict'), fs = require('node:fs'), vm = require('node:vm'), ts = require('typescript'), { indexedDB } = require('fake-indexeddb');
class HttpError extends Error { constructor(status) { super(`HTTP ${status}`); this.status = status; } }
class ConnectionError extends Error {}
let mode = 'offline', encryptions = 0, puts = [], cloud = [], cipherContent = new Map(), prepared = [];
const engine = { cryptoDevice: () => 'device', matrixUser: id => `@${id}:llmn`, syncCrypto: async () => { if (mode === 'offline') throw new ConnectionError('offline'); }, seal: async (room, users, type, content, offline) => { prepared.push({ room, offline }); const cipher = `frozen-${++encryptions}`; cipherContent.set(cipher, content); return { device_id: 'device', cipher }; }, prepareEncryption: async (room, users, offline) => { prepared.push({ prepare: true, room, offline }); }, signEnvelope: async envelope => ({ ...envelope, deviceKeys: { ed25519: "ed", curve25519: "curve" }, signatures: {} }), unseal: async envelope => ({ type: 'com.llmn.content.v1', content: cipherContent.get(envelope.content.cipher) }) };
const api = { server: 'https://resources.invalid', HttpError, ConnectionError, request: async (path, method, body) => {
  if (mode === 'offline') throw new ConnectionError('offline');
  if (mode === 'forbidden') throw new HttpError(403);
  if (method === 'PUT') { puts.push(structuredClone(body)); if (mode === 'conflict') throw new HttpError(409); const result = { id: path.split('/').pop(), ...body, revision: String(BigInt(body.expectedRevision) + 1n), applied: null }; cloud = [result]; return result; }
  if (path.startsWith('/crypto/resource-participants')) return { accounts: ['human', 'ai'], revision: 'audience-1' };
  return { items: cloud };
} };
function load() { const result = {}; vm.runInNewContext(ts.transpileModule(fs.readFileSync(require.resolve('../src/cloud/crypto/resources.ts'), 'utf8'), { compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 } }).outputText, { exports: result, indexedDB, crypto: require('node:crypto').webcrypto, require: name => name === '../api' ? api : engine }); return result; }
(async () => {
  let resources = load();
  const target = { id: 'item', aiId: 'ai', kind: 'profile', conversationId: null, expectedRevision: '0', expectedAudienceRevision: 'audience-1', users: ['human', 'ai'] };
  await resources.saveResource('human', target, { persona: 'private draft' });
  const pending = await resources.pendingResources('human');
  assert.equal(pending.length, 1); assert.equal(prepared[0].offline, true);
  assert.equal(JSON.stringify(pending[0].body).includes('private draft'), false);
  const frozen = JSON.stringify(pending[0].body);
  resources = load();
  assert.equal(JSON.stringify((await resources.pendingResources('human'))[0].body), frozen, 'reload retains the exact ciphertext/request command');
  await assert.rejects(resources.saveResource('human', target, { persona: 'replacement' }), /尚有/);
  mode = 'online'; await resources.synchronizeResources('human');
  assert.equal(JSON.stringify(puts[0]), frozen); assert.equal(encryptions, 1);
  const loaded = await resources.loadResources('human', 'ai', 'profile', null);
  assert.equal((await resources.decryptResource(loaded.items[0])).persona, 'private draft');
  mode = 'offline'; assert.equal((await resources.loadResources('human', 'ai', 'profile', null)).offline, true);
  mode = 'forbidden'; await assert.rejects(resources.loadResources('human', 'ai', 'profile', null), error => error.status === 403, 'revoked permission never falls back to cached plaintext');
  mode = 'conflict'; await resources.saveResource('human', { ...target, expectedRevision: '1' }, { persona: 'preserved conflicting edit' });
  const conflict = (await resources.pendingResources('human'))[0]; assert.ok(conflict.conflict);
  assert.ok(prepared.some(p => p.prepare && p.offline === false), 'online saves prepare sessions instead of always pretending to be offline');
  const count = puts.length; await resources.synchronizeResources('human'); assert.equal(puts.length, count, 'conflicts are not silently overwritten or automatically retried');
  assert.equal((await resources.decryptResource({ ...conflict.body, id: conflict.id, revision: '2', applied: null })).persona, 'preserved conflicting edit');
  assert.match(resources.resourceStatus({ revision: '2', tombstone: null, applied: { revision: '1', status: 'applied' } }), /待节点应用/);
  const sign = engine.signEnvelope;
  engine.signEnvelope = async envelope => { const value = await sign(envelope); engine.cryptoDevice = () => undefined; return value; };
  mode = 'online';
  await assert.rejects(resources.saveResource('human', { ...target, id: 'switched-account' }, { persona: 'must not persist' }), /账号已切换/);
  assert.equal((await resources.pendingResources('human')).some(p => p.id === 'switched-account'), false, 'a late signing result cannot persist an intent after account lock');
  console.log('PASS: encrypted resource intents persist across reload, retry original ciphertext, preserve conflicts and separate saved/applied states');
})().catch(e => { console.error(e); process.exitCode = 1; });
