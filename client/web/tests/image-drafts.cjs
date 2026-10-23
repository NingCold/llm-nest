// Use the real draft repository and social store with an IndexedDB implementation.
const assert=require('node:assert/strict'),vm=require('node:vm'),fs=require('node:fs'),path=require('node:path'),ts=require('typescript');
const {harness,snapshot,message}=require('./draft-harness.cjs');
const defer=()=>{let resolve,reject;const promise=new Promise((a,b)=>{resolve=a;reject=b});return {promise,resolve,reject}};
const tick=()=>new Promise(resolve=>setImmediate(resolve));
const image=(id='old')=>({id,name:'中文🙂.png',mime:'image/png',byteLength:5,data:'AP+JUE4='});
const visible=(h,ids=['chat'],owner='human')=>{h.api.init=async()=>({...snapshot('0'),selfId:owner,conversations:ids.map(id=>({id}))});return h.store.getState().initialize()};
const reopen=h=>harness({factory:h.factory,values:h.values});

(async()=>{
  const h=harness();await visible(h,['chat','group']);
  h.store.getState().updateDraft('chat','文字与图片🙂');
  h.store.getState().imageDraft('chat',[image()]);
  h.store.getState().targetDraft('chat',['ai'],'reply');
  const reference={actorId:'ai',selection:{conversationId:'group',sources:[{kind:'message',conversation_id:'group',message_id:'source',revision:'2'}]}};
  h.store.getState().referenceDraft('chat',reference);
  h.store.getState().imageDraft('group',[image('group-image')]);
  await h.flush();assert.equal(h.unsaved(),false);
  const restored=reopen(h);let sends=0;restored.api.send=async()=>{sends++;throw new Error('offline')};
  await visible(restored,['chat','group']);
  assert.equal(sends,0,'loading never sends a draft');
  const draft=restored.store.getState().drafts.chat;
  assert.equal(draft.text,'文字与图片🙂');assert.equal(draft.images[0].data,image().data);
  assert.equal(draft.images[0].name,image().name);assert.equal(draft.images[0].id,'old');
  assert.equal(JSON.stringify(draft.reference),JSON.stringify(reference));assert.equal(draft.replyTo,'reply');assert.equal(draft.mentions[0],'ai');
  assert.equal(restored.store.getState().drafts.group.images[0].id,'group-image');
  assert.equal(h.values.has('llm-nest-social-live-drafts-v1'),false);

  // Lost acknowledgement, then edited input, then a new page: retry the immutable command.
  const calls=[];restored.api.send=async(_,input)=>{calls.push(input);throw new Error('lost receipt')};
  await restored.store.getState().send('chat');
  restored.store.getState().updateDraft('chat','later input');
  restored.store.getState().imageDraft('chat',[...draft.images,image('new')]);
  await restored.flush();
  const recovered=reopen(restored);await visible(recovered,['chat','group']);
  assert.equal(calls.length,1);
  recovered.api.send=async(_,input)=>{calls.push(input);return {message:message(input.text),executionNotice:null}};
  await recovered.store.getState().send('chat');
  assert.equal(JSON.stringify(calls[1]),JSON.stringify(calls[0]),'reload retries identical text, images, targets, references and UUID');
  assert.equal(recovered.store.getState().drafts.chat.text,'later input');
  assert.deepEqual(Array.from(recovered.store.getState().drafts.chat.images,i=>i.id),['new']);
  assert.equal((await recovered.record('chat')).pending,null);
  recovered.store.getState().imageDraft('chat',[]);recovered.store.getState().updateDraft('chat','');await recovered.flush();
  assert.equal((await recovered.record('chat')).draft.images.length,0,'removed image blobs disappear with the new transaction');

  // Persistent send identity is required before dispatch. Quota failures stay visible and retryable.
  const f=harness();await visible(f);f.store.getState().imageDraft('chat',[image()]);await f.flush();
  const save=f.database.save.bind(f.database);f.database.save=async()=>{throw new DOMException('quota full','QuotaExceededError')};
  let network=0;f.api.send=async()=>{network++;return {message:message(''),executionNotice:null}};
  await f.store.getState().send('chat');assert.equal(network,0);
  assert.equal(f.store.getState().draftSaveStates.chat.status,'error');assert.equal(f.store.getState().drafts.chat.images.length,1);
  await assert.rejects(f.flush(),/尚未保存/);
  f.database.save=save;await f.flush();assert.equal(f.store.getState().draftSaveStates.chat.status,'saved','close retry also saves drafts in a hidden/blocked window');await f.store.getState().send('chat');assert.equal(network,1);
  assert.equal((await f.record('chat')).draft.images?.length??0,0);

  // A failure to clean the local draft cannot turn a committed message into a failed send.
  const cleanup=harness();await visible(cleanup);cleanup.store.getState().imageDraft('chat',[image()]);await cleanup.flush();
  const originalSave=cleanup.database.save.bind(cleanup.database);
  cleanup.database.save=async(owner,id,record)=>{if(!record.pending)throw new Error('cleanup failed');await originalSave(owner,id,record)};
  let sent;cleanup.api.send=async(_,input)=>{sent=input;return {message:message('committed'),executionNotice:null}};
  await cleanup.store.getState().send('chat');assert.equal(cleanup.store.getState().histories.chat[0].text,'committed');
  assert.equal(cleanup.store.getState().draftSaveStates.chat.status,'error');
  const afterCleanup=reopen(cleanup);await visible(afterCleanup);assert.equal(afterCleanup.store.getState().pendingSends.chat.requestId,sent.requestId);
  afterCleanup.api.send=async(_,input)=>{assert.equal(JSON.stringify(input),JSON.stringify(sent));return {message:message('committed'),executionNotice:null}};
  await afterCleanup.store.getState().send('chat');assert.equal((await afterCleanup.record('chat')).pending,null);

  // Loading failure does not replace the old draft with an empty editable form.
  const unread=reopen(h),load=unread.database.load.bind(unread.database);unread.database.load=async()=>{throw new Error('read denied')};
  await visible(unread);assert.equal(unread.store.getState().draftsReady,false);assert.match(unread.store.getState().draftLoadError,/read denied/);
  assert.throws(()=>unread.store.getState().updateDraft('chat','overwrite'),/先恢复/);
  unread.database.load=load;await unread.store.getState().loadDrafts();assert.equal(unread.store.getState().draftsReady,true);

  // Old text drafts migrate only for visible windows and retain their retry identity.
  const legacy=harness();const legacyKey=legacy.persistence.LEGACY_DRAFT_KEY;
  legacy.values.set(legacyKey,JSON.stringify({chat:{text:'legacy',requestId:'legacy-request',mentions:['ai'],replyTo:'r'},unknown:{text:'keep',requestId:null}}));
  await visible(legacy);assert.equal(legacy.store.getState().pendingSends.chat.requestId,'legacy-request');
  assert.deepEqual(Object.keys(JSON.parse(legacy.values.get(legacyKey))),['unknown']);
  legacy.store.getState().discardPending('chat');legacy.store.getState().updateDraft('chat','');await legacy.flush();
  // Simulate a crash between database migration and removing the old storage key.
  legacy.values.set(legacyKey,JSON.stringify({chat:{text:'must not reappear',requestId:null}}));
  const migrated=reopen(legacy);await visible(migrated);assert.equal(migrated.store.getState().drafts.chat.text,'');
  const corrupt=harness();corrupt.values.set(legacyKey,'{broken');await visible(corrupt);assert.equal(corrupt.store.getState().draftsReady,false);assert.equal(corrupt.values.get(legacyKey),'{broken');

  // Two pages cannot silently overwrite the same draft; owners and windows are separate.
  const first=harness();await visible(first,['chat','other']);
  const second=reopen(first);await visible(second,['chat','other']);
  first.store.getState().updateDraft('chat','first page');await first.flush();
  second.store.getState().updateDraft('chat','second page');await tick();
  await assert.rejects(second.store.getState().saveDraft('chat'),/另一个页面/);
  assert.equal((await first.record('chat')).draft.text,'first page');assert.equal(second.store.getState().drafts.chat.text,'second page');
  second.store.getState().updateDraft('other','independent');await second.store.getState().saveDraft('other');
  const otherOwner=reopen(first);await visible(otherOwner,['chat'],'other-human');assert.equal(otherOwner.store.getState().drafts.chat,undefined);
  otherOwner.store.getState().updateDraft('chat','other owner');await otherOwner.flush();assert.equal((await first.record('chat')).draft.text,'first page');

  // Closing waits for the outstanding transaction, and old save results cannot mark new input saved.
  const closing=harness();await visible(closing);const gate=defer(),writer=closing.database.save.bind(closing.database);
  closing.database.save=async(...args)=>{await gate.promise;await writer(...args)};
  closing.store.getState().updateDraft('chat','first');closing.store.getState().imageDraft('chat',[image()]);
  assert.equal(closing.store.getState().draftSaveStates.chat.status,'saving');let closed=false;const drain=closing.flush().then(()=>closed=true);
  await tick();assert.equal(closed,false);gate.resolve();await drain;assert.equal(closing.unsaved(),false);
  assert.equal((await closing.record('chat')).draft.images[0].data,image().data);

  // Closing during file reading includes the selected image before it acknowledges the host.
  const selection=defer(),reading=harness({readImages:()=>selection.promise});await visible(reading);
  const chosen=reading.store.getState().addDraftImages('chat',[{}]);reading.store.setState({draftsClosing:true});
  assert.equal(reading.unsaved(),true);let readClosed=false;const readDrain=reading.flush().then(()=>readClosed=true);
  await tick();assert.equal(readClosed,false);selection.resolve([image()]);await chosen;await readDrain;
  assert.equal((await reading.record('chat')).draft.images[0].data,image().data,'close includes a selection already being read');

  // The actual lifecycle hook coordinates browser unload and native close acknowledgements.
  const effects=[],events={},listeners={},invocations=[],state={};let dirty=true,flushGate=defer();
  const code=ts.transpileModule(fs.readFileSync(path.resolve(__dirname,'../src/social/useDraftLifecycle.ts'),'utf8'),{compilerOptions:{target:ts.ScriptTarget.ES2022,module:ts.ModuleKind.CommonJS}}).outputText;
  const hook={};vm.runInNewContext(code,{exports:hook,crypto:require('node:crypto').webcrypto,window:{addEventListener:(n,f)=>events[n]=f,removeEventListener:n=>delete events[n]},require:name=>{
    if(name==='react')return {useEffect:fn=>effects.push(fn)};
    if(name==='@/lib/desktop')return {IS_DESKTOP:true};
    if(name==='@tauri-apps/api/core')return {invoke:async(name,args)=>{invocations.push({name,args})}};
    if(name==='@tauri-apps/api/event')return {listen:async(name,fn)=>{listeners[name]=fn;return ()=>delete listeners[name]}};
    return {hasUnsavedDrafts:()=>dirty,flushDrafts:()=>flushGate.promise,useLiveSocialStore:{setState:update=>Object.assign(state,update)}};
  }});
  hook.useDraftLifecycle();const unmount=effects[0]();await tick();assert.equal(invocations[0].name,'desktop_register_draft_guard');
  let warned=false;events.beforeunload({preventDefault(){warned=true}});assert.equal(warned,true);
  dirty=false;warned=false;events.beforeunload({preventDefault(){warned=true}});assert.equal(warned,false);
  const close=listeners['drafts-prepare-close']({payload:'close-1'});await tick();assert.equal(state.draftsClosing,true);assert.equal(invocations.length,1);
  flushGate.resolve();await close;assert.equal(invocations[1].name,'desktop_finish_draft_close');assert.equal(invocations[1].args.error,null);
  listeners['desktop-close-retained']();assert.equal(state.draftsClosing,false,'background/cancel releases the editor after native draft drain');
  state.draftsClosing=true;
  listeners['backend-shutdown-failed']();assert.equal(state.draftsClosing,false);
  flushGate=defer();const failedClose=listeners['drafts-prepare-close']({payload:'close-2'});flushGate.reject(new Error('quota'));await failedClose;
  assert.match(invocations.at(-1).args.error,/quota/);assert.equal(state.draftsClosing,false);unmount();await tick();assert.equal(invocations.at(-1).name,'desktop_unregister_draft_guard');
  console.log('PASS: persistent binary drafts, owner/window isolation, migration, lost-receipt recovery without auto-send, exact retry, new-input preservation, removal, quota/read/cleanup failures, multi-page conflicts and native close drain');
})().catch(e=>{console.error(e);process.exitCode=1});
