const assert = require('node:assert/strict'), fs = require('node:fs'), vm = require('node:vm'), ts = require('typescript');
class HttpError extends Error { constructor(status) { super('denied'); this.status = status; } }
class ConnectionError extends Error {}
const api = { server: 'https://session.invalid', HttpError, ConnectionError };
function harness(error) {
  const exports = {}, effects = [];
  api.request = async () => { throw error; };
  const code = ts.transpileModule(fs.readFileSync(require.resolve('../src/cloud/crypto/engine.ts'), 'utf8'), { compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 } }).outputText;
  vm.runInNewContext(code + '\nexports.seed = () => { active = { account:"human", owner:ownerOf("human"), deviceId:"device", cursor:"0", machine:{ close(){ effects.push("closed") }, outgoingRequests(){ effects.push("flushed"); return [] } } }; };', { exports, effects, Event, window: { dispatchEvent() { effects.push('changed'); } }, require: name => name === '../api' ? api : {} });
  exports.seed();
  return { exports, effects };
}
(async () => {
  for (const status of [401, 403]) {
    const h = harness(new HttpError(status));
    await assert.rejects(h.exports.syncCrypto(), error => error.status === status);
    assert.equal(h.exports.cryptoDevice('human'), undefined, 'known revoked/unauthorized device must lock');
    assert.deepEqual(h.effects, ['closed', 'changed'], 'permission check precedes outgoing delivery');
  }
  const offline = harness(new ConnectionError('offline'));
  await assert.rejects(offline.exports.syncCrypto(), ConnectionError);
  assert.equal(offline.exports.cryptoDevice('human'), 'device', 'network loss retains an existing authorized local session');
  console.log('PASS: crypto authorization failure locks before queued delivery while network loss retains the existing offline session');
})().catch(error => { console.error(error); process.exitCode = 1; });
