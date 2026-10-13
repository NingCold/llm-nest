const fs=require('node:fs'),path=require('node:path'),vm=require('node:vm'),ts=require('typescript');
const {webcrypto}=require('node:crypto');
const {IDBFactory}=require('fake-indexeddb');
const compile=name=>ts.transpileModule(fs.readFileSync(path.resolve(__dirname,'../src/social',name+'.ts'),'utf8'),{compilerOptions:{target:ts.ScriptTarget.ES2022,module:ts.ModuleKind.CommonJS}}).outputText;
const snapshot=cursor=>({selfId:'human',selfName:'Human',selfAccount:{id:'human',name:'Human',kind:'human',bio:'',avatarRef:null,status:'active',revision:'1'},accounts:[],addressBook:[],conversations:[],models:[],cursor});
const message=(text,revision='1',status='committed')=>({id:'message',conversationId:'chat',senderId:'human',senderName:'Human',sequence:'1',text,status,revision,replyTo:null,runId:null,createdAt:'2026-10-09T00:00:00Z'});
function harness(options={}) {
  const factory=options.factory??new IDBFactory();
  const values=options.values??new Map([['llm-nest-social-preview-drafts-v1',JSON.stringify({chat:{text:'preview-only'}})]]);
  const storage={getItem:key=>values.get(key)??null,setItem:(key,value)=>values.set(key,value),removeItem:key=>values.delete(key)};
  const globals={crypto:webcrypto,Blob,File,URL,Uint8Array,atob,btoa,indexedDB:factory,localStorage:storage,AbortController,Map,BigInt,Date,setTimeout};
  const types={},persistence={},images={},processing={};
  vm.runInNewContext(compile('images'),{...globals,exports:images});
  vm.runInNewContext(compile('image-processing'),{...globals,exports:processing});
  vm.runInNewContext(compile('drafts'),{...globals,exports:types});
  vm.runInNewContext(compile('draft-storage'),{...globals,exports:persistence});
  const database=persistence.draftDatabase;
  let handlers;
  const api={init:async()=>snapshot('0'),messages:async()=>({messages:[],hasMore:false}),message:async()=>message(''),runDetails:async()=>null,send:async(_,input)=>({message:message(input.text),executionNotice:null})};
  const exports={};
  vm.runInNewContext(compile('live-store'),{...globals,exports,require:spec=>{
    if(spec==='./drafts')return types;
    if(spec==='./draft-storage')return {draftDatabase:database};
    if(spec==='./images')return {...images,readImages:options.readImages??(async()=>{throw new Error('unexpected file read')})};
    if(spec==='./image-processing')return {...processing,...options.processing};
    if(spec==='./client')return {socialClient:api,subscribe:(_,signal,change,live,resync)=>{handlers={signal,change,live,resync};return new Promise(()=>{})}};
    if(spec==='zustand')return {create:initialize=>{let state;const hook=selector=>selector(state);const set=update=>{state={...state,...(typeof update==='function'?update(state):update)}};hook.getState=()=>state;hook.setState=set;state=initialize(set,hook.getState);return hook}};
    throw new Error('unexpected dependency: '+spec);
  }});
  return {store:exports.useLiveSocialStore,images,processing,api,storage,values,factory,database,types,persistence,handlers:()=>handlers,flush:exports.flushDrafts,unsaved:exports.hasUnsavedDrafts,
    async record(id,owner='human'){return (await new persistence.DraftDatabase(factory,storage).load(owner,[id]))[id]}};
}
module.exports={harness,snapshot,message,compile};
