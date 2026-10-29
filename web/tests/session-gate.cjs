const assert = require('node:assert/strict'), fs = require('node:fs'), vm = require('node:vm'), ts = require('typescript'), React = require('react');
const jsx = require('react/jsx-runtime');
const transpile = path => ts.transpileModule(fs.readFileSync(require.resolve(path), 'utf8'), { compilerOptions: { jsx: ts.JsxEmit.ReactJSX, module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 } }).outputText;
const sessionCode = transpile('../src/cloud/App.tsx'), lockCode = transpile('../src/cloud/LockScreen.tsx'), apiCode = transpile('../src/cloud/api.ts');
// A small hook/lifecycle renderer exercises actual AccountSession and LockScreen
// branches, effects, handlers and child unmounting without adding a browser dependency.
function renderer() {
  let current, index, dirty = false, tree, root, rootProps;
  const instances = new Map(), effects = new Map();
  const hooks = {
    ...React,
    useState(initial) { const instance = current, i = index++; if (!(i in instance.values)) instance.values[i] = typeof initial === 'function' ? initial() : initial; return [instance.values[i], value => { const next = typeof value === 'function' ? value(instance.values[i]) : value; if (next !== instance.values[i]) { instance.values[i] = next; dirty = true; } }]; },
    useRef(initial) { const i = index++; return current.values[i] ??= { current: initial }; },
    useEffect(work, deps) { const instance = current, i = index++, previous = instance.effects[i]; if (!previous || !deps || deps.some((v, n) => !Object.is(v, previous.deps[n]))) { const effect = { work, deps, cleanup: previous?.cleanup }; instance.effects[i] = effect; effects.set(`${instance.path}:${i}`, effect); } },
  };
  function cleanup(instance) { for (const effect of instance.effects) effect?.cleanup?.(); }
  function walk(node, path, seen) {
    if (Array.isArray(node)) return node.map((child, i) => walk(child, `${path}.${i}`, seen));
    if (!node || typeof node !== 'object') return node;
    if (node.type === React.Fragment) return walk(node.props.children, path, seen);
    if (typeof node.type === 'function') {
      seen.add(path);
      let instance = instances.get(path);
      if (instance?.type !== node.type) { if (instance) cleanup(instance); instance = { path, type: node.type, values: [], effects: [] }; instances.set(path, instance); }
      const parent = current, parentIndex = index; current = instance; index = 0;
      const result = node.type(node.props); current = parent; index = parentIndex;
      return walk(result, `${path}.child`, seen);
    }
    return { ...node, props: { ...node.props, children: walk(node.props?.children, `${path}.children`, seen) } };
  }
  function render() {
    dirty = false; const seen = new Set(); tree = walk(jsx.jsx(root, rootProps), 'root', seen);
    for (const [path, instance] of instances) if (!seen.has(path)) { cleanup(instance); instances.delete(path); }
  }
  function commit() { const pending = [...effects.values()]; effects.clear(); for (const effect of pending) { effect.cleanup?.(); effect.cleanup = effect.work(); } }
  return {
    hooks,
    mount(component, props, strict = false) { root = component; rootProps = props; render(); if (strict) render(); commit(); if (strict) for (const instance of instances.values()) for (const effect of instance.effects) if (effect) { effect.cleanup?.(); effect.cleanup = effect.work(); } },
    async flush() { let quiet = 0; for (let i = 0; i < 12; i++) { await new Promise(setImmediate); const worked = dirty || effects.size; if (dirty) render(); if (effects.size) commit(); quiet = !worked && !dirty && !effects.size ? quiet + 1 : 0; if (quiet === 2) return; } throw new Error('render did not settle'); },
    get tree() { return tree; },
    unmount() { for (const instance of instances.values()) cleanup(instance); instances.clear(); },
  };
}
function nodes(tree) { if (Array.isArray(tree)) return tree.flatMap(nodes); if (!tree || typeof tree !== 'object') return []; return [tree, ...nodes(tree.props?.children)]; }
function text(tree) { if (Array.isArray(tree)) return tree.map(text).join(''); if (tree == null || typeof tree === 'boolean') return ''; return typeof tree === 'object' ? text(tree.props?.children) : String(tree); }
const button = (tree, label) => nodes(tree).find(node => node.type === 'button' && text(node) === label);
function deferred() { let resolve, reject; const promise = new Promise((yes, no) => { resolve = yes; reject = no; }); return { promise, resolve, reject }; }
function harness(options = {}) {
  const r = renderer(), events = new EventTarget(), state = { active: options.active ?? false, known: options.known ?? true, paused: false, autoCalls: 0, manualCalls: [], mounts: 0, unmounts: 0, subscriptions: 0, apiCalls: 0 };
  const pending = deferred(), exports = {}, lock = {}, nativeListeners = new Set();
  const native = payload => { for (const callback of nativeListeners) callback({ payload }); };
  const invoke = async (name, args) => {
    if (name === 'cloud_unlock_support') { state.supportCalls = (state.supportCalls ?? 0) + 1; return options.support ?? true; }
    if (name === 'cloud_set_server') return;
    if (name === 'cloud_request') { state.nativeRequests = (state.nativeRequests ?? 0) + 1; return options.nativeRequest(args, state, native); }
    throw new Error(`Unexpected native command: ${name}`);
  };
  const engine = {
    cryptoDevice: () => state.active ? 'existing-device' : undefined,
    hasCryptoDevice: () => state.known,
    lockCrypto: () => { state.active = false; events.dispatchEvent(new Event('llmn-crypto-change')); },
  };
  const automatic = {
    tryAutoUnlock: () => { state.autoCalls++; return pending.promise; },
    pauseAutoUnlock: () => { state.paused = true; },
    forgetAutoUnlock: async () => {},
    unlockManually: async (account, passphrase) => { state.manualCalls.push([account, passphrase]); if (passphrase !== 'correct-local-pass') throw new Error('wrong local passphrase'); state.active = true; state.paused = false; events.dispatchEvent(new Event('llmn-crypto-change')); },
  };
  const icon = () => jsx.jsx('svg', {});
  vm.runInNewContext(lockCode, { exports: lock, require: name => name === 'react' ? r.hooks : name === 'lucide-react' ? new Proxy({}, { get: () => icon }) : name === '@/components/BrandMark' ? { BrandMark: () => jsx.jsx('span', {}) } : name === '@/lib/desktop' ? {} : name === '@/store/ui' ? { useUiStore: fn => fn({ theme: 'light', toggleTheme() {} }) } : name.endsWith('.css') ? {} : require(name) });
  function ChatApp(props) {
    const [location, setLocation] = r.hooks.useState(props.location);
    r.hooks.useEffect(() => { state.mounts++; state.subscriptions++; return () => { state.unmounts++; state.subscriptions--; }; }, []);
    return jsx.jsxs('main', { 'data-chat': true, 'data-location': location, children: ['DECRYPTED CHAT SENTINEL', jsx.jsx('button', { onClick: () => { const next = { active: 'group-1', tab: 'messages', accountSection: 'crypto', scrollTop: 312 }; setLocation(next); props.rememberLocation(next); }, children: 'navigate group' }), jsx.jsx('button', { onClick: props.onLock, children: 'lock chat' }), jsx.jsx('button', { onClick: props.onLogout, children: 'logout chat' })] });
  }
  function CryptoDevices() { return jsx.jsxs('section', { 'data-recovery': true, children: ['RECOVERY SETUP ONLY', jsx.jsx('button', { onClick: () => { state.active = true; state.known = true; events.dispatchEvent(new Event('llmn-crypto-change')); }, children: 'complete recovery' })] }); }
  class HttpError extends Error {}
  let api = { server: 'https://gate.invalid', HttpError, request: async path => { state.apiCalls++; return state.respond ? state.respond(path) : { id: 'human', name: 'Human' }; }, connectServer: async () => ({ id: 'human', name: 'Human' }), invalidate() { state.invalidations = (state.invalidations ?? 0) + 1; } };
  if (options.realApi) {
    api = {};
    vm.runInNewContext(apiCode, { exports:api, Event, URL, window:events, localStorage:{getItem:()=> 'https://gate.invalid',setItem(){}}, require:name=>name==='@/lib/desktop'?{IS_DESKTOP:true}:name==='@tauri-apps/api/core'?{invoke,Channel:class {}}:require(name) });
    const invalidate = api.invalidate;
    api.invalidate = () => { state.invalidations = (state.invalidations ?? 0) + 1; invalidate(); };
  }
  function AuthPage(props) { state.authProps = props; return jsx.jsx('main', { 'data-auth': true, 'data-error': props.error, children: 'LOGIN' + props.error }); }
  vm.runInNewContext(sessionCode, { exports, Event, window: events, location: { hash: '' }, FormData:class {constructor(form){this.fields=form.fields;}entries(){return Object.entries(this.fields);}get(key){return this.fields[key];}}, require: name => name === 'react' ? r.hooks : name === 'lucide-react' ? { ArrowLeft: icon } : name === '@/lib/desktop' ? { CUSTOM_WINDOW_CHROME: false, IS_DESKTOP: !!options.desktop } : name === '@/hooks/useDesktopReady' ? { useDesktopReady() {} } : name === '@/components/StartupSurface' ? { StartupSurface: () => jsx.jsx('main', { children: 'STARTUP' }) } : name === './LockScreen' ? lock : name === './AuthPage' ? { AuthPage } : name === './ChatApp' ? { default: ChatApp } : name === './crypto/Devices' ? { CryptoDevices } : name === './crypto/engine' ? engine : name === './crypto/auto-unlock' ? automatic : name === './api' ? api : name === '@tauri-apps/api/core' ? { invoke } : name === '@tauri-apps/api/event' ? { listen: async (name, callback) => { assert.equal(name, 'cloud-auth-lost'); nativeListeners.add(callback); return () => nativeListeners.delete(callback); } } : name.endsWith('.css') ? {} : require(name) });
  let logouts = 0;
  r.mount(options.cloud ? exports.default : exports.AccountSession, options.cloud ? {} : { account: { id: 'human', name: 'Human' }, onLogout: async () => { logouts++; }, onEndSession() {} }, options.strict);
  return { r, state, pending, native, get logouts() { return logouts; } };
}
async function manual(h, value) {
  const input = nodes(h.r.tree).find(node => node.type === 'input' && node.props.type === 'password');
  input.props.onChange({ target: { value } }); await h.r.flush();
  nodes(h.r.tree).find(node => node.type === 'form').props.onSubmit({ preventDefault() {} });
  await h.r.flush();
}
(async () => {
  const h = harness({ strict: true });
  assert.equal(h.state.autoCalls, 1, 'StrictMode effect replay shares a single SDK opening attempt');
  assert.match(text(h.r.tree), /正在解锁/);
  assert.equal(nodes(h.r.tree).some(node => node.type === 'form'), false, 'manual form never flashes while automatic unlock is pending');
  assert.equal(h.state.mounts, 0, 'chat and subscriptions never initialize in the locked branch');
  assert.equal(text(h.r.tree).includes('DECRYPTED'), false);
  h.pending.resolve(false); await h.r.flush();
  assert.ok(nodes(h.r.tree).some(node => node.type === 'form'));
  await manual(h, 'wrong');
  assert.match(text(h.r.tree), /wrong local passphrase/);
  assert.equal(h.state.mounts, 0);
  assert.equal(nodes(h.r.tree).find(node => node.type === 'input').props.value, '', 'submitted passphrase leaves the controlled field');
  await manual(h, 'correct-local-pass');
  assert.equal(h.state.mounts, 1); assert.equal(h.state.subscriptions, 1);
  button(h.r.tree, 'navigate group').props.onClick(); await h.r.flush();
  button(h.r.tree, 'lock chat').props.onClick(); await h.r.flush();
  assert.equal(h.state.paused, true);
  assert.equal(h.state.unmounts, 1); assert.equal(h.state.subscriptions, 0, 'locking actually unmounts chat subscriptions');
  assert.equal(text(h.r.tree).includes('DECRYPTED'), false, 'decrypted tree is removed, rather than visually overlaid');
  assert.ok(nodes(h.r.tree).some(node => node.type === 'form'));
  assert.equal(h.state.autoCalls, 1, 'manual lock never automatically opens the SDK again');
  await manual(h, 'correct-local-pass');
  assert.deepEqual(JSON.parse(JSON.stringify(nodes(h.r.tree).find(node => node.props?.['data-chat']).props['data-location'])), { active: 'group-1', tab: 'messages', accountSection: 'crypto', scrollTop: 312 }, 'account-scoped navigation and scroll survive chat unmounting');
  button(h.r.tree, 'lock chat').props.onClick(); await h.r.flush();
  button(h.r.tree, '使用恢复文件').props.onClick(); await h.r.flush();
  assert.match(text(h.r.tree), /RECOVERY SETUP ONLY/);
  assert.equal(text(h.r.tree).includes('DECRYPTED'), false);
  button(h.r.tree, '返回解锁').props.onClick(); await h.r.flush();
  assert.ok(nodes(h.r.tree).some(node => node.type === 'form'));
  h.r.unmount();
  const automatic = harness();
  automatic.state.active = true; automatic.pending.resolve(true); await automatic.r.flush();
  assert.equal(automatic.state.mounts, 1);
  assert.equal(nodes(automatic.r.tree).some(node => node.type === 'form'), false);
  automatic.r.unmount();
  const recovery = harness({ known: false });
  recovery.pending.resolve(false); await recovery.r.flush();
  assert.match(text(recovery.r.tree), /RECOVERY SETUP ONLY/, 'missing local identity enters independent setup, without creating chat');
  assert.equal(recovery.state.mounts, 0);
  button(recovery.r.tree, 'complete recovery').props.onClick(); await recovery.r.flush();
  button(recovery.r.tree, '进入聊天').props.onClick(); await recovery.r.flush();
  assert.equal(recovery.state.mounts, 1); recovery.r.unmount();
  const failed = harness(); failed.pending.reject(new Error('auto material expired')); await failed.r.flush();
  assert.match(text(failed.r.tree), /auto material expired/); assert.equal(failed.state.mounts, 0); failed.r.unmount();
  const startup = harness({ cloud: true, strict: true }); await startup.r.flush();
  assert.equal(startup.state.apiCalls, 1, 'StrictMode initial authentication is not submitted twice'); startup.r.unmount();
  const signedOut = harness({ cloud:true,desktop:true,strict:true,realApi:true,nativeRequest:async(args,state,emit)=>{
    assert.ok(['/api/me','/api/auth/login'].includes(args.path));
    emit({server:'https://gate.invalid',accountId:null,message:null});
    return {status:401,body:JSON.stringify({code:'unauthorized',message:'unauthorized'})};
  }});
  await signedOut.r.flush();
  assert.equal(signedOut.state.authProps.connected,true,'an unsigned /me 401 still confirms the server connection');
  assert.equal(signedOut.state.authProps.error,'','native auth-lost before the expected 401 does not invalidate the in-flight connection');
  assert.equal(signedOut.state.invalidations??0,0);
  assert.equal(signedOut.state.nativeRequests,1,'StrictMode shares the initial connection');
  assert.equal(signedOut.state.supportCalls,1,'StrictMode shares capability detection');
  assert.equal(signedOut.state.authProps.rememberSupported,true,'the live StrictMode effect receives the capability result');
  signedOut.state.authProps.onSubmit({fields:{email:'human@example.invalid',password:'wrong-password',rememberLogin:'on'}});await signedOut.r.flush();
  assert.match(signedOut.state.authProps.error,/邮箱或密码不正确/,'login 401 retains the useful credential error');
  assert.doesNotMatch(signedOut.state.authProps.error,/账号已切换/);
  assert.equal(signedOut.state.invalidations??0,0);
  signedOut.native({server:'https://gate.invalid',accountId:'expired-owner',message:'无法清理本机保存的认证资料'});await signedOut.r.flush();
  assert.match(signedOut.state.authProps.error,/无法清理本机保存的认证资料/,'unsigned cleanup failures remain visible without ending a nonexistent session');
  assert.equal(signedOut.state.invalidations??0,0);signedOut.r.unmount();
  const capability = deferred();
  const retry = harness({cloud:true,desktop:true,strict:true,realApi:true,support:capability.promise,nativeRequest:async(args,state,emit)=>{
    if(state.nativeRequests===1)throw new Error('network unavailable');
    emit({server:'https://gate.invalid',accountId:null,message:null});
    return {status:401,body:'{}'};
  }});
  await retry.r.flush();
  assert.equal(retry.state.authProps.connected,false);assert.match(retry.state.authProps.error,/network unavailable/);
  capability.resolve(true);await retry.r.flush();
  assert.equal(retry.state.authProps.rememberSupported,true,'capability detection survives a failed connection and StrictMode cleanup');
  retry.state.authProps.onConnect();await retry.r.flush();
  assert.equal(retry.state.authProps.connected,true);assert.equal(retry.state.authProps.rememberSupported,true);
  assert.equal(retry.state.authProps.error,'');assert.equal(retry.state.nativeRequests,2);assert.equal(retry.state.supportCalls,1);
  retry.r.unmount();
  const native = harness({ cloud: true, desktop: true, strict: true, active: true }); await native.r.flush();
  assert.match(text(native.r.tree), /DECRYPTED/);
  native.native({ server: 'https://other.invalid', accountId: 'human', message: 'other server expired' });
  native.native({ server: 'https://gate.invalid', accountId: 'other-human', message: 'other account expired' });
  await native.r.flush();
  assert.match(text(native.r.tree), /DECRYPTED/); assert.equal(native.state.invalidations ?? 0, 0, 'other native scopes cannot terminate this session');
  native.native({ server: 'https://gate.invalid', accountId: 'human', message: 'saved authentication expired' }); await native.r.flush();
  assert.equal(native.state.subscriptions, 0);
  assert.equal(text(native.r.tree).includes('DECRYPTED'), false);
  assert.match(text(native.r.tree), /LOGINsaved authentication expired/);
  native.r.unmount();
  const normalLogout = harness({ cloud: true, desktop: true, active: true }); await normalLogout.r.flush();
  const logoutReply = deferred(); normalLogout.state.respond = () => logoutReply.promise;
  const loggingOut = button(normalLogout.r.tree, 'logout chat').props.onClick();
  await normalLogout.r.flush();
  normalLogout.native({ server: 'https://gate.invalid', accountId: 'human', message: null }); await normalLogout.r.flush();
  logoutReply.reject(new Error('账号已切换')); await loggingOut; await normalLogout.r.flush();
  assert.equal(nodes(normalLogout.r.tree).find(node => node.props?.['data-auth']).props['data-error'], '', 'normal native logout before the IPC reply does not display an account-switch error');
  normalLogout.r.unmount();
  console.log('PASS: account session removes locked chat, awaits one automatic opening, resumes manual unlock/navigation, isolates recovery, and survives StrictMode effect replay');
})().catch(error => { console.error(error); process.exitCode = 1; });
