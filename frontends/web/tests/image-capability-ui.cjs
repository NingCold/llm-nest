const fs=require('node:fs'),path=require('node:path'),vm=require('node:vm'),ts=require('typescript'),assert=require('node:assert/strict');
const React=require('react'),{renderToStaticMarkup}=require('react-dom/server');
const root=path.resolve(__dirname,'../src');
const code=name=>ts.transpileModule(fs.readFileSync(path.join(root,name),'utf8'),{compilerOptions:{jsx:ts.JsxEmit.ReactJSX,target:ts.ScriptTarget.ES2022,module:ts.ModuleKind.CommonJS}}).outputText;
const labels={};vm.runInNewContext(code('lib/image-input.ts'),{exports:labels});
const load=(name,require,extra={})=>{const exports={};vm.runInNewContext(code(name),{exports,require,...extra});return exports};
const views=load('social/ImageCapabilityHint.tsx',name=>name==='@/lib/image-input'?labels:require(name));
const snapshot={accounts:[{id:'human',kind:'human',name:'我'},{id:'yes',kind:'ai',name:'甲',binding:{provider:'private-secret'}},{id:'no',kind:'ai',name:'乙'},{id:'maybe',kind:'ai',name:'丙'},{id:'outside',kind:'ai',name:'无关账号'}],imageInputs:{yes:'supported',no:'unsupported',maybe:'unknown',outside:'supported'}};
const group={kind:'group',memberIds:['human','yes','no','maybe']},draft={text:'',images:[{}]};
const render=(conversation=group,input=draft,reply)=>renderToStaticMarkup(React.createElement(views.ImageCapabilityHint,{snapshot,conversation,draft:input,reply}));
assert.match(render(),/甲 · 支持图片/);assert.match(render(),/乙 · 不支持图片/);assert.match(render(),/丙 · 图片能力未知/);
assert.match(render(),/图片会照常保存/);assert.doesNotMatch(render(),/private-secret|无关账号|<button/);
assert.doesNotMatch(render(group,{...draft,mentions:['yes']}),/乙|丙/);
assert.match(render(group,{...draft,mentions:['yes']},{senderId:'no',status:'committed',images:[]}),/乙/);
assert.equal(render(group,{text:''}), '');
assert.match(render(group,{text:'follow-up',reference:{}},undefined),/引用中如有图片/);
assert.match(render({kind:'direct',memberIds:['human','no']}),/乙 · 不支持图片/);
assert.doesNotMatch(render({kind:'direct',memberIds:['human','no']}),/甲|丙/);

(async()=>{
  // Real settings form: local edits survive a failed write; auto removes the override.
  const slots=[];let index=0,providers=[{id:'provider',models:[{id:'alias',displayName:'My model',imageInput:{support:'unknown',source:'unknown'}}]}],fail=true,requests=[];
  const api={setModelImageInput:async input=>{requests.push(input);if(fail)throw new Error('disk full');return [{id:'provider',models:[{id:'alias',displayName:'My model',imageInput:{support:input.imageInput===null?'supported':input.imageInput?'supported':'unsupported',source:input.imageInput===null?'catalog':'config'}}]}]}};
  const react={useState(initial){const i=index++;if(!(i in slots))slots[i]=initial;return [slots[i],value=>slots[i]=value]}};
  const jsx=(type,props)=>({type,props});
  const form=load('components/settings/ModelImageInput.tsx',name=>{
    if(name==='react')return react;if(name==='react/jsx-runtime')return {jsx,jsxs:jsx};
    if(name==='@/lib/image-input')return labels;if(name==='@/api')return {getApi:async()=>api};
    if(name==='@/store/config')return {useConfigStore:select=>select({providers,setProviders:value=>providers=value})};throw new Error(name);
  });
  const all=n=>!n||typeof n!=='object'?[]:[n,...[n.props?.children].flat(Infinity).flatMap(all)];
  const tree=()=>{index=0;return all(form.ModelImageInput({disabled:false}))};
  tree().find(n=>n.type==='select').props.onChange({target:{value:JSON.stringify(['provider','alias'])}});
  const control=()=>tree().filter(n=>n.type==='select')[1];
  assert.equal(control().props.value,'auto');control().props.onChange({target:{value:'unsupported'}});
  const save=async()=>{tree().find(n=>n.type==='button').props.onClick();await new Promise(r=>setImmediate(r))};
  await save();assert.equal(requests[0].imageInput,false);assert.equal(control().props.value,'unsupported');assert.ok(tree().some(n=>n.props?.role==='alert'));
  fail=false;await save();assert.equal(providers[0].models[0].imageInput.support,'unsupported');
  control().props.onChange({target:{value:'auto'}});await save();assert.equal(requests.at(-1).imageInput,null);assert.equal(control().props.value,'auto');assert.equal(providers[0].models[0].imageInput.source,'catalog');
  // Both settings adapters preserve false/null, provider and model keys.
  let http,ipc;
  const httpApi=load('api/http.ts',()=>{}, {Headers,fetch:async(url,init)=>{http={url,init};return {ok:true,status:200,json:async()=>[]}}}).httpApi;
  const tauriApi=load('api/tauri.ts',()=>({invoke:async(name,input)=>{ipc={name,input};return []}})).tauriApi;
  for(const imageInput of [false,true,null]) {
    const input={provider:'provider',model:'alias/with-slash',imageInput};await httpApi.setModelImageInput(input);await tauriApi.setModelImageInput(input);
    assert.equal(http.url,'/api/models/image-input');assert.equal(http.init.method,'POST');assert.equal(http.init.headers.get('X-LLMN-Client'),'1');
    assert.deepEqual(JSON.parse(http.init.body),input);assert.equal(ipc.name,'set_model_image_input');assert.equal(ipc.input.input,input);
  }
  console.log('PASS: image support/source labels, private/group recipient hints, no binding exposure, non-blocking send, override save failure/retry/auto and HTTP/IPC parity');
})().catch(e=>{console.error(e);process.exitCode=1});
