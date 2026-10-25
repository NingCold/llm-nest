// Offline visual fixtures built from the real components. No effects, credentials,
// network requests or application-only preview routes are involved.
// Hook states and data are simulated; business actions do not execute.
// The 120 pages are state/theme/platform combinations, not 120 independent tests.
// Run after `pnpm build`: node tests/layout-preview.cjs <output-directory>
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const assert = require('node:assert/strict');
const ts = require('typescript');
const React = require('react');
const {renderToStaticMarkup} = require('react-dom/server');
const root = path.resolve(__dirname, '../src');
const out = path.resolve(process.argv[2] || path.join(require('node:os').tmpdir(), 'llmn-layout-preview'));
const h = React.createElement;
const noop = () => {};
const blocked = () => { throw new Error('Offline layout fixtures cannot invoke services'); };
const longId = '11111111-2222-4333-8444-555555555555';
const me = {id: longId, name:'林间', kind:'human', status:'active', bio:'喜欢安静的交流，也喜欢新的想法。', revision:'1'};
const ai = {account:{id:'ai-fixture', name:'陪你整理思路的长名字 AI', kind:'ai', status:'active', bio:'一起梳理想法，保存值得记住的片段。'}, role:'initial', profileRevision:'1', bindingVersion:'3', statusRevision:'1', node:{id:'node-fixture', name:'书房电脑 · 我的 Agent', online:true, capabilities:['sealedV1','profileManagement'], handoverState:'active'}};
const group = {id:'group-fixture', kind:'group', name:'一起做点小事', revision:'2', readOnly:false, unreadCount:0, members:[{account:me,role:'owner'},{account:ai.account,role:'member'},{account:{...me,id:'other',name:'有一个很长名字的群聊参与者'},role:'admin'}]};
const sourceExports = {'cloud/LocalAgentPanel.tsx':['LocalSetup'], 'cloud/AiDetails.tsx':['PublicProfile','AdministratorsPanel'], 'cloud/Consciousness.tsx':['SummarySettingsForm'], 'cloud/Participation.tsx':['SettingsEditor','IntentEditor','PolicyEditor']};

function fixture(relative, name, props, seeds={}, theme='light', desktop=false) {
  const cache = new Map();
  function load(file) {
    const key = path.relative(root,file).replaceAll('\\','/');
    if (cache.has(file)) return cache.get(file);
    const exported = {}; cache.set(file, exported);
    let cursor = 0;
    const values = seeds[key] || [];
    const hooks = {...React, useEffect:noop, useLayoutEffect:noop, useRef:value=>({current:value}), useState(initial){const i=cursor++; return [i in values ? values[i] : typeof initial==='function'?initial():initial, noop];}};
    const mockDialog = {
      Dialog:({open,children})=>open ? h(React.Fragment,null,children):null,
      DialogTitle:({children})=>h('h2',null,children),
      DialogContent:({children,className})=>h('div',{className,role:'dialog'},children,h('button',{className:'absolute right-4 top-4',type:'button','aria-label':'关闭'},h('span',{className:'sr-only'},'关闭'),'×')),
    };
    function localRequire(spec) {
      if (spec==='react') return hooks;
      if (spec.endsWith('.css')) return {};
      if (spec==='@/lib/desktop') return {IS_DESKTOP:desktop,CUSTOM_WINDOW_CHROME:desktop};
      if (spec==='@/store/ui') return {useUiStore:select=>select({theme,setTheme:noop,toggleTheme:noop,showSettings:noop})};
      if (spec==='@/components/ui/dialog') return mockDialog;
      if (spec==='@/components/chat/MarkdownView') return {MarkdownView:({content})=>h('div',{className:'md'},content)};
      if (spec==='@tauri-apps/api/core') return {invoke:blocked};
      if (spec==='@tauri-apps/api/window') return {getCurrentWindow:blocked};
      const resolved = spec.startsWith('@/') ? path.join(root,spec.slice(2)) : spec.startsWith('.') ? path.resolve(path.dirname(file),spec) : null;
      if (resolved) {
        const basename = path.basename(resolved);
        if (basename==='api' || basename==='local-api') return {request:blocked,download:blocked,server:'https://offline.invalid',HttpError:class extends Error{}};
        if (resolved.includes(path.sep+'crypto'+path.sep) && !['Devices','ResourcePanel','HistoryMigrationPanel'].includes(basename)) return new Proxy({cryptoDevice:()=> 'fixture-device',hasCryptoDevice:()=>true,recoveryConfirmed:()=>true,resourceStatus:()=> '已保存 · 节点已应用'}, {get:(obj,key)=>obj[key] || blocked});
        const candidate = ['.tsx','.ts',''].map(ext=>resolved+ext).find(p=>fs.existsSync(p)&&fs.statSync(p).isFile());
        if (!candidate) throw new Error(`Cannot resolve ${spec} from ${key}`);
        return load(candidate);
      }
      return require(spec);
    }
    let source = ts.transpileModule(fs.readFileSync(file,'utf8'),{compilerOptions:{jsx:ts.JsxEmit.ReactJSX,module:ts.ModuleKind.CommonJS,target:ts.ScriptTarget.ES2022}}).outputText;
    if (sourceExports[key]) source+='\n'+sourceExports[key].map(n=>`exports.${n} = ${n};`).join('\n');
    vm.runInNewContext(source,{exports:exported,require:localRequire,window:{},location:{hash:'',search:''},URLSearchParams,console,crypto:require('node:crypto').webcrypto,TextEncoder,TextDecoder,setTimeout,clearTimeout,setInterval,clearInterval},{filename:key});
    return exported;
  }
  return renderToStaticMarkup(h(load(path.join(root,relative))[name],props));
}

function workspace(tab, seeds, theme, desktop, overrides={}) {
  const props = {me,snapshot:{contacts:[ai.account],conversations:[group],cursor:'1'},active:'',tab,location:null,page:{messages:[]},query:'',results:[],record:{draft:{text:'',images:[],files:[],mentions:[],reference:null},pending:null},draftReady:true,busy:false,error:'',notice:'',editing:null,searchMore:false,...overrides};
  for (const name of ['navigate','selectConversation','queryChange','search','locate','act','refresh','history','openDirect','continuePrivately','list','mutate','edit','text','attach','save','send','recoverDraft','onBottom','onProfile','logout','endSession','onPreview','dismissError','enableEncryption','prepareLocal','syncLocal','stopLocal','closeLocal']) props[name]=noop;
  return fixture('cloud/Workspace.tsx','Workspace',props,seeds,theme,desktop);
}
const status = {deviceId:longId,trusted:true,master:'master-'+longId,fingerprint:'ed25519-'+longId,curve:'curve25519-'+longId,hasRecoverySecrets:true};
const localStatus = {configured:false,running:false,nodeId:null,configuration:null,pairing:null,aiIds:[],error:null};
const localCatalog = {providers:[{id:'example',name:'示例模型服务',models:[{id:'example-model',name:'示例模型',contextWindow:65536,reasoningEfforts:[]}]}]};
const model = {providerId:'example',modelId:'example-model',name:'示例模型',reasoningEfforts:['low','medium','high'],imageInput:'unknown'};
const summarySettings = {revision:'1',enabled:true,binding:{providerId:model.providerId,modelId:model.modelId,reasoningEffort:'medium',maxTokens:1024}};
const state = {busy:false,pending:null,error:'',loading:false,view:{canManage:true,settings:{enabled:true}},execute:blocked};
const workspaceSeed = (contact=null, section='profile', editing=false) => ({'cloud/Workspace.tsx':[contact,'listed',[],false,false,false,[ai.account.id],'',false,'all',false,false,'appearance',section,editing,[]]});
function chat(theme,desktop,{sealed=false,local=false,pending=false,details=false}={}) {
  const conversation = {...group,id:'direct-fixture',kind:'direct',name:ai.account.name,peer:ai.account,members:group.members.slice(0,2),latestPosition:'2',lastMessage:null,...(sealed?{encryption:{version:1,activationPosition:'0'}}:{})};
  const message = {id:'message-fixture',conversationId:conversation.id,senderId:me.id,senderName:me.name,text:'今天想一起整理一下最近的计划。',revision:'1',position:'1',status:'committed',createdAt:'2026-10-25T08:00:00Z',attachments:[],mentions:[],replyTo:null,contextReference:null,aiResponses:[],...(local?{local:{synced:!pending}}:{})};
  const response = {...message,id:'response-fixture',senderId:ai.account.id,senderName:ai.account.name,text:'可以。先从最想完成的一件事开始，我们慢慢梳理。',position:'2'};
  const seeds = workspaceSeed(); seeds['cloud/Workspace.tsx'][4]=details;
  return workspace('messages',seeds,theme,desktop,{active:conversation.id,cryptoUnlocked:true,snapshot:{self:me,contacts:[ai.account],conversations:[conversation],cursor:'1'},page:{messages:[message,response],hasMore:false,latestPosition:'2'},localSelected:local,localChat:local?{ai:ai.account,human:me,conversation:{id:conversation.id},messages:[],hasMore:false,syncState:pending?'pending':'synced',error:null,running:pending}:undefined});
}
const pages = {
  'chat':(theme,desktop)=>chat(theme,desktop),
  'chat-sealed':(theme,desktop)=>chat(theme,desktop,{sealed:true}),
  'chat-local':(theme,desktop)=>chat(theme,desktop,{sealed:true,local:true}),
  'chat-local-pending':(theme,desktop)=>chat(theme,desktop,{sealed:true,local:true,pending:true}),
  'chat-details':(theme,desktop)=>chat(theme,desktop,{sealed:true,details:true}),
  'chat-local-details':(theme,desktop)=>chat(theme,desktop,{sealed:true,local:true,pending:true,details:true}),
  'account':(theme,desktop)=>workspace('account',workspaceSeed(),theme,desktop),
  'account-edit':(theme,desktop)=>workspace('account',workspaceSeed(null,'profile',true),theme,desktop),
  'contact':(theme,desktop)=>workspace('contacts',workspaceSeed(ai.account),theme,desktop),
  'settings':(theme,desktop)=>workspace('settings',workspaceSeed(),theme,desktop),
  'ai-list':(theme,desktop)=>workspace('account',{...workspaceSeed(null,'ai'),'cloud/AiManagement.tsx':[[ai], '',false,'',null,null]},theme,desktop),
  'ai-profile':(theme,desktop)=>fixture('cloud/AiDetails.tsx','AiDetails',{ai,operatorId:me.id,conversations:[group],cursor:'1',back:noop,changed:noop},{},theme,desktop),
  'ai-administrators':(theme,desktop)=>fixture('cloud/AiDetails.tsx','AiDetails',{ai,operatorId:me.id,conversations:[group],cursor:'1',back:noop,changed:noop},{'cloud/AiDetails.tsx':['administrators','',{revision:'1',administrators:[{account:me,role:'initial'},{account:{...me,id:'other',name:'另一位管理员'},role:'admin'}]}]},theme,desktop),
  'ai-configuration':(theme,desktop)=>fixture('cloud/AiConfiguration.tsx','AiConfiguration',{ai:{...ai,node:{...ai.node,capabilities:['profileManagement']}},operatorId:me.id},{'cloud/AiConfiguration.tsx':[{profile:{revision:'1',persona:'温和、清晰地帮助用户整理思路。',providerId:model.providerId,modelId:model.modelId,reasoningEffort:'medium'},models:[model]}]},theme,desktop),
  'ai-create':(theme,desktop)=>fixture('cloud/AiManagement.tsx','AiManagement',{operatorId:me.id,conversations:[group],cursor:'1',back:noop,refresh:noop},{'cloud/AiManagement.tsx':[[ai], '',false,'',null,{kind:'create'}]},theme,desktop),
  'group-create':(theme,desktop)=>fixture('cloud/Groups.tsx','CreateGroup',{contacts:[me,ai.account],me:me.id,act:blocked,busy:false,error:'',refresh:noop,select:noop,close:noop},{},theme,desktop),
  'group-manage':(theme,desktop)=>fixture('cloud/Groups.tsx','GroupDetails',{conversation:group,me:me.id,act:blocked,busy:false,refresh:noop,leave:noop},{},theme,desktop),
  'local-agent':(theme,desktop)=>fixture('cloud/LocalAgentPanel.tsx','LocalAgentPanel',{ai,changed:noop},{'cloud/LocalAgentPanel.tsx':[localStatus,localCatalog]},theme,desktop),
  'devices':(theme,desktop)=>workspace('account',{...workspaceSeed(null,'crypto'),'cloud/crypto/Devices.tsx':[status,false,'',false,me.id,[],null,false,false,'','','',null,false,0]},theme,desktop),
  'recovery':(theme,desktop)=>fixture('cloud/crypto/Devices.tsx','CryptoDevices',{accountId:me.id},{'cloud/crypto/Devices.tsx':[status,false,'',false,me.id,[],null,false,false,'','','',{format:'fixture'},true,0]},theme,desktop),
  'device-trust':(theme,desktop)=>fixture('cloud/crypto/Devices.tsx','CryptoDevices',{accountId:me.id},{'cloud/crypto/Devices.tsx':[status,false,'',false,me.id,[],{deviceId:'other-'+longId,name:'另一台设备',fingerprint:'ed-'+longId,curve:'curve-'+longId,master:'master-'+longId},false,false,'','','',null,false,0]},theme,desktop),
  'summary-settings':(theme,desktop)=>fixture('cloud/Consciousness.tsx','SummarySettingsForm',{settings:summarySettings,models:[model],state:{busy:false,uncertain:false},done:noop},{},theme,desktop),
  'encrypted-summary':(theme,desktop)=>fixture('cloud/crypto/ResourcePanel.tsx','EncryptedResourcePanel',{operatorId:me.id,aiId:ai.account.id,kind:'summarySettings',conversation:group},{'cloud/crypto/ResourcePanel.tsx':[[],[],{},{data:{enabled:true}},'',false,false,true]},theme,desktop),
  'contact-settings':(theme,desktop)=>fixture('cloud/Participation.tsx','SettingsEditor',{value:{enabled:true,quietAction:'defer',quietUntil:null},state,done:noop},{},theme,desktop),
  'contact-intent':(theme,desktop)=>fixture('cloud/Participation.tsx','IntentEditor',{ais:[ai.account],state,done:noop},{},theme,desktop),
};
for (const mode of ['login','register','verify','forgot','reset']) pages['auth-'+mode]=(theme,desktop)=>fixture('cloud/AuthPage.tsx','AuthPage',{mode,onMode:noop,busy:false,connected:true,connecting:false,serverUrl:'https://offline.invalid',onServerUrl:noop,onConnect:noop,onSubmit:blocked,error:'',notice:'',canResume:true,onCorrectEmail:noop,onResend:noop},{},theme,desktop);

const assets = path.resolve(__dirname,'../dist/assets');
assert.ok(fs.existsSync(assets),'Run pnpm build before generating previews');
const base = fs.readdirSync(assets).filter(n=>n.startsWith('index-')&&n.endsWith('.css')).map(n=>fs.readFileSync(path.join(assets,n),'utf8')).join('\n');
const css = base+['social/social.css','cloud/cloud.css','cloud/auth.css','cloud/crypto/devices.css','cloud/chat-layout.css'].filter(p=>fs.existsSync(path.join(root,p))).map(p=>fs.readFileSync(path.join(root,p),'utf8')).join('\n');
fs.mkdirSync(out,{recursive:true});
fs.cpSync(path.resolve(__dirname,'../public/brand'),path.join(out,'brand'),{recursive:true});
const links=[];
for (const theme of ['light','dark']) for (const desktop of [false,true]) for (const [name,render] of Object.entries(pages)) {
  let markup=render(theme,desktop);
  if (name==='device-trust') markup=markup.replace('class="crypto-card crypto-other-devices"','class="crypto-card crypto-other-devices" open');
  if (['account','contact','settings','ai-list','devices'].includes(name)) {
    assert.ok(markup.includes('aria-label="主导航"'),`${name}: preview must retain the real workspace navigation`);
    assert.ok(!markup.includes('cloud-image-dialog'),`${name}: fixture must not accidentally open the image preview`);
  }
  if (name==='encrypted-summary') assert.match(markup,/<label class="cloud-check"><input[^>]+type="checkbox"/,'summary toggle must use the shared checkbox row');
  const full = ['account','account-edit','contact','settings','ai-list','devices'].includes(name) || name.startsWith('auth-') || name.startsWith('chat');
  const modal = ['ai-create','group-create'].includes(name);
  if (!full) markup=modal ? markup : `<main class="social-shell cloud-workspace ${desktop?'desktop-chrome':''}"><div class="fixture-content">${markup}</div></main>`;
  const filename=`${name}-${theme}-${desktop?'desktop':'web'}.html`;
  const controls = desktop ? fixture('components/layout/WindowControls.tsx','WindowControls',{}, {},theme,true) : '';
  fs.writeFileSync(path.join(out,filename),`<!doctype html><html class="${theme}"><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>${name} · offline layout preview</title><style>${css}\nbody{margin:0;font-family:Arial,sans-serif;background:${theme==='dark'?'#181818':'#fff'}}.fixture-content{height:100%;max-width:880px;margin:auto;padding:24px;overflow:auto}.fixture-content>.contact-view{height:100%}body>.social-dialog{position:relative;transform:none;left:auto;top:auto;margin:24px auto;display:grid;gap:16px}@media(max-width:600px){.fixture-content{padding:18px 16px}}</style>${markup}${controls}</html>`);
  links.push(`<a href="${filename}">${name} · ${theme} · ${desktop?'desktop':'web'}</a>`);
}
fs.writeFileSync(path.join(out,'index.html'),`<!doctype html><meta charset="utf-8"><title>LLM Nest offline layout previews</title><style>body{font:14px/1.6 Arial;margin:32px}nav{display:grid;grid-template-columns:repeat(auto-fit,minmax(270px,1fr));gap:8px}a{color:#3573c4}</style><h1>离线布局预览</h1><p>来自实际 React 组件，仅使用虚构资料。请在 390、800、1200、1440px 宽度检查。所有动作均不执行。</p><nav>${links.join('')}</nav>`);
console.log(`PASS: generated ${links.length} offline component previews in ${out}`);
