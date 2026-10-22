const assert=require('node:assert/strict'),fs=require('node:fs'),path=require('node:path'),vm=require('node:vm'),ts=require('typescript');
const {harness,snapshot,message,compile}=require('./draft-harness.cjs');
const tick=()=>new Promise(resolve=>setImmediate(resolve));
const deferred=()=>{let resolve;return {promise:new Promise(r=>resolve=r),resolve:value=>resolve(value)}};
const all=n=>!n||typeof n!=='object'?[]:[n,...[n.props?.children].flat(Infinity).flatMap(all)];
const image=(id='old')=>({id,name:'原图.png',mime:'image/png',data:'AQID',byteLength:3});

(async()=>{
  // Exercise the real processing module with controlled decoding/encoding boundaries.
  const urls=new Map(),canvases=[];let next=0,failDecode=false,failEncode=false,fallback=false;
  const urlApi={createObjectURL:file=>{const url='blob:'+next++;urls.set(url,file);return url},revokeObjectURL:url=>urls.delete(url)};
  const api={};
  vm.runInNewContext(compile('image-processing'),{exports:api,Blob,File,Uint8Array,atob,URL:urlApi,
    Image:class {async decode(){if(failDecode)throw new Error('bad image');const file=urls.get(this.src);this.naturalWidth=file.width??100;this.naturalHeight=file.height??100}},
    document:{createElement:()=>{const calls=[];const canvas={width:0,height:0,calls,getContext:()=>({fillRect:(...a)=>calls.push(['fill',...a]),drawImage:(_, ...a)=>calls.push(['draw',...a])}),toBlob(callback,mime,quality){calls.push(['encode',mime,quality]);callback(failEncode?null:new Blob(['processed'],{type:fallback?'image/png':mime}))}};canvases.push(canvas);return canvas}},
  });
  const file=Object.assign(new File(['original'],'透明大图.png',{type:'image/png'}),{width:6000,height:3000});
  const sources=await api.inspectImages([file]);assert.equal(sources[0].width,6000);
  const output=await api.processImages(sources,api.defaultImageOptions);
  assert.equal(output[0].width,2560);assert.equal(output[0].height,1280);assert.equal(output[0].file.type,'image/webp');
  assert.equal(output[0].file.name,'透明大图-处理后.webp');assert.equal(await file.text(),'original');
  assert.equal(canvases[0].calls.some(c=>c[0]==='fill'),false,'transparent output must not be flattened');
  assert.equal(canvases[0].width,0,'release canvas pixels after encoding');
  assert.deepEqual(JSON.parse(JSON.stringify(api.fittedSize(200,100,4096))),{width:200,height:100},'never upscale');
  const jpeg=await api.processImages(sources,{...api.defaultImageOptions,mime:'image/jpeg'});
  assert.equal(jpeg[0].file.name,'透明大图-处理后.jpg');assert.equal(canvases.at(-1).calls[0][0],'fill','JPEG explicitly uses a white background');
  api.releaseImages(output);api.releaseImages(jpeg);assert.equal(urls.size,1);
  fallback=true;await assert.rejects(api.processImages(sources,api.defaultImageOptions),/不支持/);fallback=false;
  failEncode=true;await assert.rejects(api.processImages(sources,api.defaultImageOptions),/编码失败/);failEncode=false;
  failDecode=true;await assert.rejects(api.inspectImages([file]),/bad image/);failDecode=false;assert.equal(urls.size,1,'failed source URL is released');
  const huge=Object.assign(new File(['x'],'huge.png',{type:'image/png'}),{width:10000,height:10000});
  await assert.rejects(api.inspectImages([huge]),/6400/);assert.equal(urls.size,1);
  await assert.rejects(api.inspectImages([{type:'image/png',size:21*1024*1024}]),/20 MiB/);
  assert.match(api.imageSelectionIssue([{...sources[0],width:4097}],[]),/4096/);
  assert.match(api.imageSelectionIssue([{...output[0],file:{size:5*1024*1024}}],[]),/4 MiB/);
  assert.match(api.imageSelectionIssue([{...output[0],file:{size:3*1024*1024}}],[{byteLength:6*1024*1024}]),/8 MiB/);
  assert.match(api.imageSelectionIssue(sources,Array(4).fill(image())),/最多 4/);
  api.releaseImages(sources);assert.equal(urls.size,0);

  // Selection remains outside the durable draft until explicit confirmation.
  let h,mode='oversize';const released=[];
  const normalSource={file:new File(['abc'],'processed.webp',{type:'image/webp'}),url:'blob:selected',width:1600,height:800};
  const originalSource={...normalSource,url:'blob:original',width:6000};
  const inspectGate=deferred();let inspection=null;
  h=harness({readImages:async()=>{if(mode==='oversize')throw new h.images.ImageProcessingRequired('4096');return [{...image('processed'),mime:'image/webp'}]},
    processing:{inspectImages:async()=>inspection?inspection.promise:[originalSource],releaseImages:s=>released.push(...s.map(i=>i.url))}});
  h.api.init=async()=>({...snapshot('0'),conversations:[{id:'chat'},{id:'other'}]});await h.store.getState().initialize();
  h.store.getState().updateDraft('chat','keep text');h.store.getState().imageDraft('chat',[image()]);await h.flush();
  await h.store.getState().addDraftImages('chat',[file]);const selection=h.store.getState().imageSelection;
  assert.equal((await h.record('chat')).draft.images[0].id,'old');assert.equal(h.unsaved(),true);
  await assert.rejects(h.flush(),/确认图片处理/);
  let sent=0;h.api.send=async()=>{sent++;throw new Error('should not send')};await h.store.getState().send('chat');assert.equal(sent,0);
  h.store.getState().cancelImageSelection();assert.equal(h.store.getState().drafts.chat.images[0].id,'old');assert.ok(released.includes('blob:original'));
  await assert.rejects(h.store.getState().acceptImageSelection(selection,[normalSource]),/已取消/);
  await h.store.getState().addDraftImages('chat',[file]);mode='normal';
  await h.store.getState().acceptImageSelection(h.store.getState().imageSelection,[normalSource]);
  assert.equal(h.store.getState().imageSelection,null);assert.equal((await h.record('chat')).draft.images.length,2);assert.equal(sent,0);
  assert.equal(h.store.getState().drafts.chat.text,'keep text');
  const restored=harness({factory:h.factory,values:h.values});restored.api.init=h.api.init;await restored.store.getState().initialize();
  assert.equal(restored.store.getState().drafts.chat.images[1].mime,'image/webp','accepted result survives reload');
  // Replacing a draft image must not mutate a pending send's immutable original image.
  h.api.send=async()=>{throw new Error('lost receipt')};await h.store.getState().send('chat');
  const pending=h.store.getState().pendingSends.chat;
  await h.store.getState().editDraftImage('chat','old');
  await h.store.getState().acceptImageSelection(h.store.getState().imageSelection,[normalSource]);
  assert.equal(h.store.getState().drafts.chat.images[0].id,'processed');assert.equal(pending.images[0].id,'old');
  assert.equal(h.store.getState().drafts.chat.requestId,null);
  // Close during inspection must expose confirmation, not silently accept a transformed image.
  mode='oversize';inspection=inspectGate;const selecting=h.store.getState().addDraftImages('other',[file]);
  const closing=assert.rejects(h.flush(),/确认图片处理/);inspectGate.resolve([originalSource]);await selecting;await closing;
  h.store.getState().cancelImageSelection();

  // A failed save keeps the explicitly accepted result editable and retryable.
  inspection=null;await h.store.getState().addDraftImages('other',[file]);mode='normal';
  const writer=h.database.save.bind(h.database);h.database.save=async()=>{throw new Error('quota')};
  await assert.rejects(h.store.getState().acceptImageSelection(h.store.getState().imageSelection,[normalSource]),/quota/);
  assert.equal(h.store.getState().imageSelection,null);assert.equal(h.store.getState().drafts.other.images[0].id,'processed');
  assert.equal(h.store.getState().draftSaveStates.other.status,'error');h.database.save=writer;await h.flush();
  assert.equal((await h.record('other')).draft.images[0].mime,'image/webp');

  // A send receipt can remove the original while confirmation is reading the result.
  const readGate=deferred(),sendGate=deferred(),sendStarted=deferred();
  const racing=harness({readImages:()=>readGate.promise,processing:{inspectImages:async()=>[originalSource],releaseImages:()=>{}}});
  racing.api.init=h.api.init;await racing.store.getState().initialize();racing.store.getState().imageDraft('chat',[image()]);
  racing.api.send=()=>{sendStarted.resolve();return sendGate.promise};
  const sending=racing.store.getState().send('chat');await sendStarted.promise;
  await racing.store.getState().editDraftImage('chat','old');
  const confirming=assert.rejects(racing.store.getState().acceptImageSelection(racing.store.getState().imageSelection,[normalSource]),/原草稿图片已移除/);
  sendGate.resolve({message:message(''),executionNotice:null});await sending;
  readGate.resolve([image('processed')]);await confirming;
  assert.equal(racing.store.getState().drafts.chat.images,undefined,'do not restore an already sent image');
  racing.store.getState().cancelImageSelection();

  // The actual dialog requires a current preview, rejects oversize results, and ignores late work.
  const slots=[],effects=[],state={drafts:{chat:{images:[]}},draftsClosing:false};let index=0,processed=0,accepted=0,processingGate=null;
  const react={useState(initial){const i=index++;if(!(i in slots))slots[i]=initial;return[slots[i],v=>slots[i]=typeof v==='function'?v(slots[i]):v]},useRef(initial){const i=index++;return slots[i]??={current:initial}},useEffect(fn){index++;effects.push(fn)}};
  const hook=()=>state;state.acceptImageSelection=async()=>accepted++;state.cancelImageSelection=()=>{};
  const jsx=(type,props)=>({type,props}),view={};
  const code=fs.readFileSync(path.resolve(__dirname,'../src/social/ImageProcessingDialog.tsx'),'utf8')+'\nexport { Editor };';
  vm.runInNewContext(ts.transpileModule(code,{compilerOptions:{jsx:ts.JsxEmit.ReactJSX,target:ts.ScriptTarget.ES2022,module:ts.ModuleKind.CommonJS}}).outputText,{exports:view,require:name=>{
    if(name==='react')return react;if(name==='react/jsx-runtime')return {jsx,jsxs:jsx};if(name==='./live-store')return {useLiveSocialStore:hook};
    if(name==='./image-processing')return {...api,processImages:async()=>{processed++;return processingGate?processingGate.promise:[normalSource]},releaseImages:s=>released.push(...s.map(i=>i.url))};return {};
  }});
  const props={selection:{id:'selection',conversationId:'chat',reason:'4096',sources:[originalSource]}};
  const render=()=>{index=0;return all(view.Editor(props))};
  const button=text=>render().find(n=>n.type==='button'&&n.props.children===text);
  assert.equal(button('使用处理结果加入草稿').props.disabled,true);
  button('生成处理预览').props.onClick();await tick();assert.equal(processed,1);assert.equal(accepted,0);
  assert.equal(button('使用处理结果加入草稿').props.disabled,false);
  state.drafts.chat.images=[{...image(),byteLength:8*1024*1024}];
  assert.equal(button('使用处理结果加入草稿').props.disabled,true,'include existing draft bytes in the acceptance limit');state.drafts.chat.images=[];
  render().find(n=>n.type==='select'&&n.props.value===2560).props.onChange({target:{value:'1600'}});
  assert.equal(button('使用处理结果加入草稿').props.disabled,true,'changing options invalidates the old preview');
  button('生成处理预览').props.onClick();await tick();button('使用处理结果加入草稿').props.onClick();await tick();assert.equal(accepted,1);
  processingGate=deferred();button('重新生成预览').props.onClick();const cleanup=effects[0]();cleanup();processingGate.resolve([{...normalSource,url:'blob:late'}]);await tick();
  assert.ok(released.includes('blob:late'),'late work after cancellation is discarded');
  console.log('PASS: fit and encoding, alpha/JPEG background, format failure, resource cleanup, confirmation/cancellation, totals, replacement, durable result, immutable send, close guard and stale preview');
})().catch(error=>{console.error(error);process.exitCode=1});
