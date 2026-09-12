// Run with pnpm test in frontends/web. No React renderer or external services required.
// Executes production store and hook code with React/transport test doubles.
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const assert = require('node:assert/strict');
const { createRequire } = require('node:module');
const root = path.resolve(__dirname, '../../..');
const ts = createRequire(path.join(root, 'frontends/web/package.json'))('typescript');
const modules = {};
let fetchMock;
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
  }, console, Date, Math, setTimeout, clearTimeout, Headers, TextDecoder, fetch: (...args) => fetchMock(...args)}, {filename: relative});
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
let backend;
let reject = false;
let pending = null;
const sent = [];
function stored() { return backend.map((m, i) => ({...m, id:`persisted-${i}`, revision:'r1', status:'done', createdAt:0})); }
modules['@/api'] = {getApi:async () => ({
  getMessages: async () => stored(),
  cancelChat: async id => { assert.equal(id, 's'); },
  chat: async (params,onEvent) => {
    sent.push(params);
    if (reject) throw new Error('rejected');
    if (params.edit) {
      assert.equal(params.edit.expectedMessageCount, backend.length);
      assert.equal(params.edit.expectedRevision, 'r1');
      assert.equal(params.edit.userId, stored()[params.edit.userIndex].id);
      backend.splice(params.edit.userIndex);
    }
    backend.push({role:'user',content:params.input,attachments:params.attachments});
    if (pending) await pending;
    onEvent({type:'delta',content:'new answer'});
    backend.push({role:'assistant',content:'new answer'});
    onEvent({type:'finished'});
  }
})};
const {useChat} = load('frontends/web/src/hooks/useChat.ts');
function seed(withTool = false) {
  backend = [{role:'user',content:'old question',attachments:[{name:'fixture'}]}, ...(withTool ? [{role:'tool',content:'result'}] : []), {role:'assistant',content:'old answer'}];
  useChatStore.getState().hydrateSession('s',stored());
}
(async () => {
  seed(true);
  await useChat().regenerate('persisted-2');
  assert.equal(backend.length,2);
  assert.equal(backend[0].content,'old question');
  assert.equal(sent[0].attachments.length,1);
  assert.equal(useChatStore.getState().getMessages('s')[0].id,'persisted-0');
  seed();
  await useChat().editAndResend('persisted-0','edited question');
  assert.equal(backend.length,2);
  assert.equal(backend[0].content,'edited question');
  seed(); reject = true;
  await useChat().editAndResend('persisted-0','rejected edit');
  const local = useChatStore.getState().getMessages('s');
  assert.equal(local[0].content,'old question');
  assert.equal(local[1].content,'old answer');
  assert.equal(local[2].status,'error');
  reject = false; seed();
  let release; pending = new Promise(resolve => {release=resolve});
  const before = sent.length;
  const hook = useChat();
  const first = hook.send('one');
  await hook.send('duplicate');
  await Promise.resolve(); await Promise.resolve();
  session.currentSessionId = 'another';
  await useChat().cancel();
  release(); await first;
  assert.equal(sent.length, before + 1);
  assert.equal(useChatStore.getState().isStreaming,false);
  const {cacheHitRate} = load('frontends/web/src/lib/format.ts');
  assert.equal(cacheHitRate(80,100),'80.0%');
  assert.equal(cacheHitRate(0,100),'0.0%');

  modules['./normalize'] = load('frontends/web/src/api/normalize.ts');
  let callback; let unlistened = 0; let rejectInvoke = false;
  modules['@tauri-apps/api/event'] = {listen: async (_, cb) => { callback=cb; return () => {unlistened++}; }};
  modules['@tauri-apps/api/core'] = {invoke: async (command, args) => {
    if (command === 'chat') {
      if (rejectInvoke) throw new Error('invoke rejected');
      callback({payload:{type:'delta',messageId:'other',content:'wrong'}});
      callback({payload:{type:'delta',messageId:args.params.messageId,content:'right'}});
      callback({payload:{type:'finished',messageId:args.params.messageId}});
    } else if (command === 'get_messages') {
      assert.equal(args.sessionId,'s'); assert.equal(args.session_id,undefined); return [{id:'persisted-1',role:'assistant',content:'partial',status:'cancelled'}];
    }
  }};
  const {tauriApi} = load('frontends/web/src/api/tauri.ts');
  const events=[];
  await tauriApi.chat({sessionId:'s',messageId:'run',input:'q'},e=>events.push(e));
  assert.equal(events.length,2); assert.equal(events[0].content,'right'); assert.equal(unlistened,1);
  assert.equal((await tauriApi.getMessages('s'))[0].status,'cancelled');
  rejectInvoke=true;
  await assert.rejects(tauriApi.chat({sessionId:'s',messageId:'run',input:'q'},()=>{}),/invoke rejected/);
  assert.equal(unlistened,2);
  const {httpApi} = load('frontends/web/src/api/http.ts');
  fetchMock = async (_, init) => {
    assert.equal(init.headers['X-LLMN-Client'],'1');
    return new Response('data: {"type":"delta","content":"partial"}\n\n', {status:200});
  };
  await assert.rejects(httpApi.chat({sessionId:'s',input:'q'},()=>{}), /完成事件/);
  fetchMock = async () => new Response('data: {"type":"finished"}\n\n',{status:200});
  await httpApi.chat({sessionId:'s',input:'q'},()=>{});
  fetchMock = async () => new Response(JSON.stringify([{id:'persisted-1',role:'assistant',content:'partial',status:'error',error:'lost'}]));
  const history = await httpApi.getMessages('s');
  assert.equal(history[0].status,'error'); assert.equal(history[0].error,'lost');
  console.log('PASS: Tauri event filtering, camelCase arguments, listener cleanup, HTTP terminal/EOF handling');
  console.log('PASS: regenerate across tools, edit persistence contract, rejected edit restoration, duplicate send, cancel original session, cache ratio');
})().catch(e=>{console.error(e);process.exitCode=1});
