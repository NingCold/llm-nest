const assert = require('node:assert/strict'), fs = require('node:fs'), vm = require('node:vm'), ts = require('typescript');
const history = {}, calls = [], projected = [];
let respond, project = async page => page;
vm.runInNewContext(ts.transpileModule(fs.readFileSync(require.resolve('../src/cloud/crypto/history.ts'), 'utf8'), { compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 } }).outputText, {
  exports: history, require(name) {
    if (name === '../api') return { request: async path => { calls.push(path); return respond(path); } };
    if (name === './content') return { projectPage: async page => { projected.push(page); return project(page); } };
    if (name === './attachments' || name === 'fflate') return {};
    throw Error(name);
  },
});
const deferred = () => { let resolve; const promise = new Promise(yes => resolve = yes); return { promise, resolve }; };
const tick = () => new Promise(resolve => setImmediate(resolve));
const message = (id, position, text = 'irrelevant', extra = {}) => ({ id, position: String(position), text, status: 'committed', createdAt: '2027-01-01', ...extra });
(async () => {
  const conversations = Array.from({ length: 8 }, (_, i) => ({ id: `room-${i}` }));
  let active = 0, maximum = 0;
  respond = async path => {
    active++; maximum = Math.max(maximum, active); await tick(); active--;
    const id = path.match(/conversations\/([^/]+)/)[1], page = path.includes('before=101') ? 0 : path.includes('before=201') ? 1 : 2;
    const messages = Array.from({ length: 100 }, (_, i) => message(`${id}-${page * 100 + i + 1}`, page * 100 + i + 1));
    messages[0].text = 'Needle';
    messages[1].cryptoError = 'missing key'; messages[1].text = 'needle';
    messages[2].status = 'withdrawn'; messages[2].text = 'needle';
    return { messages, hasMore: page > 0 };
  };
  const result = await history.searchHistory(conversations, 'NEEDLE', () => true);
  assert.equal(maximum, 3, 'at most three room page reads run concurrently');
  assert.equal(calls.length, 24); assert.equal(result.failed, 24); assert.equal(result.messages.length, 24);
  assert.deepEqual(Array.from(result.messages, m => m.id), conversations.flatMap(c => [1, 101, 201].map(p => `${c.id}-${p}`)), 'same-timestamp results preserve prior conversation and chronological tie ordering');
  assert.ok(calls.every(path => !path.includes('query=') && !path.includes('NEEDLE')), 'search words never leave the client');
  calls.length = 0;
  const complete = await history.completeHistory(conversations[0]);
  assert.equal(complete.length, 300); assert.equal(complete[0].position, '1'); assert.equal(complete[299].position, '300');

  // Cancellation before a page returns must prevent both decryption and follow-up reads.
  calls.length = projected.length = 0;
  const delayed = deferred(); let current = true;
  respond = () => delayed.promise;
  const cancelled = history.completeHistory(conversations[0], () => current);
  current = false; delayed.resolve({ messages: [message('stale', 200)], hasMore: true });
  await assert.rejects(cancelled, /取消/); assert.equal(calls.length, 1); assert.equal(projected.length, 0);
  calls.length = 0;
  await assert.rejects(history.completeHistory(conversations[0], () => false), /取消/); assert.equal(calls.length, 0);

  // A failed room stops queued rooms and pending peers at their next page boundary.
  calls.length = 0; const peers = deferred();
  respond = path => path.includes('/room-0/') ? Promise.reject(new Error('offline')) : peers.promise;
  await assert.rejects(history.searchHistory(conversations, 'needle', () => true), /offline/);
  peers.resolve({ messages: [message('peer', 200)], hasMore: true }); await tick();
  assert.equal(calls.length, 3);
  console.log('PASS: bounded private search filters pagewise, counts failures, preserves ordering and stops stale reads before decryption');
})().catch(error => { console.error(error); process.exitCode = 1; });
