// Exercise committed sends, retry identities, cursor application and stream gaps.
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const assert = require('node:assert/strict');
const { webcrypto } = require('node:crypto');
const ts = require('typescript');
const root = path.resolve(__dirname, '../src/social');
const compile = name => ts.transpileModule(fs.readFileSync(path.join(root, name + '.ts'), 'utf8'), { compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.CommonJS } }).outputText;
const defer = () => { let resolve, reject; const promise = new Promise((a,b) => { resolve=a; reject=b; }); return { promise,resolve,reject }; };
const snapshot = cursor => ({ selfId:'human',selfName:'Human',contacts:[],conversations:[],models:[],cursor });
const message = (text, revision='1', status='committed') => ({ id:'message',conversationId:'chat',senderId:'human',senderName:'Human',sequence:'1',text,status,revision,replyTo:null,runId:null,createdAt:'2026-10-09T00:00:00Z' });

function harness() {
  const values = new Map([['llm-nest-social-preview-drafts-v1', JSON.stringify({ chat:{ text:'preview-only' } })]]);
  const storage = { getItem:key=>values.get(key)??null, setItem:(key,value)=>values.set(key,value) };
  let handlers;
  const api = { init:async()=>snapshot('0'), messages:async()=>({ messages:[],hasMore:false }),message:async()=>message(""),runDetails:async()=>null,send:async(_,input)=>({ message:message(input.text),executionNotice:null }) };
  const exports={};
  vm.runInNewContext(compile('live-store'), {
    exports,require:spec=> {
      if (spec==='./client') return { socialClient:api,subscribe:(_,signal,change,live,resync)=> { handlers={ signal,change,live,resync }; return new Promise(()=>{}); } };
      if (spec==='zustand') return { create:initialize=> {
        let state;
        const hook=selector=>selector(state);
        const set=update=> { state={ ...state,...(typeof update==='function'?update(state):update) }; };
        hook.getState=()=>state; hook.setState=set; state=initialize(set,hook.getState); return hook;
      } };
      throw new Error('unexpected dependency: '+spec);
    },localStorage:storage,crypto:webcrypto,AbortController,Map,BigInt,Date,setTimeout,
  });
  return { store:exports.useLiveSocialStore, api,storage,values,handlers:()=>handlers,key:exports.LIVE_DRAFT_KEY };
}

(async()=> {
  const h=harness();
  assert.equal(h.store.getState().drafts.chat,undefined,'preview drafts are isolated');
  await h.store.getState().initialize();
  const sends=[];
  h.api.send=async(_,input)=> { sends.push(input); throw new Error('lost acknowledgement'); };
  h.store.getState().targetDraft('chat',['bob','alice'],'quoted-message');
  h.store.getState().updateDraft('chat','first');
  await h.store.getState().send('chat');
  const retryId=h.store.getState().drafts.chat.requestId;
  assert.equal(h.store.getState().drafts.chat.text,'first');
  assert.equal(JSON.parse(h.values.get(h.key)).chat.requestId,retryId);
  h.api.send=async(_,input)=> { sends.push(input); return { message:message(input.text),executionNotice:{ code:'model_unavailable',message:'saved without a model' } }; };
  await h.store.getState().send('chat');
  assert.equal(sends[1].requestId,sends[0].requestId);
  assert.deepEqual(Array.from(sends[1].mentions),['bob','alice']);
  assert.equal(sends[1].replyTo,'quoted-message');
  assert.equal(JSON.stringify(sends[1]),JSON.stringify(sends[0]),'structured targets and reply identity survive retry');
  assert.equal(h.store.getState().drafts.chat.text,'');
  assert.equal(h.store.getState().drafts.chat.mentions.length,0);
  assert.equal(h.store.getState().drafts.chat.replyTo,null);
  assert.equal(h.store.getState().histories.chat[0].text,'first');
  assert.equal(h.store.getState().error,'saved without a model');

  const pending=defer(); let count=0;
  h.api.send=(_,input)=> { count++; return pending.promise; };
  h.store.getState().updateDraft('chat','old draft');
  const send=h.store.getState().send('chat');
  await h.store.getState().send('chat'); assert.equal(count,1);
  h.store.getState().updateDraft('chat','new draft while awaiting acknowledgement');
  h.store.getState().targetDraft('chat',['carol'],'new-quote');
  h.api.init=async()=> { throw new Error('refresh disconnected'); };
  pending.resolve({ message:{ ...message('old draft'),id:'second',sequence:'2' },executionNotice:null });
  await send;
  assert.equal(h.store.getState().drafts.chat.text,'new draft while awaiting acknowledgement');
  assert.deepEqual(Array.from(h.store.getState().drafts.chat.mentions),['carol']);
  assert.equal(h.store.getState().drafts.chat.replyTo,'new-quote');
  assert.ok(h.store.getState().histories.chat.some(m=>m.id==='second'),'a committed acknowledgement survives refresh failure');
  h.api.init=async()=>snapshot('2');

  let reads=0;
  h.api.message=async()=> { reads++; if(reads===1)throw new Error('state reload failed'); return message('final','2'); };
  const event={ cursor:'2',change:{ kind:'message_changed',conversationId:'chat',messageId:'message',revision:'2' } };
  await assert.rejects(h.handlers().change(event),/state reload failed/);
  await h.handlers().change(event);
  assert.equal(reads,2,'cursor advances only after state application succeeds');
  assert.equal(h.store.getState().histories.chat[0].text,'final');
  h.api.messages=async()=>({ messages:[message('stale','1','generating')],hasMore:false });
  await h.store.getState().loadHistory('chat');
  assert.equal(h.store.getState().histories.chat[0].status,'committed','late drafts cannot overwrite the final message');
  const older = { ...message('old text'), id:'older', sequence:'1' };
  h.store.setState({ histories: { ...h.store.getState().histories, chat:[older, { ...message('latest'),id:'latest',sequence:'301' }] } });
  h.api.message=async()=>({ ...older, text:'', revision:'2', status:'withdrawn' });
  await h.handlers().change({ cursor:'3',change:{ kind:'message_changed',conversationId:'chat',messageId:'older',revision:'2' } });
  assert.equal(h.store.getState().histories.chat[0].text,'','older loaded messages are refreshed by ID beyond the latest page');
  assert.equal(h.store.getState().histories.chat[0].status,'withdrawn');
  assert.equal(h.store.getState().histories.chat[1].id,'latest','revision updates preserve subsequent history');
  h.store.getState().applyMessage(older);
  assert.equal(h.store.getState().histories.chat[0].status,'withdrawn','late acknowledgements cannot revive withdrawn content');
  const staleHistory = defer();
  h.api.messages=()=>staleHistory.promise;
  const staleRead=h.store.getState().loadHistory('chat',true);
  h.api.init=async()=>snapshot('3');
  h.api.messages=async()=>({ messages:[{ ...message('latest'),id:'latest',sequence:'301' }],hasMore:true });
  await h.handlers().resync();
  staleHistory.resolve({ messages:[older],hasMore:false }); await staleRead;
  assert.equal(h.store.getState().histories.chat.length,1,'resync discards old loaded pages and pre-resync read responses');
  assert.equal(h.store.getState().histories.chat[0].id,'latest');



  const live=(deltaSeq,text)=>({ type:'generation.delta',conversationId:'other',runId:'run',messageId:'live-message',actorId:'ai',deltaSeq,text });
  h.handlers().live(live(7,'gap')); assert.equal(h.store.getState().generations['live-message'],undefined);
  h.handlers().live(live(1,'A')); h.handlers().live(live(2,'B'));
  assert.equal(h.store.getState().generations['live-message'].text,'AB');
  h.handlers().live(live(4,'missing third fragment')); assert.equal(h.store.getState().generations['live-message'],undefined);
  assert.equal(h.values.get('llm-nest-social-preview-drafts-v1'),JSON.stringify({ chat:{ text:'preview-only' } }));

  const exports={}; let requestHeaders;
  const frames=': ping\r\n\r\nevent: change\r\ndata: '+JSON.stringify({ ...event,cursor:'9007199254740993' })+'\r\n\r\nevent: generation\r\ndata: '+JSON.stringify(live(1,'你好🙂'))+'\r\n\r\nevent: resync\r\ndata: {}\r\n\r\n';
  const bytes=new TextEncoder().encode(frames);
  vm.runInNewContext(compile('http'),{
    exports,Headers,TextDecoder,require:()=>({SocialApiError: class extends Error { constructor(message,code) {super(message);this.code=code;} }}),
    fetch:async(_,init)=> { requestHeaders=init.headers; return new Response(new ReadableStream({ start(controller) { for(let i=0;i<bytes.length;i+=3)controller.enqueue(bytes.slice(i,i+3));controller.close(); } }),{ status:200 }); },
  });
  const applied=[];
  await assert.rejects(exports.subscribe('0',new AbortController().signal,async e=>{ await Promise.resolve(); applied.push(e.cursor); },e=>applied.push(e.text),async()=>applied.push('resync'),()=>{}),/消息订阅已断开/);
  assert.deepEqual(applied,['9007199254740993','你好🙂','resync']);
  assert.equal(requestHeaders.get('X-LLMN-Client'),'1');
  const previous = h.handlers();
  h.store.getState().disconnect();
  assert.equal(previous.signal.aborted, true);
  previous.live(live(1,'late old subscription'));
  assert.equal(h.store.getState().generations['live-message'],undefined);
  let lateReads=0;
  h.api.init=async()=> { lateReads++; return snapshot('99'); };
  await previous.change({ cursor:'99',change:{ kind:'actor_changed',actorId:'ai',revision:'1' } });
  assert.equal(lateReads,0,'released subscriptions cannot read state or advance cursor');
  await h.store.getState().initialize();
  assert.equal(h.handlers().signal.aborted,false,'root remount creates a fresh subscription');
  console.log('PASS: live social structured mentions/replies, retry identity, committed acknowledgements, independent drafts, cursor application, terminal revisions, older message refresh/withdrawal, stale-page resync, Unicode SSE and local headers');
})().catch(error=> { console.error(error);process.exitCode=1; });
