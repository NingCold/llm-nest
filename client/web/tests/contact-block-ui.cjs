// Exercise the actual control and retry hook: no optimistic block on failure.
const fs = require('node:fs'), path = require('node:path'), vm = require('node:vm'), assert = require('node:assert/strict');
const { webcrypto } = require('node:crypto'), ts = require('typescript');
const flush = () => new Promise(resolve => setImmediate(resolve));
const slots = [], calls = []; let index = 0, fail = true, refreshes = 0;
const entry = { accountId: 'ai', ownerId: 'human', listed: false, blocked: false, revision: '7' };
const state = { error: null, refresh: async () => { refreshes++ }, action: async fn => { try { return await fn() } catch (e) { state.error = String(e) } } };
const react = {
  useState(initial) { const i = index++; if (!(i in slots)) slots[i] = initial; return [slots[i], value => slots[i] = typeof value === 'function' ? value(slots[i]) : value] },
  useRef(initial) { const i = index++; return slots[i] ??= { current: initial } }
};
const jsx = (type, props) => ({ type, props }), exported = {};
const client = { setContactBlock: async (id, input) => { calls.push({ id, input }); if (fail) throw new Error('lost acknowledgement'); entry.blocked = input.blocked; entry.revision = '8'; return { ...entry } } };
const code = ts.transpileModule(fs.readFileSync(path.resolve(__dirname, '../src/social/LiveControls.tsx'), 'utf8'), { compilerOptions: { jsx: ts.JsxEmit.ReactJSX, target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.CommonJS } }).outputText;
vm.runInNewContext(code, { exports: exported, crypto: webcrypto, require: name => {
  if (name === 'react') return react;
  if (name === 'react/jsx-runtime') return { jsx, jsxs: jsx };
  if (name === './live-store') return { useLiveSocialStore: () => state };
  if (name === './client') return { socialClient: client, SocialApiError: class extends Error {} };
  return {};
} });
const render = () => { index = 0; return exported.ContactBlockControls({ contact: { id: 'ai', name: 'Alice' }, entry }) };
const all = n => !n || typeof n !== 'object' ? [] : [n, ...[n.props?.children].flat(Infinity).flatMap(all)];
const button = label => all(render()).find(n => n.type === 'button' && n.props.children === label);
(async () => {
  button('屏蔽联系人').props.onClick(); assert.equal(calls.length, 0);
  button('取消').props.onClick(); assert.ok(button('屏蔽联系人')); assert.equal(calls.length, 0);
  button('屏蔽联系人').props.onClick(); button('确认屏蔽').props.onClick(); await flush();
  assert.equal(entry.blocked, false); assert.equal(refreshes, 0); assert.ok(button('确认屏蔽'));
  fail = false; button('确认屏蔽').props.onClick(); await flush();
  assert.equal(JSON.stringify(calls[0]), JSON.stringify(calls[1]));
  assert.equal(calls[1].input.expectedRevision, '7'); assert.equal(entry.blocked, true); assert.equal(entry.listed, false);
  assert.equal(refreshes, 1); button('解除屏蔽').props.onClick(); await flush();
  assert.equal(calls[2].input.blocked, false); assert.equal(calls[2].input.expectedRevision, '8');
  assert.notEqual(calls[1].input.requestId, calls[2].input.requestId); assert.ok(button('屏蔽联系人'));
  console.log('PASS: explicit block confirmation, failed-write retention, stable retry identity, independent listing and unblock');
})().catch(e => { console.error(e); process.exitCode = 1 });
