// Exercise the real desktop startup view, including failed/dismissed initialization.
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const assert = require('node:assert/strict');
const React = require('react');
const { renderToStaticMarkup } = require('react-dom/server');
const ts = require('typescript');
const root = path.resolve(__dirname, '../src');
const cache = new Map();
const ui = { desktopReady:false, theme:'dark', showSettings() {}, toggleTheme() {}, setDesktopReady() {} };
const state = { snapshot:null, loading:true, error:null, connected:false, connectionError:null,
  drafts:{}, initialize() {}, disconnect() {}, action() {}, refresh() {}, dismissError() {} };
function load(name) {
  if (cache.has(name)) return cache.get(name);
  const file = ['.tsx','.ts'].map(ext=>path.join(root,name+ext)).find(fs.existsSync);
  const output = ts.transpileModule(fs.readFileSync(file,'utf8'), {compilerOptions:{jsx:ts.JsxEmit.ReactJSX,target:ts.ScriptTarget.ES2022,module:ts.ModuleKind.CommonJS}}).outputText;
  const exports={}; cache.set(name,exports);
  vm.runInNewContext(output, { exports, location:{hash:''}, require:spec=> {
    if (spec==='@/lib/desktop') return {IS_DESKTOP:true,CUSTOM_WINDOW_CHROME:true};
    if (spec==='@tauri-apps/api/core') return {invoke(){throw new Error('SSR must not invoke native commands');}};
    if (spec==='@tauri-apps/api/window') return {getCurrentWindow(){throw new Error('SSR must not operate the native window');}};
    if (spec==='@/store/ui') return {useUiStore:Object.assign(fn=>fn(ui),{getState:()=>ui})};
    if (spec==='./live-store') return {useLiveSocialStore:selector=>selector?selector(state):state};
    if (spec==='./AccountPages') return {AccountDetails:()=>null,AccountViewer:()=>null,AiManagement:()=>null};
    if (spec==='./LiveChat') return {Chat:()=>null};
    if (spec==='./GroupControls') return {CreateGroupForm:()=>null};
    if (spec==='@/components/settings/SettingsPage') return {SettingsPage:()=>null};
    if (spec==='@/hooks/useInit') return {useInit:()=>({ready:false,error:null})};
    if (spec==='@/store/config') return {useConfigStore:()=>null};
    if (spec==='@/components/ui/dialog') return {Dialog:()=>null,DialogContent:()=>null,DialogDescription:()=>null,DialogTitle:()=>null};
    if (spec.endsWith('.css')) return {};
    if (spec.startsWith('@/')) return load(spec.slice(2));
    if (spec.startsWith('.')) return load(path.posix.normalize(path.posix.join(path.posix.dirname(name),spec)));
    return require(spec);
  }});
  return exports;
}
const App=load('social/App').default;
const render=()=>renderToStaticMarkup(React.createElement(App));
let html=render();
assert.match(html,/desktop-startup-surface/);
assert.doesNotMatch(html,/主导航|重试连接|连接恢复中/);
state.loading=false; state.error='configuration cannot be parsed';
html=render();
assert.match(html,/configuration cannot be parsed/);
assert.match(html,/本地服务启动失败/);
assert.match(html,/重试连接/);
assert.match(html,/打开设置/);
assert.match(html,/social-main[^"<>]*has-selection/);
assert.doesNotMatch(html,/desktop-startup-surface/);
// Closing the error banner must not replace the retry controls with an endless blank.
ui.desktopReady=true; state.error=null;
assert.match(render(),/重试连接/);
const self={id:'human',kind:'human',name:'我',bio:'',status:'active',revision:'1',avatarRef:null};
state.snapshot={selfId:self.id,selfName:self.name,selfAccount:self,accounts:[self],addressBook:[],conversations:[]};
state.connected=true;
html=render();
assert.match(html,/主导航/);
assert.match(html,/记录持续保存/);
assert.doesNotMatch(html,/social-titlebar|重试连接/);
const WindowControls=load('components/layout/WindowControls').WindowControls;
html=renderToStaticMarkup(React.createElement(WindowControls));
assert.match(html,/class="desktop-window-controls" aria-label="窗口控制" data-tauri-drag-region="deep"/);
for (const label of ['最小化窗口','最大化窗口','关闭窗口']) assert.match(html,new RegExp(`<button[^>]*aria-label="${label}"`));
assert.equal((html.match(/data-tauri-drag-region/g)||[]).length,1,'native buttons retain their normal clickable exclusion inside the draggable gaps');
console.log('PASS: blank desktop startup, visible initialization failure, retry after dismissed error, and connected titlebar-free social shell');
