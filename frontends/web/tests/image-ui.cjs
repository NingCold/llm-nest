// Real selection/composer and image reader with controlled browser primitives.
const fs=require('node:fs'),path=require('node:path'),vm=require('node:vm'),assert=require('node:assert/strict'),ts=require('typescript');
const compile=file=>ts.transpileModule(fs.readFileSync(path.resolve(__dirname,'../src/social',file),'utf8'),{compilerOptions:{jsx:ts.JsxEmit.ReactJSX,target:ts.ScriptTarget.ES2022,module:ts.ModuleKind.CommonJS}}).outputText;
const flush=()=>new Promise(resolve=>setImmediate(resolve));
const all=n=>!n||typeof n!=='object'?[]:[n,...[n.props?.children].flat(Infinity).flatMap(all)];
(async()=>{
  let dimensions=32, decodingFails=false, reads=0;
  const images={};
  vm.runInNewContext(compile('images.ts'),{exports:images,crypto:require('node:crypto').webcrypto,
    FileReader:class {readAsDataURL(file){reads++;this.result=`data:${file.type};base64,AQID`;this.onload()}},
    Image:class {get naturalWidth(){return dimensions}get naturalHeight(){return dimensions}async decode(){if(decodingFails)throw new Error('decode failed')}},
  });
  const file={type:'image/png',size:3,name:'中文.png'};
  const selected=await images.readImages([file],[]);
  assert.equal(selected[0].data,'AQID');assert.equal(selected[0].name,'中文.png');
  await assert.rejects(images.readImages([file],Array(4).fill(selected[0])),/最多 4/);
  await assert.rejects(images.readImages([{...file,size:5*1024*1024}],[]),/单张/);
  await assert.rejects(images.readImages([{...file,type:'image/svg+xml'}],[]),/PNG/);
  dimensions=4097;await assert.rejects(images.readImages([file],[]),/4096/);dimensions=32;
  decodingFails=true;await assert.rejects(images.readImages([file],[]),/无法识别/);decodingFails=false;
  const slots=[],effects=[];let index=0;
  const react={
    useState(initial){const i=index++;if(!(i in slots))slots[i]=initial;return[slots[i],v=>slots[i]=typeof v==='function'?v(slots[i]):v]},
    useRef(initial){const i=index++;return slots[i]??={current:initial}},
  };
  const state={draftsReady:true,draftsClosing:false,pendingSends:{},draftSaveStates:{},drafts:{chat:{text:'',images:[]}},sending:{},error:null,
    action:async fn=>{try{await fn()}catch(e){state.error=String(e)}},
    imageDraft:(id,images)=>{state.drafts[id]={...state.drafts[id],images}},updateDraft:(id,text)=>state.drafts[id].text=text,
    addDraftImages:async(id,files)=>{state.imageDraft(id,[...state.drafts[id].images,...await images.readImages(files,state.drafts[id].images)])},
    send:async id=>effects.push(id),
  };
  const hook=()=>state;hook.getState=()=>state;
  const jsx=(type,props)=>({type,props});const out={};
  vm.runInNewContext(compile('ChatComposer.tsx'),{exports:out,require:name=>name==='react'?react:name==='react/jsx-runtime'?{jsx,jsxs:jsx}:name==='./images'?images:name==='./live-store'?{useLiveSocialStore:hook}:{}});
  const render=()=>{index=0;return all(out.ChatComposer({id:'chat',title:'Alice',hint:'先保存'}))};
  const textarea=()=>render().find(n=>n.type==='textarea');const send=()=>render().find(n=>n.props['aria-label']==='发送消息');
  assert.equal(send().props.disabled,true);
  let prevented=false;
  textarea().props.onPaste({clipboardData:{items:[{kind:'string'}]},preventDefault:()=>prevented=true});assert.equal(prevented,false,'plain paste remains native');
  textarea().props.onPaste({clipboardData:{items:[{kind:'file',getAsFile:()=>file}]},preventDefault:()=>prevented=true});assert.ok(prevented);assert.ok(send().props.disabled);
  await flush();assert.equal(state.drafts.chat.images.length,1);assert.equal(send().props.disabled,false);
  textarea().props.onKeyDown({key:'Enter',shiftKey:false,nativeEvent:{isComposing:true},keyCode:229,preventDefault(){}});assert.equal(effects.length,0,'IME confirmation never sends');
  textarea().props.onKeyDown({key:'Enter',shiftKey:false,nativeEvent:{isComposing:false},keyCode:13,preventDefault(){}});assert.equal(effects[0],'chat');
  render().find(n=>n.props.label==='移除图片 中文.png').props.onClick();assert.equal(state.drafts.chat.images.length,0);
  render()[0].props.onDrop({dataTransfer:{files:[file]},preventDefault(){}});await flush();assert.equal(state.drafts.chat.images.length,1);
  const input=render().find(n=>n.type==='input');const event={target:{files:[file],value:'selected'}};input.props.onChange(event);assert.equal(event.target.value,'');await flush();assert.equal(state.drafts.chat.images.length,2);
  slots.length=0;effects.length=0;index=0;
  react.useEffect=(fn,deps)=>{const i=index++;let saved=slots[i];if(!saved||deps.some((d,n)=>d!==saved.deps[n])){saved?.cleanup?.();saved={deps};slots[i]=saved;effects.push(()=>saved.cleanup=fn())}};
  const pending=[],viewer={};const source=fs.readFileSync(path.resolve(__dirname,'../src/social/MessageImages.tsx'),'utf8')+'\nexport { MessageImage };';
  vm.runInNewContext(ts.transpileModule(source,{compilerOptions:{jsx:ts.JsxEmit.ReactJSX,target:ts.ScriptTarget.ES2022,module:ts.ModuleKind.CommonJS}}).outputText,{exports:viewer,
    IntersectionObserver:class {constructor(fn){this.fn=fn}observe(){this.fn([{isIntersecting:true}])}disconnect(){}},
    require:name=>name==='react'?react:name==='react/jsx-runtime'?{jsx,jsxs:jsx}:name==='./images'?images:name==='./client'?{socialClient:{messageImage:(...args)=>new Promise((resolve,reject)=>pending.push({args,resolve,reject}))}}:{},
  });
  const props={message:{id:'m',revision:'1',senderName:'我',images:[{index:0}]},index:0};
  const view=()=>{index=0;const tree=viewer.MessageImage(props);while(effects.length)effects.shift()();return all(tree)};
  view();view();assert.equal(pending.length,1);
  props.message={...props.message,revision:'2'};view();assert.equal(pending.length,2);
  pending[0].resolve({mime:'image/png',data:'OLD'});await flush();assert.equal(view().some(n=>n.props.src),false,'late older image never renders');
  pending[1].reject(new Error('offline'));await flush();assert.ok(view().some(n=>n.props.children==='重新加载'));
  view().find(n=>n.props.children==='重新加载').props.onClick();view();pending[2].resolve({mime:'image/png',data:'NEW'});await flush();
  assert.equal(view().find(n=>n.props.src).props.src,'data:image/png;base64,NEW');
  assert.equal(viewer.MessageImages({message:{...props.message,status:'withdrawn'}}),null,'withdrawal removes images');
  console.log('PASS: format/size/dimensions/decode checks, picker/paste/drop, image-only send, reading lock, remove and IME safety');
})().catch(e=>{console.error(e);process.exitCode=1});
