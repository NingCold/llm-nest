const assert = require('node:assert/strict'), fs = require('node:fs'), vm = require('node:vm'), ts = require('typescript');
function load(file, require) {
  const exports = {};
  vm.runInNewContext(ts.transpileModule(fs.readFileSync(requirePath(file), 'utf8'), {
    compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022, jsx: ts.JsxEmit.ReactJSX },
  }).outputText, { exports, require, performance: timing, navigator: { clipboard: { writeText: async text => { if (copyFails) throw new Error('denied'); copied = text; } } } });
  return exports;
}
const requirePath = file => require.resolve('../src/cloud/' + file);
let reads = 0, copied, copyFails = false;
const entry = (name, extra = {}) => ({ name, entryType: 'measure', startTime: 10, duration: 2, ...extra });
const sourceEntries = [entry('llmn.crypto.unlock.unlockReady', { detail: { account: 'private' }, url: 'private' }),
  entry('llmn.crypto.prewarm.sdkReady'), entry('llmn.chat.startup.firstSnapshotFrame'),
  entry('llmn.crypto.unlock.private'), entry('llmn.crypto.unlock.unlockReady.private'),
  entry('llmn.crypto.unlock.unlockReady', { entryType: 'resource' }),
  entry('llmn.crypto.unlock.storeOpen', { duration: NaN }), entry('llmn.crypto.unlock.storeOpen', { startTime: Infinity }),
  entry('llmn.crypto.unlock.storeOpen', { duration: -1 }), entry('llmn.crypto.unlock.storeOpen', { startTime: -1 })];
const timing = { getEntriesByType(type) { assert.equal(type, 'measure'); reads++; return sourceEntries; } };
const helper = load('unlock-timings.ts', () => { throw new Error('unexpected import'); });
const selected = helper.selectUnlockTimings(sourceEntries);
assert.equal(selected.length, 3);
assert.deepEqual(JSON.parse(JSON.stringify(selected[0])), { name: 'llmn.crypto.unlock.unlockReady', startTime: 10, duration: 2 });
assert.equal(JSON.stringify(selected).includes('private'), false);
let index = 0;
const states = [];
const component = load('UnlockTimings.tsx', name => {
  if (name === './unlock-timings') return helper;
  if (name === 'react') return { useState(initial) { const i = index++; if (!(i in states)) states[i] = initial; return [states[i], value => { states[i] = value; }]; } };
  if (name === 'react/jsx-runtime') return { jsx: (type, props) => ({ type, props }), jsxs: (type, props) => ({ type, props }) };
  throw new Error('unexpected import ' + name);
});
const render = () => { index = 0; return component.UnlockTimings(); };
function find(node, type) {
  if (!node || typeof node !== 'object') return;
  if (node.type === type) return node;
  for (const child of [node.props?.children].flat(Infinity)) { const found = find(child, type); if (found) return found; }
}
(async () => {
  const first = render(); assert.equal(reads, 0, 'mount does not inspect performance');
  assert.equal(find(first, 'button'), undefined, 'empty timings cannot be copied');
  first.props.onToggle({ currentTarget: { open: false } }); assert.equal(reads, 0);
  first.props.onToggle({ currentTarget: { open: true } }); assert.equal(reads, 1);
  const content = find(render(), 'div');
  assert.equal(content.props.className, 'cloud-form', 'expanded data reuses themed form controls');
  assert.ok(find(content, 'textarea'));
  find(render(), 'button').props.onClick(); await new Promise(resolve => setImmediate(resolve));
  assert.equal(JSON.parse(copied).unit, 'ms');
  assert.deepEqual(JSON.parse(copied).entries, JSON.parse(JSON.stringify(selected)));
  assert.equal(JSON.stringify(render()).includes('已复制解锁耗时'), true);
  copyFails = true;
  find(render(), 'button').props.onClick(); await new Promise(resolve => setImmediate(resolve));
  assert.equal(JSON.stringify(render()).includes('复制失败'), true);
  first.props.onToggle({ currentTarget: { open: true } }); assert.equal(reads, 2, 'reopening refreshes the snapshot');
  console.log('unlock timings: passed');
})().catch(error => { console.error(error); process.exitCode = 1; });
