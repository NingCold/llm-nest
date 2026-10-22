// Exercise real reference components with controlled effects and delayed reads.
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const assert = require('node:assert/strict');
const { webcrypto } = require('node:crypto');
const ts = require('typescript');
const flush = () => new Promise(resolve => setImmediate(resolve));
const defer = () => { let resolve; const promise = new Promise(r => resolve=r); return {promise,resolve}; };
const message = {id:'source',conversationId:'group',senderId:'ai',senderName:'A',text:'selected body',images:[],files:[],status:'committed',revision:'1',sequence:'1'};
function harness(component, props) {
  const slots=[], effects=[]; let index=0; const calls=[];
  const state={ snapshot:{cursor:'0',accounts:[{id:'ai',name:'A',kind:'ai',status:'active'}],conversations:[{id:'group',memberIds:['human','ai']}]}, connected:true, histories:{group:[message]},summaries:{},summaryErrors:{},error:null,
    action:async fn=>{try{return await fn()}catch(e){state.error=String(e)}},loadSummaries:async()=>{},refresh:async()=>{},referenceDraft:(id,input)=>calls.push(['draft',id,input]) };
  const client={referencePreview:async input=>{calls.push(['preview',input]);return {title:'Group',messages:[message],summary:null}},openDirect:async()=>{calls.push(['open']);return 'unique-private'}};
  const react={
    useState(initial){const i=index++;if(!(i in slots)) slots[i]=initial;return [slots[i],value=>slots[i]=typeof value==='function'?value(slots[i]):value]},
    useEffect(fn,deps){const i=index++;const prev=slots[i];if(!prev||deps.some((d,n)=>d!==prev.deps[n])){prev?.cleanup?.();const record={deps};slots[i]=record;effects.push(()=>record.cleanup=fn());}},
  };
  const jsx=(type,props)=>({type,props}); const exports={};
  const source=fs.readFileSync(path.resolve(__dirname,'../src/social/LiveReference.tsx'),'utf8')+'\nexport { Preview, ContinueEditor };';
  vm.runInNewContext(ts.transpileModule(source,{compilerOptions:{jsx:ts.JsxEmit.ReactJSX,target:ts.ScriptTarget.ES2022,module:ts.ModuleKind.CommonJS}}).outputText,{exports,crypto:webcrypto,require:name=>{
    if(name==='react') return react;
    if(name==='react/jsx-runtime') return {jsx,jsxs:jsx};
    if(name==='./live-store') return {useLiveSocialStore:()=>state};
    if(name==='./client') return {socialClient:client};
    if(name==='./router') return {navigate:(...args)=>calls.push(['navigate',...args])};
    return {};
  }});
  const render=()=>{index=0;const tree=exports[component](props);while(effects.length)effects.shift()();return tree};
  function all(n){return !n||typeof n!=='object'?[]:[n,...[n.props?.children].flat(Infinity).flatMap(all)]}
  return {render,nodes:()=>all(render()),state,client,calls,props};
}
(async()=>{
  const input={actorId:'ai',selection:{conversationId:'group',sources:[{kind:'message',conversation_id:'group',message_id:'source',revision:'1'}]}};
  const h=harness('Preview',{input});
  const old=defer();h.client.referencePreview=()=>old.promise;
  h.render();h.state.snapshot.cursor='1';
  const current=defer();h.client.referencePreview=()=>current.promise;h.render();
  current.resolve({title:'Group',messages:[{...message,text:'current body'}],summary:null});await flush();
  assert.match(JSON.stringify(h.render()),/current body/);
  old.resolve({title:'Group',messages:[{...message,text:'obsolete body'}],summary:null});await flush();
  assert.doesNotMatch(JSON.stringify(h.render()),/obsolete body/,'late previews cannot restore revoked content');
  h.state.connected=false;assert.doesNotMatch(JSON.stringify(h.render()),/current body/);
  h.state.connected=true;h.props.stale=true;assert.doesNotMatch(JSON.stringify(h.render()),/current body/);
  assert.match(JSON.stringify(h.render()),/引用已失效/);

  const e=harness('ContinueEditor',{conversation:{id:'group'},message,messages:[message],done(){}});
  assert.equal(e.calls.length,0);
  e.nodes().find(n=>n.type==='button'&&n.props.children==='带入私聊草稿').props.onClick();await flush();
  assert.deepEqual(e.calls.map(c=>c[0]),['preview','open','draft','navigate']);
  assert.equal(e.calls[2][1],'unique-private');
  assert.equal(e.calls[2][2].selection.sources[0].revision,'1');
  const located={...message,id:'older-source',sequence:'0',text:'older located message'};
  const search=harness('ContinueEditor',{conversation:{id:'group'},message:located,messages:[located,message],done(){}});
  search.state.histories.group=[message];
  const sourceChoice=search.nodes().find(n=>n.type==='input'&&n.props.checked);
  assert.ok(sourceChoice,'search-located source remains selectable even outside latest history');
  search.nodes().find(n=>n.type==='button'&&n.props.children==='带入私聊草稿').props.onClick();await flush();
  assert.equal(search.calls[0][1].selection.sources[0].message_id,'older-source');
  const denied=harness('ContinueEditor',{conversation:{id:'group'},message,messages:[message],done(){}});
  denied.client.referencePreview=async()=>{throw new Error('source revoked')};
  denied.nodes().find(n=>n.type==='button'&&n.props.children==='带入私聊草稿').props.onClick();await flush();
  assert.equal(denied.calls.length,0,'failed authorization never opens a private chat or replaces a draft');
  console.log('PASS: explicit continuation prepares only a draft, checks source first, and preview discards late, offline and invalidated content');
})().catch(error=>{console.error(error);process.exitCode=1});
