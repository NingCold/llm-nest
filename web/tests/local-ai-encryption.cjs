const assert = require('node:assert/strict');
const fs = require('node:fs'), vm = require('node:vm'), ts = require('typescript'), React = require('react');
const source = ts.transpileModule(fs.readFileSync(require.resolve('../src/cloud/LocalAiEncryption.tsx'),'utf8'), {compilerOptions:{jsx:ts.JsxEmit.ReactJSX,module:ts.ModuleKind.CommonJS,target:ts.ScriptTarget.ES2022}}).outputText;
const ai = {deviceId:'ai-device',fingerprint:'ai-ed',curve:'ai-curve',master:'ai-master',identityReady:true,recoverySaved:true,peerTrusted:false,ready:false};
function harness(options={}) {
  let cursor=0, reads=0, device='human-device'; const state=[],refs=[],effects=[],calls=[]; let refCursor=0; const exports={};
  vm.runInNewContext(source,{exports,require:name=>name.endsWith('.css')?{}:name==='react'?{...React,useState(initial){const i=cursor++;if(state[i]===undefined)state[i]=initial;return[state[i],value=>{state[i]=typeof value==='function'?value(state[i]):value;}];},useRef(initial){const i=refCursor++;return refs[i]??=( {current:initial});},useEffect(fn){if(!effects.length)effects.push(fn);}}:name==='@tauri-apps/api/core'?{invoke:async(command,args)=>{calls.push([command,args]);if(command.endsWith('_status'))return options.trusted?{...ai,peerTrusted:true,ready:false}:ai;if(command.endsWith('_export'))return options.exported??false;if(command.endsWith('_trust'))return {...ai,peerTrusted:true,ready:true};return ai;}}:name==='./crypto/ResourcePanel'?{EncryptedResourcePanel:()=>null}:name==='./crypto/engine'?{cryptoDevice:()=>device,recoveryConfirmed:()=>true,deviceStatus:async()=>{if(options.lock && ++reads>1)device=null;return {deviceId:'human-device',fingerprint:'human-ed',curve:'human-curve',master:'human-master',trusted:true};},devices:async()=>options.trusted?[{...ai,trusted:true}]:[],trustDevice:async(...args)=>{calls.push(['trustDevice',args]);if(options.trustError)throw Error('trust failed');}}:require(name)});
  return {calls,state,render(){cursor=0;refCursor=0;return exports.LocalAiEncryption({operatorId:'human',aiId:'ai',running:false,configuration:{provider:'p',model:'m'},start:async()=>{calls.push(['start']);return true;},openChat:async()=>{}});},async mount(){this.render();effects[0]();await tick();},lock(){device=null;}};
}
const tick=()=>new Promise(setImmediate);
function nodes(tree){return !tree||typeof tree!=='object'?[]:[tree,...React.Children.toArray(tree.props?.children).flatMap(nodes)];}
function text(tree){return React.Children.toArray(tree?.props?.children).map(x=>typeof x==='object'?text(x):String(x)).join('');}
function button(h,label){return nodes(h.render()).find(x=>x.type==='button'&&text(x)===label);}
(async()=>{
  const h=harness();await h.mount();assert.equal(button(h,'确认双方信任').props.disabled,true);
  button(h,'保存 AI 恢复文件').props.onClick();await tick();assert.match(text(h.render()),/已取消保存恢复文件/);assert.equal(h.state[2],false);
  const partial=harness({trustError:true});await partial.mount();partial.state[2]=true;partial.state[3]=true;button(partial,'确认双方信任').props.onClick();await tick();assert.match(text(partial.render()),/trust failed/);assert.equal(partial.state[1],false);assert.ok(partial.calls.some(c=>c[0]==='local_agent_crypto_trust'));
  const success=harness({trusted:true});await success.mount();success.state[2]=true;success.state[4]={saved:true,applied:false};assert.equal(button(success,'启动 Agent 并等待资料应用').props.disabled,false);button(success,'启动 Agent 并等待资料应用').props.onClick();await tick();assert.ok(success.calls.some(c=>c[0]==='start'));assert.ok(!button(success,'进入私聊，开启加密聊天'),'saved profile is not an applied running profile');
  const locked=harness({lock:true});await locked.mount();locked.state[2]=true;locked.state[3]=true;button(locked,'确认双方信任').props.onClick();await tick();assert.ok(!locked.calls.some(c=>c[0]==='local_agent_crypto_trust'),'late device-status must not trust after lock');
  console.log('PASS: local encrypted AI recovery cancellation, explicit confirmation, partial trust failure and late lock');
})().catch(e=>{console.error(e);process.exitCode=1;});

async function fullSetupFlow() {
  let resourceFailure=false, resourceItems=[], running=false, opened=false, trusted=false, stoppedStatus={...ai,identityReady:false,recoverySaved:false};
  const pendingEffects=[], resourceCalls=[];
  function moduleHarness(file, dependencies) {
    const slots=[];let cursor=0;
    const exports={};
    const hooks={...React,useState(initial){const i=cursor++;if(!(i in slots))slots[i]=initial;return[slots[i],v=>{slots[i]=typeof v==='function'?v(slots[i]):v;}];},useRef(initial){const i=cursor++;return slots[i]??={current:initial};},useEffect(fn,deps){const i=cursor++, old=slots[i];if(!old||deps.some((v,j)=>v!==old.deps[j])){old?.cleanup?.();const record={deps};slots[i]=record;pendingEffects.push(()=>{record.cleanup=fn();});}}};
    const code=ts.transpileModule(fs.readFileSync(require.resolve(file),'utf8'),{compilerOptions:{jsx:ts.JsxEmit.ReactJSX,module:ts.ModuleKind.CommonJS,target:ts.ScriptTarget.ES2022}}).outputText;
    vm.runInNewContext(code,{exports,localStorage:{setItem(){},getItem(){return null;}},crypto:require('node:crypto').webcrypto,setInterval:()=>1,clearInterval(){},FormData:class{constructor(form){this.data=form;}get(name){return this.data[name]??null;}},require:name=>name==='react'?hooks:name.endsWith('.css')?{}:dependencies[name]??require(name)});
    return {exports,render(fn,props){cursor=0;return fn(props);}};
  }
  const engine={cryptoDevice:()=> 'human-device',recoveryConfirmed:()=>true,deviceStatus:async()=>({deviceId:'human-device',fingerprint:'human-ed',curve:'human-curve',master:'human-master',trusted:true}),devices:async()=>trusted?[{...ai,trusted:true}]:[],trustDevice:async()=>{trusted=true;}};
  const resources={loadResources:async()=>{if(resourceFailure)throw Error('offline failure');return {items:resourceItems,offline:false};},decryptResource:async r=>r.data,pendingResources:async()=>[],resourceStatus:r=>r.applied?'节点已应用':'等待节点应用',saveResource:async(operator,body,data)=>{resourceCalls.push(body);resourceItems=[{...body,revision:'1',tombstone:null,applied:null,data}];}};
  const resource=moduleHarness('../src/cloud/crypto/ResourcePanel.tsx',{'./engine':engine,'../api':{request:async()=>({accounts:['human','ai'],revision:'1'}),server:'fixture',HttpError:class extends Error{}},'./resources':resources});
  const local=moduleHarness('../src/cloud/LocalAiEncryption.tsx',{'./crypto/engine':engine,'./crypto/ResourcePanel':resource.exports,'@tauri-apps/api/core':{invoke:async cmd=>{if(cmd.endsWith('_initialize'))stoppedStatus={...ai,recoverySaved:false};if(cmd.endsWith('_export')){stoppedStatus={...stoppedStatus,recoverySaved:true};return true;}if(cmd.endsWith('_trust'))stoppedStatus={...stoppedStatus,peerTrusted:true};return stoppedStatus;}}});
  const props=()=>({operatorId:'human',aiId:'ai',bindingVersion:'1',running,configuration:{provider:'fixture',model:'small'},start:async()=>{running=true;return true;},openChat:async()=>{opened=true;}});
  const render=()=>{let tree=local.render(local.exports.LocalAiEncryption,props());function expand(node){if(!node||typeof node!=='object')return node;if(node.type===resource.exports.EncryptedResourcePanel)return expand(resource.render(node.type,node.props));return {...node,props:{...node.props,children:React.Children.toArray(node.props?.children).map(expand)}};}return expand(tree);};
  const settle=async()=>{for(let i=0;i<3;i++){render();while(pendingEffects.length)pendingEffects.shift()();await tick();}};
  const click=async label=>{const b=nodes(render()).find(n=>n.type==='button'&&text(n)===label);assert.ok(b,label);assert.ok(!b.props.disabled,label+' enabled');b.props.onClick();await settle();};
  await settle();await click('准备首次加密身份');await click('保存 AI 恢复文件');
  let checks=nodes(render()).filter(n=>n.type==='input'&&n.props.type==='checkbox');checks[0].props.onChange({target:{checked:true}});checks[1].props.onChange({target:{checked:true}});
  await click('确认双方信任');assert.equal(stoppedStatus.ready,false,'profile is not applied yet');
  await click('设置角色与模型');
  const form=nodes(render()).find(n=>n.type==='form');form.props.onSubmit({preventDefault(){},currentTarget:{persona:'Helpful',provider:'fixture',model:'small',maxTokens:'512'}});await settle();assert.equal(resourceCalls.length,1);
  await click('启动 Agent 并等待资料应用');assert.equal(running,true);assert.ok(!nodes(render()).some(n=>n.type==='button'&&text(n)==='进入私聊，开启加密聊天'));
  resourceItems[0].applied={bindingVersion:'old',revision:'1',status:'applied'};await click('读取当前版本');assert.equal(opened,false);assert.ok(!nodes(render()).some(n=>n.type==='button'&&text(n)==='进入私聊，开启加密聊天'));
  resourceItems[0].applied={bindingVersion:'1',revision:'1',status:'applied'};await click('读取当前版本');resourceFailure=true;await click('读取当前版本');assert.ok(!nodes(render()).some(n=>n.type==='button'&&text(n)==='进入私聊，开启加密聊天'),'failed read clears old applied readiness');resourceFailure=false;await click('读取当前版本');await click('编辑当前版本');assert.ok(!nodes(render()).some(n=>n.type==='button'&&text(n)==='进入私聊，开启加密聊天'),'editing clears readiness');await click('取消未保存输入');await click('进入私聊，开启加密聊天');assert.equal(opened,true);
  console.log('PASS: first identity ready=false → recovery → mutual trust → real profile form save → start → current binding applied → chat');
  // A fresh setup dialog for a running node must not query its locked crypto DB.
  // The existing resource reader verifies its own decryption permissions.
  let runningOpened=false, nativeReads=0;
  const runningResource=moduleHarness('../src/cloud/crypto/ResourcePanel.tsx',{'./engine':engine,'../api':{request:async()=>({accounts:['human','ai'],revision:'1'}),server:'fixture',HttpError:class extends Error{}},'./resources':resources});
  const runningLocal=moduleHarness('../src/cloud/LocalAiEncryption.tsx',{'./crypto/engine':engine,'./crypto/ResourcePanel':runningResource.exports,'@tauri-apps/api/core':{invoke:async()=>{nativeReads++;throw Error('running crypto DB must not be read');}}});
  const renderRunning=()=>{const tree=runningLocal.render(runningLocal.exports.LocalAiEncryption,{...props(),running:true,openChat:async()=>{runningOpened=true;}});function expand(node){if(!node||typeof node!=='object')return node;if(node.type===runningResource.exports.EncryptedResourcePanel)return expand(runningResource.render(node.type,node.props));return {...node,props:{...node.props,children:React.Children.toArray(node.props?.children).map(expand)}};}return expand(tree);};
  const settleRunning=async()=>{for(let i=0;i<3;i++){renderRunning();while(pendingEffects.length)pendingEffects.shift()();await tick();}};
  resourceItems[0].applied={bindingVersion:'old',revision:'1',status:'applied'};
  await settleRunning();assert.equal(nativeReads,0);assert.match(text(renderRunning()),/加密角色与模型/);assert.match(text(renderRunning()),/Helpful/);assert.ok(!nodes(renderRunning()).some(n=>n.type==='button'&&text(n)==='进入私聊，开启加密聊天'),'running mount cannot open an old binding');
  resourceItems[0].applied={bindingVersion:'1',revision:'1',status:'applied'};
  nodes(renderRunning()).find(n=>n.type==='button'&&text(n)==='读取当前版本').props.onClick();await settleRunning();
  const runningChat=nodes(renderRunning()).find(n=>n.type==='button'&&text(n)==='进入私聊，开启加密聊天');assert.ok(runningChat);runningChat.props.onClick();await tick();assert.equal(runningOpened,true);assert.equal(nativeReads,0);assert.ok(!text(renderRunning()).includes('确认双方信任'),'unknown running trust must not be presented as confirmed');
  console.log('PASS: already running mount reads existing profile without native crypto query and opens chat only for current applied binding');
}
fullSetupFlow().catch(e=>{console.error(e);process.exitCode=1;});
