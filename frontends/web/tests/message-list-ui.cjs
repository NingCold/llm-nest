const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const assert = require('node:assert/strict');
const ts = require('typescript');
const root = path.resolve(__dirname, '../src/social');
const compile = (name, extra='') => ts.transpileModule(fs.readFileSync(path.join(root,name),'utf8')+extra,{compilerOptions:{jsx:ts.JsxEmit.ReactJSX,target:ts.ScriptTarget.ES2022,module:ts.ModuleKind.CommonJS}}).outputText;
const flush = () => new Promise(resolve=>setImmediate(resolve));
const defer = () => { let resolve,reject; const promise=new Promise((a,b)=>{resolve=a;reject=b});return {promise,resolve,reject}; };
const helpers={};vm.runInNewContext(compile('message-list.ts'),{exports:helpers});
const conversation=(id,position,pinned=false)=>({id,listState:{pinned,latestDelivery:position,readThrough:'0',markedUnread:false,unreadCount:0,revision:'1'}});
const a=conversation('a','9007199254740993'),b=conversation('b','9007199254740992'),c=conversation('c','1',true);
assert.equal(helpers.orderConversations([b,a,c]).map(c=>c.id).join(','),'c,a,b');
assert.equal(helpers.canAutoRead(false,true,false,'2',a),false);
assert.equal(helpers.canAutoRead(true,false,false,'2',a),false);
assert.equal(helpers.canAutoRead(true,true,true,'2',a),false);
assert.equal(helpers.canAutoRead(true,true,false,undefined,a),false);
assert.equal(helpers.canAutoRead(true,true,false,'2',a),true);

function hooks() {
  const slots=[],effects=[],timers=new Map();let index=0,nextTimer=0;
  const react={
    useState(initial){const i=index++;if(!(i in slots))slots[i]=typeof initial==='function'?initial():initial;return [slots[i],v=>slots[i]=typeof v==='function'?v(slots[i]):v]},
    useEffect(fn,deps){const i=index++;const prev=slots[i];if(!prev||deps.some((d,n)=>d!==prev.deps[n])){prev?.cleanup?.();const record={deps};slots[i]=record;effects.push(()=>record.cleanup=fn())}},
  };
  return {react,render(fn){index=0;const result=fn();while(effects.length)effects.shift()();return result},
    setTimeout(fn){timers.set(++nextTimer,fn);return nextTimer},clearTimeout(id){timers.delete(id)},tick(){const todo=[...timers.values()];timers.clear();todo.forEach(fn=>fn())},
    close(){slots.forEach(s=>s?.cleanup?.())}};
}
function all(n){return !n||typeof n!=='object'?[]:[n,...[n.props?.children].flat(Infinity).flatMap(all)]}

(async()=>{
  const h=hooks(),events={},writes=[];let focused=true,atLatest=true;
  const state={historyReadThrough:{a:'2'},loadingHistory:{a:false},error:'消息已保存；请配置模型',action:(fn,clearError=true)=>{if(clearError)state.error=null;return fn()},updateList:async(id,input)=>writes.push([id,input])};
  const doc={visibilityState:'visible',hasFocus:()=>focused,addEventListener:(n,f)=>events[n]=f,removeEventListener:n=>delete events[n]};
  const out={};vm.runInNewContext(compile('useConversationRead.ts'),{exports:out,document:doc,window:doc,setTimeout:h.setTimeout,clearTimeout:h.clearTimeout,require:name=>name==='react'?h.react:name==='./live-store'?{useLiveSocialStore:()=>state}:helpers});
  const render=()=>h.render(()=>out.useConversationRead(a,atLatest));
  render();focused=false;events.blur();render();h.tick();assert.equal(writes.length,0,'blur cancels a pending read acknowledgement');
  focused=true;events.focus();atLatest=false;render();h.tick();assert.equal(writes.length,0,'search and older history never mark the window read');
  atLatest=true;render();h.tick();await flush();assert.equal(writes.length,1);assert.equal(writes[0][1].through,'2');assert.equal(state.error,'消息已保存；请配置模型','automatic read acknowledgement keeps visible execution notices');
  a.listState={...a.listState,revision:'2',readThrough:'2'};render();h.tick();assert.equal(writes.length,1,'the saved watermark cannot cause an acknowledgement loop');h.close();

  const s=hooks(),calls=[],client={searchMessages:input=>{const d=defer();calls.push({input,...d});return d.promise}},exp={};
  const jsx=(type,props)=>({type,props});
  vm.runInNewContext(compile('ConversationList.tsx','\nexport { SearchResults };'),{exports:exp,setTimeout:s.setTimeout,clearTimeout:s.clearTimeout,require:name=>name==='react'?s.react:name==='react/jsx-runtime'?{jsx,jsxs:jsx}:name==='./client'?{socialClient:client}:name==='./message-list'?helpers:{}});
  const props={query:'old',scope:'',snapshot:{conversations:[]}};
  const search=()=>s.render(()=>exp.SearchResults(props));
  search();s.tick();props.query='新的中文';search();s.tick();
  calls[1].resolve({messages:[],nextBefore:'50'});await flush();search();
  calls[0].resolve({messages:[{id:'obsolete',text:'obsolete'}],nextBefore:null});await flush();assert.doesNotMatch(JSON.stringify(search()),/obsolete/);
  all(search()).find(n=>n.type==='button'&&n.props.children==='更多结果').props.onClick();search();s.tick();
  assert.equal(calls[2].input.before,'50');calls[2].reject(new Error('search disconnected'));await flush();search();
  all(search()).find(n=>n.type==='button'&&n.props.children==='重试搜索').props.onClick();search();s.tick();
  assert.equal(calls[3].input.before,'50','retry retains the failed page cursor');
  s.close();calls[3].resolve({messages:[{id:'unmounted',text:'unmounted'}],nextBefore:null});await flush();
  assert.doesNotMatch(JSON.stringify(search()),/unmounted/,'late reads cannot update an unmounted search');
  console.log('PASS: persistent-list ordering, large counters, focused/bottom-only read acknowledgements, blur cancellation, no read loop, stale searches and retry pagination');
})().catch(error=>{console.error(error);process.exitCode=1});
