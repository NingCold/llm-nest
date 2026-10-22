// Run the real rule editor and retry hook with lost acknowledgements.
const fs=require('node:fs'),path=require('node:path'),vm=require('node:vm'),assert=require('node:assert/strict');
const {webcrypto}=require('node:crypto'),ts=require('typescript');
const flush=()=>new Promise(resolve=>setImmediate(resolve));
function harness(kind='group') {
  const slots=[],calls=[],modules={};let index=0,done=0,fail=true;
  const react={useState(initial){const i=index++;if(!(i in slots))slots[i]=initial;return[slots[i],v=>slots[i]=typeof v==='function'?v(slots[i]):v]},useRef(initial){const i=index++;return slots[i]??={current:initial}}};
  const jsx=(type,props)=>({type,props});
  const state={error:null,refresh:async()=>{},action:async fn=>{try{return await fn()}catch(e){state.error=String(e)}}};
  const client={updateParticipation:async(id,input)=>{calls.push({id,input});if(fail)throw new Error('lost acknowledgement');return{ok:true}}};
  function load(name){
    if(modules[name])return modules[name];const exports={};modules[name]=exports;
    let code=fs.readFileSync(path.resolve(__dirname,`../src/social/${name}.tsx`),'utf8');
    if(name==='LiveParticipation')code+='\nexport { ParticipationForm };';
    vm.runInNewContext(ts.transpileModule(code,{compilerOptions:{jsx:ts.JsxEmit.ReactJSX,target:ts.ScriptTarget.ES2022,module:ts.ModuleKind.CommonJS}}).outputText,{exports,crypto:webcrypto,require:name=>{
      if(name==='react')return react;if(name==='react/jsx-runtime')return{jsx,jsxs:jsx};
      if(name==='./live-store')return{useLiveSocialStore:()=>state};if(name==='./LiveControls')return load('LiveControls');
      if(name==='./client')return{socialClient:client,SocialApiError:class extends Error{}};return{};
    }});return exports;
  }
  const policy={revision:'7',mergeDelayMs:800,idleSeconds:300,limitedAuto:false,maxConsecutive:3,maxAutoPerBout:12,maxAutoPerDay:48,dailyTokenBudget:262144};
  const {ParticipationForm}=load('LiveParticipation');
  const render=()=>{index=0;return ParticipationForm({conversation:{id:'chat',kind},initial:policy,done:()=>done++})};
  const all=n=>!n||typeof n!=='object'?[]:[n,...[n.props?.children].flat(Infinity).flatMap(all)];
  return{render,nodes:()=>all(render()),calls,state,done:()=>done,succeed:()=>fail=false};
}
(async()=>{
  const h=harness();const toggle=()=>h.nodes().find(n=>n.type==='input'&&n.props.type==='checkbox');
  assert.equal(toggle().props.checked,false,'automatic participation is opt-in');
  toggle().props.onChange({target:{checked:true}});
  h.render().props.onSubmit({preventDefault(){}});await flush();
  assert.equal(h.done(),0);assert.equal(toggle().props.checked,true);assert.match(h.state.error,/lost acknowledgement/);
  h.succeed();h.render().props.onSubmit({preventDefault(){}});await flush();
  assert.equal(h.calls[0].input.requestId,h.calls[1].input.requestId);
  assert.equal(h.calls[1].input.policy.revision,'7');assert.equal(h.calls[1].id,'chat');assert.equal(h.done(),1);
  const direct=harness('direct');assert(!direct.nodes().some(n=>n.type==='input'&&n.props.type==='checkbox'));
  assert(direct.nodes().some(n=>n.type==='input'&&n.props.value===800));
  console.log('PASS: participation consent defaults off, rules retain failed edits and retry identity, policy revision is explicit, private chats expose only private rhythm controls');
})().catch(e=>{console.error(e);process.exitCode=1});
