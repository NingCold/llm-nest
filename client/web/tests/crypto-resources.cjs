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
  const React = require('react');
  function panelHarness(kind, tombstone = 'stale', pending = []) {
    const resource = { id: 'settings', aiId: 'ai', conversationId: 'group', kind, revision: '9', tombstone, sources: [], applied: null };
    const states = [[{ resource, data: { enabled: true, binding: { providerId: 'old-provider', modelId: 'old-model', maxTokens: 99 } } }], pending];
    let index = 0; const saved = [], panel = {};
    const hooks = { ...React, useState(initial) { const slot = index++; if (!(slot in states)) states[slot] = typeof initial === 'function' ? initial() : initial; return [states[slot], value => { states[slot] = typeof value === 'function' ? value(states[slot]) : value; }]; }, useEffect() {}, useRef: () => ({ current: 0 }) };
    const fields = { enabled: 'on', provider: 'new-provider', model: 'new-model', effort: '', maxTokens: '321' };
    vm.runInNewContext(ts.transpileModule(fs.readFileSync(require.resolve('../src/cloud/crypto/ResourcePanel.tsx'), 'utf8'), { compilerOptions: { jsx: ts.JsxEmit.ReactJSX, module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 } }).outputText, { exports: panel, crypto: require('node:crypto').webcrypto, FormData: class { get(key) { return fields[key] ?? null; } has(key) { return key in fields; } }, localStorage: { setItem() {} }, require: name => name === 'react' ? hooks : name === './engine' ? { cryptoDevice: () => 'device' } : name === '../api' ? { server: api.server, HttpError, request: async () => ({ accounts: ['human', 'ai'], revision: 'current-audience' }) } : name === './resources' ? { loadResources: async () => ({ items: [resource], offline: false }), decryptResource: async () => null, pendingResources: async () => [], resourceStatus: () => 'stale', saveResource: async (...args) => saved.push(args) } : require(name) });
    return { saved, render() { index = 0; return panel.EncryptedResourcePanel({ operatorId: 'human', aiId: 'ai', kind, conversation: { id: 'group' } }); } };
  }
  const all = node => !node || typeof node !== 'object' ? [] : [node, ...React.Children.toArray(node.props?.children).flatMap(all)];
  const label = node => typeof node === 'string' ? node : React.Children.toArray(node?.props?.children).map(label).join('');
  const settingsPanel = panelHarness('summarySettings');
  let tree = settingsPanel.render();
  const restart = all(tree).find(n => n.type === 'button' && label(n) === '重新设置摘要');
  assert.ok(restart); assert.equal(restart.props.disabled, false); restart.props.onClick();
  tree = settingsPanel.render();
  const inputs = all(tree).filter(n => n.type === 'input');
  assert.equal(inputs.find(n => n.props.name === 'enabled').props.defaultChecked, false);
  for (const name of ['provider', 'model', 'maxTokens']) assert.equal(inputs.find(n => n.props.name === name).props.defaultValue, '', 'stale values are never restored');
  all(tree).find(n => n.type === 'form').props.onSubmit({ preventDefault() {}, currentTarget: {} });
  await new Promise(resolve => setImmediate(resolve));
  assert.equal(settingsPanel.saved.length, 1);
  assert.equal(settingsPanel.saved[0][1].id, 'settings');
  assert.equal(settingsPanel.saved[0][1].expectedRevision, '9');
  assert.equal(settingsPanel.saved[0][2].binding.modelId, 'new-model');
  for (const kind of ['memory', 'summary']) assert.equal(all(panelHarness(kind).render()).some(n => n.type === 'button' && ['重新设置摘要', '编辑当前版本'].includes(label(n))), false, 'memory/summary tombstones cannot be revived');
  assert.equal(all(panelHarness('summarySettings', 'forgotten').render()).some(n => label(n) === '重新设置摘要'), false);
  assert.equal(all(panelHarness('summarySettings', 'stale', [{ id: 'settings', body: { expectedRevision: '9' } }]).render()).find(n => n.type === 'button' && label(n) === '重新设置摘要').props.disabled, true, 'an unknown original save remains locked');
  console.log('PASS: encrypted resource intents persist across reload, retry original ciphertext, preserve conflicts and separate saved/applied states');
})().catch(e => { console.error(e); process.exitCode = 1; });
