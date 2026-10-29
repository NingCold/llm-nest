const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const ts = require('typescript');
const React = require('react');
const {renderToStaticMarkup} = require('react-dom/server');

function harness(file, desktop = false) {
  const values=[]; let cursor=0;
  const source=ts.transpileModule(fs.readFileSync(path.join(__dirname,'../src/cloud',file+'.tsx'),'utf8'), {compilerOptions:{jsx:ts.JsxEmit.ReactJSX,module:ts.ModuleKind.CommonJS,target:ts.ScriptTarget.ES2022}}).outputText;
  const exports={};
  vm.runInNewContext(source,{exports,location:{hash:'',search:''},URLSearchParams,require(name){
    if(name==='react') return {...React,useState(initial){const i=cursor++;if(values[i]===undefined)values[i]=initial;return [values[i],v=>{values[i]=typeof v==='function'?v(values[i]):v;}];}};
    if(name==='@/lib/desktop') return {CUSTOM_WINDOW_CHROME:desktop,IS_DESKTOP:desktop};
    if(name==='@/store/ui') return {useUiStore:fn=>fn({theme:'light',toggleTheme(){}})};
    if(name==='@/components/BrandMark') return {BrandMark:()=>React.createElement('span',{'aria-hidden':true},'◈')};
    if(name.endsWith('.css'))return {};
    return require(name);
  }});
  return {render(props){cursor=0;return exports[file](props);}};
}
function nodes(tree){if(!tree||typeof tree!=='object')return [];return [tree,...React.Children.toArray(tree.props?.children).flatMap(nodes)];}
function text(tree){return React.Children.toArray(tree?.props?.children).map(n=>typeof n==='object'?text(n):String(n)).join('');}
function button(tree,label){return nodes(tree).find(n=>n.type==='button'&&text(n)===label);}
const authProps={mode:'login',onMode(){},busy:false,connected:true,connecting:false,serverUrl:'',onServerUrl(){},onConnect(){},onSubmit(){},error:'',notice:'',canResume:false,onCorrectEmail(){},onResend(){}};
for(const mode of ['login','verify']){
  const h=harness('AuthPage');let submitted;
  const tree=h.render({...authProps,mode,onSubmit:form=>submitted=form});
  const checkbox=nodes(tree).find(n=>n.type==='input'&&n.props.name==='rememberLogin');
  assert.equal(checkbox.props.type,'checkbox');assert.ok(!checkbox.props.checked&&!checkbox.props.defaultChecked,'extended sessions require explicit consent');
  const form=nodes(tree).find(n=>n.type==='form'&&n.props.className==='social-form auth-form');
  const formElement={rememberLogin:true};form.props.onSubmit({preventDefault(){},currentTarget:formElement});
  assert.equal(submitted,formElement,'the caller reads consent from the submitted form');
  const disabled=h.render({...authProps,mode,rememberSupported:false});
  assert.equal(nodes(disabled).find(n=>n.props?.name==='rememberLogin').props.disabled,true);
  assert.match(text(disabled),/此平台暂不支持保持登录/);
}
for(const mode of ['register','forgot','reset'])assert.ok(!nodes(harness('AuthPage').render({...authProps,mode})).some(n=>n.props?.name==='rememberLogin'));

let unlocked, recovered=0,loggedOut=0;
const lockProps={accountName:'测试账号',busy:false,error:'',onUnlock:value=>{unlocked=value;},onRecovery(){recovered++;},onLogout(){loggedOut++;}};
const lock=harness('LockScreen',true);
let tree=lock.render(lockProps);
assert.equal(nodes(tree).find(n=>n.type==='header').props['data-tauri-drag-region'],'deep');
assert.deepEqual(nodes(tree).filter(n=>n.props?.['data-tauri-drag-region']).map(n=>n.type),['header'],'the form stays outside the drag region');
const input=nodes(tree).find(n=>n.type==='input');
assert.equal(input.props.type,'password');assert.equal(input.props.autoComplete,'off');assert.equal(input.props.autoFocus,true);
assert.equal(button(tree,'解锁').props.disabled,true);
input.props.onChange({target:{value:'independent-local-passphrase'}});
tree=lock.render(lockProps);
assert.equal(button(tree,'解锁').props.disabled,false);
nodes(tree).find(n=>n.type==='form').props.onSubmit({preventDefault(){}});
assert.equal(unlocked,'independent-local-passphrase');
assert.equal(nodes(lock.render(lockProps)).find(n=>n.type==='input').props.value,'','submitted passphrases are cleared');
button(tree,'使用恢复文件').props.onClick();button(tree,'退出登录').props.onClick();assert.equal(recovered,1);assert.equal(loggedOut,1);
const automatic=lock.render({...lockProps,autoUnlocking:true});
assert.ok(!nodes(automatic).some(n=>n.type==='form'||n.type==='input'),'automatic unlock never flashes a password form');
assert.ok(nodes(automatic).some(n=>n.props?.role==='status'&&text(n)==='正在解锁…'));
const busy=lock.render({...lockProps,busy:true,error:'口令不正确'});
assert.ok(nodes(busy).find(n=>n.props?.role==='alert'&&text(n)==='口令不正确'));
assert.ok(nodes(busy).filter(n=>n.type==='input'||n.type==='button'&&text(n)!=='').every(n=>n.props.disabled),'busy unlock blocks duplicate submissions and conflicting actions');
const web=renderToStaticMarkup(harness('LockScreen').render(lockProps));
assert.doesNotMatch(web,/data-tauri-drag-region|消息列表|conversation-row|live-bubble/);
assert.match(web,/lock-screen-backdrop[^>]*aria-hidden="true"/);
console.log('PASS: login consent, independent glass lock screen, accessible manual/automatic unlock and safe desktop header');

if(process.argv[2]){
  const css=['social/social.css','cloud/auth.css','cloud/lock-screen.css'].map(file=>fs.readFileSync(path.join(__dirname,'../src',file),'utf8')).join('\n');
  const assets=path.join(__dirname,'../dist/assets');
  const base=fs.existsSync(assets)?fs.readdirSync(assets).filter(name=>name.startsWith('index-')&&name.endsWith('.css')).map(name=>fs.readFileSync(path.join(assets,name),'utf8')).join(''):'';
  fs.mkdirSync(process.argv[2],{recursive:true});
  for(const desktop of [true,false])for(const theme of ['light','dark'])for(const [name,element]of [['lock',harness('LockScreen',desktop).render(lockProps)],['automatic',harness('LockScreen',desktop).render({...lockProps,autoUnlocking:true})],['error',harness('LockScreen',desktop).render({...lockProps,error:'口令不正确，请重试。'})],['login',harness('AuthPage',desktop).render(authProps)]]){
    fs.writeFileSync(path.join(process.argv[2],`${desktop?"":"web-"}${name}-${theme}.html`),`<!doctype html><html class="${theme}"><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><style>${base}\n${css}\nbody{margin:0;font-family:Arial,sans-serif}button{font:inherit}</style>${renderToStaticMarkup(element)}</html>`);
  }
}
