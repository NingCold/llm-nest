// Public profiles must not reveal the private AI administration fields.
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const assert = require('node:assert/strict');
const React = require('react');
const { renderToStaticMarkup } = require('react-dom/server');
const ts = require('typescript');
const root = path.resolve(__dirname, '../src/social');
const human = { id:'human',kind:'human',name:'Human',bio:'Human public bio',revision:'1',status:'active',avatarRef:null };
const ai = { id:'ai',kind:'ai',name:'AI',bio:'AI public bio',revision:'1',status:'active',avatarRef:null,persona:'private-persona-secret',binding:{providerId:'private-provider-secret',modelId:'model'} };
const state = { snapshot:{ selfId:human.id,selfAccount:human,accounts:[human,ai],addressBook:[{ownerId:human.id,accountId:ai.id,listed:true,revision:'3'}],conversations:[] },error:null, action:async f=>f(),refresh:async()=>{} };
const modules = {};
function load(name) {
  if (modules[name]) return modules[name];
  const exports = {}; modules[name]=exports;
  const source = ts.transpileModule(fs.readFileSync(path.join(root,name+'.tsx'),'utf8'), { compilerOptions:{jsx:ts.JsxEmit.ReactJSX,target:ts.ScriptTarget.ES2022,module:ts.ModuleKind.CommonJS} }).outputText;
  vm.runInNewContext(source, {exports,require:spec=> {
    if (spec==='./live-store') return {useLiveSocialStore:selector=>selector?selector(state):state};
    if (spec==='./LiveControls') return load('LiveControls');
    if (spec==='./client') return {socialClient:{},SocialApiError:class extends Error{}};
    if (spec==='./LiveChat') return {ConsciousnessInfo:()=>null};
    if (spec==='./live-ui') return {contactAvatar:()=>undefined};
    if (spec==='./router') return {navigate:()=>{}};
    if (spec==='./components/common') return {Avatar:()=>null,EmptyState:()=>null,IconButton:({children,...props})=>React.createElement('button',props,children)};
    if (spec==='@/components/chat/MarkdownView') return {MarkdownView:()=>null};
    if (spec==='@/components/ui/dialog') return {Dialog:()=>null,DialogContent:()=>null,DialogDescription:()=>null,DialogTitle:()=>null};
    return require(spec);
  }});
  return exports;
}
const {AccountDetails}=load('AccountPages');
const render=account=>renderToStaticMarkup(React.createElement(AccountDetails,{account}));
for (const account of [human,ai]) {
  const html=render(account);
  assert.match(html,/公开简介/);
  assert.match(html,/账号 ID/);
  assert.match(html,new RegExp(account.bio));
  assert.doesNotMatch(html,/private-persona-secret|private-provider-secret|回复模型|内部角色设定|停用 AI 账号/);
}
assert.match(render(ai),/移除联系人/);
assert.match(render(ai),/仍可继续私聊与共同群聊/);
state.snapshot.addressBook[0].listed=false;
assert.match(render(ai),/添加到通讯录/);
assert.doesNotMatch(render(ai),/恢复联系人|恢复 AI 账号/);
assert.match(render(human),/AI 角色管理/);
console.log('PASS: shared human/AI public profile fields, private-setting exclusion, own-account management entry, independent remove/re-add controls');
