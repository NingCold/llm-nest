// Run the real rule editor and retry hook with lost acknowledgements.
const fs=require('node:fs'),path=require('node:path'),vm=require('node:vm'),assert=require('node:assert/strict');
const {webcrypto}=require('node:crypto'),ts=require('typescript');
const flush=()=>new Promise(resolve=>setImmediate(resolve));
function harness(form='SettingsForm') {
  const slots=[],calls=[],modules={};let index=0,done=0,fail=true;
  const react={useState(initial){const i=index++;if(!(i in slots))slots[i]=initial;return[slots[i],v=>slots[i]=typeof v==='function'?v(slots[i]):v]},useRef(initial){const i=index++;return slots[i]??={current:initial}}};
  const jsx=(type,props)=>({type,props});
  const state={error:null,refresh:async()=>{},action:async fn=>{try{return await fn()}catch(e){state.error=String(e)}}};
  const client={updateContactSettings:async(id,input)=>{calls.push({id,input});if(fail)throw new Error('lost acknowledgement');return{ok:true}}};
  function load(name){
    if(modules[name])return modules[name];const exports={};modules[name]=exports;
    let code=fs.readFileSync(path.resolve(__dirname,`../src/social/${name}.tsx`),'utf8');
    if(name==='LiveContactSchedule')code+='\nexport { SettingsForm, IntentForm };';
    vm.runInNewContext(ts.transpileModule(code,{compilerOptions:{jsx:ts.JsxEmit.ReactJSX,target:ts.ScriptTarget.ES2022,module:ts.ModuleKind.CommonJS}}).outputText,{exports,crypto:webcrypto,require:name=>{
      if(name==='react')return react;if(name==='react/jsx-runtime')return{jsx,jsxs:jsx};
      if(name==='./live-store')return{useLiveSocialStore:()=>state};if(name==='./LiveControls')return load('LiveControls');
      if(name==='./client')return{socialClient:client,SocialApiError:class extends Error{}};return{};
    }});return exports;
  }
  const initial={revision:'7',enabled:false,quietUntil:null,quietAction:'defer'};
  state.snapshot={addressBook:[],accounts:[{id:'ai',name:'test',kind:'ai',status:'active'}]};
  client.createContactIntent=async(id,input)=>{calls.push({id,input});if(fail)throw new Error('lost acknowledgement');return{ok:true}};
  const mod=load('LiveContactSchedule');
  const render=()=>{index=0;return mod[form]({conversation:{id:'chat',members:[{actorId:'ai',paused:false}]},initial,done:()=>done++})};
  const all=n=>!n||typeof n!=='object'?[]:[n,...[n.props?.children].flat(Infinity).flatMap(all)];
  return{mod,render,nodes:()=>all(render()),calls,state,done:()=>done,succeed:()=>fail=false};
}

(async()=>{
  const h=harness();const toggle=()=>h.nodes().find(n=>n.type==='input'&&n.props.type==='checkbox');
  assert.equal(toggle().props.checked,false);
  toggle().props.onChange({target:{checked:true}});
  h.render().props.onSubmit({preventDefault(){}});await flush();assert.equal(h.done(),0);
  h.succeed();h.render().props.onSubmit({preventDefault(){}});await flush();
  assert.equal(h.calls[0].input.requestId,h.calls[1].input.requestId);
  assert.equal(h.calls[1].input.settings.revision,'7');assert.equal(h.calls[1].input.settings.enabled,true);
  const c=harness('IntentForm');c.nodes().find(n=>n.type==='textarea').props.onChange({target:{value:'reading'}});
  c.render().props.onSubmit({preventDefault(){}});await flush();assert.equal(c.done(),0);
  c.succeed();c.render().props.onSubmit({preventDefault(){}});await flush();
  assert.equal(JSON.stringify(c.calls[0]),JSON.stringify(c.calls[1]));assert.equal(c.calls[1].input.actorId,'ai');assert.equal(c.calls[1].id,'chat');
  assert.match(c.calls[1].input.dueAt,/[+-]\d{2}:\d{2}$/);assert.equal(c.calls[1].input.text,'reading');assert.equal(c.done(),1);
  c.state.snapshot.addressBook=[{accountId:'ai',blocked:true}];
  assert.equal(c.nodes().filter(n=>n.type==='option').length,0);
  assert.equal(c.nodes().find(n=>n.type==='button'&&n.props.className==='social-button primary').props.disabled,true);
  assert.throws(()=>c.mod.explicitTime('invalid'));
  assert.equal(c.mod.offsetLabel(345),'UTC+05:45');assert.equal(c.mod.offsetLabel(-210),'UTC-03:30');
  const old=process.env.TZ;process.env.TZ='America/New_York';
  assert.throws(()=>c.mod.explicitTime('2026-03-08T02:30'),'reject silently shifted DST gap');
  assert.equal(c.mod.explicitTime('2026-07-01T09:00'),'2026-07-01T09:00:00-04:00');
  if(old===undefined)delete process.env.TZ;else process.env.TZ=old;
  console.log('PASS: contact opt-in, failed-form retention and idempotent retry, explicit offset, DST gap rejection and independent window/AI selection');
})().catch(e=>{console.error(e);process.exitCode=1});
