// Run production editor/hooks with controlled state and transport acknowledgements.
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const assert = require('node:assert/strict');
const { webcrypto } = require('node:crypto');
const ts = require('typescript');
const root = path.resolve(__dirname, '../src/social');
const flush = () => new Promise(resolve => setImmediate(resolve));
function harness(memory) {
  const slots = []; let index = 0, done = 0;
  const calls = []; let fail = true;
  const react = {
    useState(initial) { const i = index++; if (!(i in slots)) slots[i] = initial; return [slots[i], value => { slots[i] = typeof value === 'function' ? value(slots[i]) : value; }]; },
    useRef(initial) { const i = index++; return slots[i] ??= { current: initial }; },
    useEffect() {},
  };
  const jsx = (type, props) => ({ type, props });
  const state = { snapshot: { cursor: '0' }, error: null, action: async operation => { try { return await operation(); } catch (error) { state.error = String(error); } }, refresh: async () => {} };
  const client = {
    createMemory: async (conversation, actor, input) => { calls.push({ conversation, actor, input }); if (fail) throw new Error('lost acknowledgement'); return { id: 'saved' }; },
    updateMemory: async (id, input) => { calls.push({ id, input }); if (fail) throw new Error('lost acknowledgement'); return { id }; },
    message: async () => ({ id: 'source', text: 'corrected original', status: 'committed', revision: '2', senderName: 'Human' }),
  };
  const modules = {};
  function load(name) {
    if (modules[name]) return modules[name];
    const exports = {}; modules[name] = exports;
    let code = fs.readFileSync(path.join(root, name + '.tsx'), 'utf8');
    if (name === 'LiveMemory') code += '\nexport { MemoryEditor };';
    const source = ts.transpileModule(code, { compilerOptions: { jsx: ts.JsxEmit.ReactJSX, target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.CommonJS } }).outputText;
    vm.runInNewContext(source, { exports, crypto: webcrypto, require: name => {
      if (name === 'react') return react;
      if (name === 'react/jsx-runtime') return { jsx, jsxs: jsx };
      if (name === './live-store') return { useLiveSocialStore: () => state };
      if (name === './LiveControls') return load('LiveControls');
      if (name === './client') return { socialClient: client, SocialApiError: class extends Error {} };
      return {};
    } });
    return exports;
  }
  const { MemoryEditor } = load('LiveMemory');
  const render = () => { index = 0; return MemoryEditor({ conversation: { id: 'chat', kind: 'direct' }, actor: 'ai', memory, done: () => done++ }); };
  function all(node) { return !node || typeof node !== 'object' ? [] : [node, ...[node.props?.children].flat(Infinity).flatMap(all)]; }
  return { render, nodes: () => all(render()), calls, state, succeed: () => { fail = false; }, done: () => done };
}
(async () => {
  const h = harness();
  h.nodes().find(n => n.type === 'textarea').props.onChange({ target: { value: 'remember my preference' } });
  h.render().props.onSubmit({ preventDefault() {} }); await flush();
  assert.equal(h.calls.length, 1);
  const first = h.calls[0];
  assert.equal(h.done(), 0);
  assert.equal(h.nodes().find(n => n.type === 'textarea').props.value, 'remember my preference');
  assert.match(h.state.error, /lost acknowledgement/);
  h.succeed(); h.render().props.onSubmit({ preventDefault() {} }); await flush();
  assert.equal(h.calls[1].input.requestId, first.input.requestId);
  assert.equal(h.calls[1].conversation, 'chat'); assert.equal(h.calls[1].actor, 'ai');
  assert.equal(h.done(), 1);

  const stale = { id: 'memory', kind: 'fact', text: 'old conclusion', revision: '3', status: 'stale', source: { messageId: 'source', revision: '1' } };
  const edit = harness(stale);
  edit.render().props.onSubmit({ preventDefault() {} }); await flush();
  assert.equal(edit.calls[0].input.source.revision, '1', 'source versions never advance silently');
  edit.nodes().find(n => n.type === 'button' && n.props.children === '查看当前原文').props.onClick(); await flush();
  edit.nodes().find(n => n.type === 'button' && n.props.children === '已核对，采用此版本').props.onClick();
  edit.succeed(); edit.render().props.onSubmit({ preventDefault() {} }); await flush();
  assert.equal(edit.calls[1].input.source.revision, '2');
  assert.equal(edit.calls[1].input.expectedRevision, '3');
  assert.notEqual(edit.calls[0].input.requestId, edit.calls[1].input.requestId, 'changed provenance is a new command');
  console.log('PASS: memory editor retains failed input, retries the same command, scopes the owner/window, and requires explicit source-version confirmation');
})().catch(error => { console.error(error); process.exitCode = 1; });
