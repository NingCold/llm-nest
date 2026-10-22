const fs=require('node:fs'),path=require('node:path'),vm=require('node:vm'),assert=require('node:assert/strict'),ts=require('typescript');
const {webcrypto}=require('node:crypto');const flush=()=>new Promise(resolve=>setImmediate(resolve));
const slots=[],calls=[];let index=0,mode='lost';
class SocialApiError extends Error {}
const conversation={id:'group',revision:'1',memberIds:['human','ai'],members:[{actorId:'human'},{actorId:'ai'}]};
const state={snapshot:{selfId:'human',selfName:'H',accounts:[{id:'ai',kind:'ai',name:'A',status:'active'},{id:'second',kind:'ai',name:'B',status:'active'}]},refresh:async()=>{},action:async fn=>{try{return await fn()}catch(error){state.error=String(error)}}};
const client={changeMembership:async(id,input)=>{calls.push({id,input});if(mode==='lost')throw new Error('lost acknowledgement');if(mode==='conflict')throw new SocialApiError('revision conflict');return{}}};
const react={useState(initial){const i=index++;if(!(i in slots))slots[i]=initial;return[slots[i],value=>slots[i]=typeof value==='function'?value(slots[i]):value]},useRef(initial){const i=index++;return slots[i]??={current:initial}}};
const jsx=(type,props)=>({type,props}),modules={};
function load(name){const exports={};const code=ts.transpileModule(fs.readFileSync(path.resolve(__dirname,'../src/social',name+'.tsx'),'utf8'),{compilerOptions:{jsx:ts.JsxEmit.ReactJSX,target:ts.ScriptTarget.ES2022,module:ts.ModuleKind.CommonJS}}).outputText;vm.runInNewContext(code,{exports,crypto:webcrypto,require:name=>name==='react'?react:name==='react/jsx-runtime'?{jsx,jsxs:jsx}:name==='./live-store'?{useLiveSocialStore:()=>state}:name==='./client'?{socialClient:client,SocialApiError}:name==='./LiveControls'?modules.controls:name==='./live-ui'?{contactAvatar:()=>undefined}:{}});return exports}
modules.controls=load('LiveControls');const groups=load('GroupControls');
const all=n=>!n||typeof n!=='object'?[]:[n,...[n.props?.children].flat(Infinity).flatMap(all)];const nodes=()=>{index=0;return all(groups.GroupMembers({conversation}))};const remove=()=>nodes().find(n=>n.props?.['aria-label']==='移出 A').props.onClick();
(async()=>{
 remove();await flush();conversation.revision='2';remove();await flush();assert.deepEqual(calls[0],calls[1],'lost acknowledgement retries exact request/version');
 mode='conflict';remove();await flush();conversation.revision='3';mode='success';remove();await flush();assert.equal(calls.at(-1).input.expectedRevision,'3');assert.notEqual(calls.at(-1).input.requestId,calls[0].input.requestId,'explicit API rejection allows refreshed revision retry');
 console.log('PASS: group membership preserves lost-receipt request and resets rejected revisions for retry');
})().catch(error=>{console.error(error);process.exitCode=1});
