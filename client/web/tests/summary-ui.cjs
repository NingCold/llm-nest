// Render the production summary view: inactive material must never reappear as context.
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const assert = require('node:assert/strict');
const React = require('react');
const { renderToStaticMarkup } = require('react-dom/server');
const ts = require('typescript');
const root = path.resolve(__dirname, '../src/social');
const segment = { id: 'segment', fromSequence: '1', throughSequence: '16', text: 'verified-summary-text', status: 'ready', revision: '3', attempt: '1', sources: [], binding: {modelId:'summary-model'}, updatedAt: '2026-10-13T00:00:00Z' };
const state = { snapshot:{cursor:'0',models:[]}, summaries:{ chat:{ settings:{enabled:true,revision:'1',binding:{modelId:'summary-model'}},segments:[segment],hasMore:false } }, loadingSummaries:{}, summaryErrors:{}, error:null };
const modules = {};
function load(name) {
  if (modules[name]) return modules[name];
  const exports = {}; modules[name] = exports;
  const source = ts.transpileModule(fs.readFileSync(path.join(root,name+'.tsx'),'utf8'), {compilerOptions:{jsx:ts.JsxEmit.ReactJSX,target:ts.ScriptTarget.ES2022,module:ts.ModuleKind.CommonJS}}).outputText;
  vm.runInNewContext(source,{exports,require:spec=>{
    if(spec==='./live-store') return {useLiveSocialStore:()=>state};
    if(spec==='./LiveControls') return load('LiveControls');
    if(spec==='./client') return {socialClient:{},SocialApiError:class extends Error{}};
    if(spec==='@/components/ui/dialog') return {Dialog:()=>null,DialogContent:()=>null,DialogDescription:()=>null,DialogTitle:()=>null};
    return require(spec);
  }});
  return exports;
}
const { SummaryPanel } = load('LiveSummary');
const conversation = { id:'chat',kind:'group',status:'active',legacy:false };
const render = () => renderToStaticMarkup(React.createElement(SummaryPanel,{conversation}));
assert.match(render(),/verified-summary-text/);
assert.match(render(),/本群当前成员共享/);
assert.match(render(),/不等同于已确认记忆/);
for(const status of ['stale','failed','interrupted','running','queued']) {
  segment.status=status;
  assert.doesNotMatch(render(),/verified-summary-text/,'only ready material has a visible body');
}
segment.status='stale';
assert.match(render(),/按当前原文重建/);
state.summaries.chat.settings.enabled=false;
assert.match(render(),/disabled="">按当前原文重建/);
state.summaryErrors.chat='read failed';state.summaries.chat=null;
assert.match(render(),/重新读取摘要/);
assert.doesNotMatch(render(),/verified-summary-text/);
console.log('PASS: summary view shares window scope, renders only valid text, explains lossy material and disables retry until enabled');
