// Run from repository root: node docs/audit-2026-09-10/frontend_probes.cjs
// Executes production store and hook code with React/transport test doubles.
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const assert = require('node:assert/strict');
const { createRequire } = require('node:module');
const root = path.resolve(__dirname, '../..');
const ts = createRequire(path.join(root, 'frontends/web/package.json'))('typescript');
const modules = {};
function create(init) {
  let state;
  const set = update => { state = {...state, ...(typeof update === 'function' ? update(state) : update)}; };
  const hook = fn => fn(state);
  hook.getState = () => state;
  state = init(set, hook.getState);
  return hook;
}
function load(relative) {
  const code = ts.transpileModule(fs.readFileSync(path.join(root, relative), 'utf8'), {
    compilerOptions: {target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.CommonJS}
  }).outputText;
  const exports = {};
  vm.runInNewContext(code, {exports, require: name => {
    if (!(name in modules)) throw new Error(`Unmocked module: ${name}`);
    return modules[name];
  }, console, Date, Math, setTimeout, clearTimeout}, {filename: relative});
  return exports;
}
modules.zustand = {create};
modules.react = {useCallback: fn => fn, useMemo: fn => fn()};
modules['@/store/chat'] = load('frontends/web/src/store/chat.ts');
const {useChatStore} = modules['@/store/chat'];
const session = {currentSessionId:'s', refreshSessions() {}};
const useSessionStore = fn => fn(session);
useSessionStore.getState = () => session;
modules['@/store/session'] = {useSessionStore};
modules['@/store/config'] = {useConfigStore: fn => fn({config:{currentModel:{provider:'fake',model:'fake'}},providers:[]})};
modules['@/store/ui'] = {useUiStore:fn => fn({reasoningEffort:'off'}),clampEffort:x=>x};
const backend = [{role:'user',content:'old question'},{role:'assistant',content:'old answer'}];
const sent = [];
modules['@/api'] = {getApi:async () => ({chat:async (params,onEvent) => {
  sent.push(params);
  backend.push({role:'user',content:params.input});
  onEvent({type:'delta',content:'new answer'});
  backend.push({role:'assistant',content:'new answer'});
  onEvent({type:'finished'});
}})};
const {useChat} = load('frontends/web/src/hooks/useChat.ts');
function seed() {
  useChatStore.getState().hydrateSession('s',[
    {id:'m-0',role:'user',content:'old question',status:'done'},
    {id:'m-1',role:'assistant',content:'old answer',status:'done'}
  ]);
}
(async () => {
  seed();
  await useChat().regenerate('m-1');
  const local = useChatStore.getState().getMessages('s');
  assert.equal(local.length,1);
  assert.equal(local[0].role,'assistant');
  assert.equal(backend.length,4);
  assert.equal(sent[0].input,'old question');
  console.log('REPRODUCED: regenerate removes the user message locally; transport appends the question to unchanged backend history.');
  seed();
  await useChat().editAndResend('m-0','edited question');
  assert.equal(useChatStore.getState().getMessages('s')[0].content,'edited question');
  assert.equal(backend[0].content,'old question');
  assert.equal(sent[1].input,'edited question');
  console.log('REPRODUCED: edit changes local state, but sends a normal new-turn request without a history revision.');
  const {cacheHitRate} = load('frontends/web/src/lib/format.ts');
  assert.equal(cacheHitRate(80,100),'44.4%');
  console.log('OBSERVED: cacheHitRate(80 cached, 100 total input) = 44.4%, rather than 80.0%.');
})().catch(e=>{console.error(e);process.exitCode=1});
