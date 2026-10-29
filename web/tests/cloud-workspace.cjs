const fs = require("node:fs"),
  path = require("node:path"),
  vm = require("node:vm"),
  assert = require("node:assert/strict"),
  React = require("react"),
  { renderToStaticMarkup } = require("react-dom/server"),
  ts = require("typescript");
const root = path.resolve(__dirname, "../src"),
  cache = new Map();
const location = { hash: "", search: "" };
const desktop = { CUSTOM_WINDOW_CHROME: false, IS_DESKTOP: false };
let confirmEnable = true;
const ui = {
  theme: "light",
  setTheme() {},
  toggleTheme() {},
  showSettings() {
    throw new Error("cloud settings must not initialize local settings");
  },
};
function load(name) {
  if (cache.has(name)) return cache.get(name);
  const file = [".tsx", ".ts"]
    .map((ext) => path.join(root, name + ext))
    .find(fs.existsSync);
  const exports = {};
  cache.set(name, exports);
  vm.runInNewContext(
    ts.transpileModule(fs.readFileSync(file, "utf8"), {
      compilerOptions: {
        jsx: ts.JsxEmit.ReactJSX,
        target: ts.ScriptTarget.ES2022,
        module: ts.ModuleKind.CommonJS,
      },
    }).outputText,
    {
      exports,
      URL,
      URLSearchParams,
      location,
      window: { confirm: () => confirmEnable },
      console,
      setTimeout,
      clearTimeout,
      require: (spec) => {
        if (spec === "@/lib/desktop")
          return desktop;
        if (spec === "@/store/ui") return { useUiStore: (fn) => fn(ui) };
        if (spec === "@/lib/utils")
          return { cn: (...args) => args.filter(Boolean).join(" ") };
        if (spec === "@/components/chat/MarkdownView")
          return {
            MarkdownView: ({ content }) =>
              React.createElement("div", { "data-markdown": true }, content),
          };
        if (spec === "./api")
          return {
            request() {
              throw new Error("render must not load local or cloud runtime");
            },
            download() {
              throw new Error("effects must not run during server render");
            },
          };
        if (spec.endsWith(".css")) return {};
        if (spec.startsWith("@/")) return load(spec.slice(2));
        if (spec.startsWith("."))
          return load(
            path.posix.normalize(
              path.posix.join(path.posix.dirname(name), spec),
            ),
          );
        return require(spec);
      },
    },
  );
  return exports;
}
const me = { id: "self", name: "自己", bio: "公开简介", kind: "human" },
  peer = { id: "peer", name: "朋友", bio: "对方介绍", kind: "human" };
const message = {
  id: "message",
  conversationId: "direct",
  senderId: "peer",
  text: "**原有消息正文**",
  revision: "1",
  position: "1",
  status: "committed",
  createdAt: "2026-10-20T12:00:00Z",
  attachments: [],
  senderName: "朋友", replyTo: null, mentions: [], aiResponses: [],
};
const conversation = {
  id: "direct",
  peer, kind: "direct", name: peer.name, members: [{ account: me, role: "member" }, { account: peer, role: "member" }], revision: "1", directBlocked: false,
  lastMessage: message,
  unreadCount: 1,
  manualUnread: false,
  pinned: true,
  listRevision: "1",
  latestPosition: "1",
};
const noop = () => {},
  asyncNoop = async () => {};
const p = {
  me,
  snapshot: {
    self: me,
    contacts: [peer],
    conversations: [conversation],
    cursor: "1",
  },
  active: "direct",
  tab: "messages",
  page: { messages: [message], hasMore: false, latestPosition: "1" },
  query: "",
  results: [],
  searchMore: false,
  record: {
    draft: { text: "保留的草稿", requestId: null, images: [], files: [] },
    pending: null,
  },
  draftReady: true,
  busy: false,
  error: "",
  notice: "",
  editing: null,
  navigate: noop,
  selectConversation: noop,
  queryChange: noop,
  search: asyncNoop,
  locate: asyncNoop,
  act: asyncNoop,
  refresh: asyncNoop,
  history: asyncNoop,
  openDirect: asyncNoop,
  list: asyncNoop,
  mutate: asyncNoop,
  edit: noop,
  text: noop,
  attach: noop,
  save: asyncNoop,
  send: asyncNoop,
  recoverDraft: asyncNoop,
  onBottom: noop,
  onProfile: noop,
  logout: asyncNoop,
  endSession: noop,
  onPreview: noop,
  dismissError: noop,
};
const Workspace = load("cloud/Workspace").Workspace,
  render = (overrides) =>
    renderToStaticMarkup(
      React.createElement(Workspace, { ...p, ...overrides }),
    );
let html = render();
assert.match(html, /创建群聊/);
assert.match(html, /social-sidebar/);
assert.match(html, /rail-brand/);
assert.match(html, /主导航/);
assert.match(html, /conversation-list-item/);
assert.match(html, /live-message/);
assert.match(html, /live-bubble/);
assert.match(html, /data-markdown/);
assert.match(html, /composer-area/);
assert.match(html, /composer-bottom/);
assert.match(html, /保留的草稿/);
assert.match(html, /添加图片/);
assert.match(html, /添加文件/);
assert.match(html, /聊天详情/);
assert.doesNotMatch(html, /创建 AI|意识与记忆|模型服务/);
html = render({ tab: "contacts", active: "" });
assert.match(html, /contact-list-item/);
assert.match(html, /查找账号/);
assert.match(html, /对方介绍/);
assert.doesNotMatch(html, /cloud-person/);
html = render({ tab: "account" });
assert.match(html, /profile-cover/);
assert.match(html, /profile-intro/);
assert.match(html, /账号 ID/);
assert.match(html, /公开简介/);
assert.match(html, /登录会话/);
assert.match(html, /我管理的 AI/);
html = render({ page: { ...p.page, messages: [{ ...message, status: "generating", text: "部分回复" }] } });
assert.match(html, /正在回复/);
html = render({ page: { ...p.page, messages: [{ ...message, senderId: me.id, aiResponses: [{ aiId: "peer", taskId: "task", status: "running" }] }] } });
assert.match(html, /AI 正在回复/);
assert.match(html, /停止回复/);
html = render({ page: { ...p.page, messages: [{ ...message, aiResponses: [{ aiId: "peer", taskId: "task", status: "failed", error: "image_input_unsupported" }] }] } });
assert.match(html, /暂不支持图片/);
assert.doesNotMatch(html, /停止回复|image_input_unsupported/);
html = render({ tab: "settings" });
assert.match(html, /外观/);
assert.match(html, /浅色/);
assert.match(html, /深色/);
assert.doesNotMatch(html, /模型服务|API Key/);
html = render({
  record: {
    ...p.record,
    pending: { ...p.record.draft, requestId: "immutable-id" },
  },
});
assert.match(html, /上次发送尚未确认/);
assert.match(html, /重试上次发送/);
html = render({ location: { conversationId: "direct", messageId: "message" } });
assert.match(html, /located-message/);
assert.match(html, /正在查看搜索到的消息/);
assert.match(html, /返回最新消息/);
html = render({
  busy: true,
  record: {
    ...p.record,
    pending: { ...p.record.draft, requestId: "immutable-id" },
  },
});
assert.doesNotMatch(html, /上次发送尚未确认|重试上次发送/);
html = render({ tab: "contacts" });
assert.doesNotMatch(
  html.match(/class="social-main ([^"]*)"/)[1],
  /has-selection/,
);
const AuthPage = load("cloud/AuthPage").AuthPage;
const authProps = {
  mode: "login",
  onMode: noop,
  busy: false,
  connected: true,
  connecting: false,
  serverUrl: "",
  onServerUrl: noop,
  onConnect: noop,
  onSubmit: noop,
  error: "",
  notice: "",
  canResume: false,
  onCorrectEmail: noop,
  onResend: noop,
};
const authRender = (overrides) =>
  renderToStaticMarkup(
    React.createElement(AuthPage, { ...authProps, ...overrides }),
  );
html = authRender({ error: "邮箱或密码不正确，请检查后重试。" });
assert.match(html, /<label for="auth-password">密码<\/label>/);
assert.match(html, /<input(?=[^>]*id="auth-password")(?=[^>]*name="password")(?=[^>]*type="password")[^>]*>/);
assert.match(html, /<input(?=[^>]*name="email")(?=[^>]*type="email")[^>]*>/);
assert.match(html, /role="alert"[^>]*>邮箱或密码不正确/);
location.hash = "#/register?invite=invitation-token";
html = authRender({ mode: "register" });
assert.match(html, /name="invite"[^>]*value="invitation-token"/);
assert.match(html, /name="name"/);
assert.match(html, /autoComplete="new-password"/i);
html = authRender({ mode: "forgot" });
assert.match(html, /name="email"/);
assert.doesNotMatch(html, /name="password"/);
location.hash = "#/reset?token=reset-token";
html = authRender({ mode: "reset" });
assert.match(html, /name="token"[^>]*value="reset-token"/);
assert.match(html, /name="password"/);
assert.doesNotMatch(html, /name="email"/);
html = authRender({ mode: "verify", canResume: true });
assert.match(html, /name="token"/);
assert.match(html, /重新发送邮件/);
assert.match(html, /更正邮箱/);
html = authRender({ connected: false });
assert.match(html, /class="social-button primary auth-submit" disabled/);
console.log(
  "PASS: cloud uses original sidebar/list/profile/message/composer structure, Markdown rendering, native-free settings and explicit pending retry and accessible authentication fields",
);

const ai1 = { id: "ai1", name: "蓝雀", bio: "", kind: "ai", status: "active" };
const ai2 = { ...ai1, id: "ai2", name: "白鹭" };
const group = { ...conversation, id: "group", kind: "group", name: "共同讨论", peer: null,
  members: [{ account: me, role: "owner" }, { account: peer, role: "member" }, { account: ai1, role: "member" }, { account: ai2, role: "member" }] };
const groupProps = { active: group.id, snapshot: { ...p.snapshot, conversations: [group] },
  page: { ...p.page, messages: [{ ...message, conversationId: group.id, senderId: "former", senderName: "已退出的朋友", replyTo: "old-message", mentions: [ai1.id, ai2.id], aiResponses: [{ aiId: ai1.id, taskId: "one", status: "running" }, { aiId: ai2.id, taskId: "two", status: "queued" }] }] },
  record: { draft: { ...p.record.draft, mentions: [ai1.id, ai2.id], replyTo: message.id }, pending: null } };
html = render(groupProps);
assert.match(html, /共同讨论/);
assert.match(html, /群聊 · 4 位成员/);
assert.match(html, /已退出的朋友/);
assert.match(html, /蓝雀：AI 正在回复/);
assert.match(html, /白鹭：等待 AI 回复/);
assert.equal((html.match(/停止回复/g) || []).length, 2);
assert.match(html, /取消回复/);
assert.match(html, /提及成员/);
assert.equal((html.match(/type="checkbox" checked=""/g) || []).length, 2);
html = render({ ...groupProps, record: { ...groupProps.record, pending: { ...groupProps.record.draft, requestId: "pending-group" } } });
assert.match(html, /<fieldset[^>]*disabled/);
html = render({ snapshot: { ...p.snapshot, conversations: [{ ...conversation, directBlocked: true }] } });
assert.match(html, /双方私聊已屏蔽/);
assert.match(html, /<button[^>]*aria-label="发送消息"[^>]*disabled/);
html = render({ snapshot: { ...p.snapshot, conversations: [{ ...conversation, readOnly: true }] }, page: { ...p.page, messages: [{ ...message, senderId: me.id, importOrigin: { importId: "batch", sourceMessageId: "old-message", sourceRevision: "1" } }] } });
assert.match(html, /导入历史/);
assert.match(html, /只读聊天，可以查看、搜索和导出记录/);
assert.doesNotMatch(html, /消息输入框|发送消息|添加图片|添加文件|>编辑<|>撤回</);
const { GroupDetails, ContactBlock } = load("cloud/Groups");
const details = (c, meId) => renderToStaticMarkup(React.createElement(GroupDetails, { conversation: c, me: meId, busy: false, act: asyncNoop, refresh: asyncNoop, leave: noop }));
html = details(group, me.id);
assert.match(html, /保存群名称/); assert.match(html, /添加成员/); assert.match(html, /转让群主/); assert.match(html, /群角色/);
assert.doesNotMatch(html, />退出群聊</);
assert.equal((html.match(/>转让群主<\/button>/g) || []).length, 1, "owner can transfer only to the other human");
html = details(group, peer.id);
assert.match(html, /退出群聊/); assert.doesNotMatch(html, /添加成员|保存群名称|转让群主|群角色|>移出</);
html = details({ ...group, members: group.members.map(m => m.account.id === peer.id ? { ...m, role: "admin" } : m) }, peer.id);
assert.match(html, /添加成员/); assert.match(html, /移出/); assert.doesNotMatch(html, /转让群主|群角色/);
html = renderToStaticMarkup(React.createElement(ContactBlock, { account: peer, act: asyncNoop, refresh: asyncNoop, busy: false }));
assert.match(html, /共同群聊仍可正常使用/);
console.log("PASS: real group conversations, historical sender names, independent AI tasks, structured draft targets, pending lock and group roles");

html = render({ ...groupProps, record: { draft: { ...groupProps.record.draft, mentions: ["former-member"] }, pending: null } });
assert.match(html, /移除已离群提及：former-member/);

const { ConsciousnessPanel } = load("cloud/Consciousness");
const consciousness = ids => renderToStaticMarkup(React.createElement(ConsciousnessPanel, { conversation: group, cursor: "1", summaryAiIds: ids, summaryError: "" }));
html = consciousness([]); assert.match(html, /当前窗口记忆/); assert.doesNotMatch(html, /私有窗口摘要|摘要设置/);
html = consciousness([ai1.id]); assert.match(html, /蓝雀 · 私有窗口摘要/); assert.match(html, /仅其管理员且仍有本窗口访问权/); assert.doesNotMatch(html, /本群当前成员共享/);
console.log("PASS: cloud memory is member scoped and summaries are private per AI with an administrator-only entry");

const { ChatConnectionDetails } = load("cloud/ChatConnection");
const aiDirect = { ...conversation, peer: ai1, name: "一个足够长的聊天名称用于确认标题保持单行省略", members: [{ account: me, role: "member" }, { account: ai1, role: "member" }] };
const encryptedDirect = { ...aiDirect, encryption: { version: 1, activationPosition: "1", historyComplete: false } };
const encryptedGroup = { ...group, encryption: encryptedDirect.encryption };
const localChat = { ai: ai1, human: me, conversation: { id: conversation.id }, messages: [], hasMore: false, syncState: "synced", error: null, running: false };
const connectionProps = { conversation: aiDirect, meId: me.id, hasPlaintextMessages: false, pending: false, act: fn => fn(), reconnect: asyncNoop, enableEncryption: asyncNoop, prepareLocal: asyncNoop, syncLocal: asyncNoop, closeLocal: asyncNoop, stopLocal: asyncNoop };
const renderConnection = overrides => renderToStaticMarkup(React.createElement(ChatConnectionDetails, { ...connectionProps, ...overrides }));
function connectionButton(tree, text) {
  for (const child of React.Children.toArray(tree)) {
    if (!React.isValidElement(child)) continue;
    if (child.type === "button" && renderToStaticMarkup(child).includes(text)) return child;
    const nested = connectionButton(child.props.children, text);
    if (nested) return nested;
  }
}
const encryptedProps = { snapshot: { ...p.snapshot, conversations: [encryptedDirect] }, page: { ...p.page, messages: [{ ...message, sealed: {} }] }, localSelected: true, localChat: { ...localChat, running: true }, cryptoUnlocked: true, stopLocal: asyncNoop };
html = render(encryptedProps);
const header = html.match(/<header class="conversation-header cloud-chat-header[^]*?<\/header>/)[0];
assert.match(header, /title="一个足够长的聊天名称用于确认标题保持单行省略"/);
assert.match(header, /端到端加密/);
assert.match(header, /同机私聊/);
assert.match(header, /aria-label="停止本机回复"/);
assert.match(header, /aria-label="聊天详情"/);
assert.match(header, /aria-label="返回消息列表"/);
assert.match(header, /class="social-avatar[^>]+>[^]*?<\/span><span class="cloud-chat-title">/);
assert.doesNotMatch(header, /分享历史密钥|开启同机私聊|新成员或新设备|未迁移|返回云端发送/);
desktop.CUSTOM_WINDOW_CHROME = true;
const desktopHeader = render(encryptedProps).match(/<header class="conversation-header cloud-chat-header[^]*?<\/header>/)[0];
assert.match(desktopHeader, /class="conversation-heading" data-tauri-drag-region="false"/);
assert.match(desktopHeader, /class="cloud-chat-drag-spacer" aria-hidden="true" data-tauri-drag-region="true"/);
assert.match(desktopHeader, /aria-label="聊天详情"[^>]*data-tauri-drag-region="false"/);
const managedAi = { account: ai1, role: "initial", profileRevision: "1", bindingVersion: "1", statusRevision: "1", node: null };
const { AiManagement } = load("cloud/AiManagement"), { AiDetails } = load("cloud/AiDetails");
for (const [name, markup] of [
  ["account profile", render({ tab: "account" })],
  ["appearance settings", render({ tab: "settings" })],
  ["AI management", renderToStaticMarkup(React.createElement(AiManagement, { operatorId: me.id, conversations: [], cursor: "1", back: noop, openDirect: asyncNoop, refresh: asyncNoop }))],
  ["AI details", renderToStaticMarkup(React.createElement(AiDetails, { ai: managedAi, operatorId: me.id, conversations: [], cursor: "1", back: noop, changed: asyncNoop }))],
]) {
  assert.match(markup, /<header class="contact-header" data-tauri-drag-region="deep"/, `${name} retains a native blank header drag region`);
  assert.match(markup, /<div class="contact-scroll">/, `${name} keeps its selectable content outside the header`);
}
assert.match(authRender({}), /<header class="auth-header" data-tauri-drag-region="deep"/);
desktop.CUSTOM_WINDOW_CHROME = false;
assert.doesNotMatch(render({ tab: "account" }), /data-tauri-drag-region="deep"/);
assert.doesNotMatch(authRender({}), /data-tauri-drag-region="deep"/);
html = render({ ...encryptedProps, localChat: { ...localChat, syncState: "conflict" } });
assert.match(html, /云端同步冲突 · 本机记录已保留/);
assert.match(html, /查看详情/);
html = render({ ...encryptedProps, localChat: undefined });
assert.match(html, /本机连接中断 · 发送仍使用本机/);
html = render({ snapshot: { ...p.snapshot, requireEncryption: true } });
assert.match(html, /开启端到端加密后即可发送/);
assert.match(html, /设置加密/);

html = renderConnection({ conversation: encryptedDirect, cryptoUnlocked: false });
assert.match(html, /旧历史转换尚未确认完成/);
assert.doesNotMatch(html, /含尚未转换的明文|旧记录尚未迁移/);
assert.doesNotMatch(html, /分享历史密钥|新成员或新设备|所有当前成员须先完成设备核验/);
assert.match(html, /新设备请在“我的账号 → 可信设备与恢复”使用恢复文件或已有可信设备授权恢复/);
html = renderConnection({ conversation: encryptedDirect, cryptoUnlocked: true, hasPlaintextMessages: true });
assert.match(html, /当前已加载的消息中含尚未转换的明文记录/);
assert.doesNotMatch(html, /分享历史密钥|新成员或新设备/);
html = renderConnection({ conversation: encryptedGroup, cryptoUnlocked: false });
assert.match(html, /<button[^>]*disabled[^>]*>分享历史密钥/);
assert.match(html, /新成员或新设备缺少历史密钥/);
html = renderConnection({ conversation: encryptedGroup, cryptoUnlocked: true });
assert.match(html, /<button(?![^>]*disabled)[^>]*>分享历史密钥/);
assert.doesNotMatch(html, /新设备请在/);
assert.match(renderConnection({ conversation: { ...encryptedDirect, encryption: { ...encryptedDirect.encryption, historyComplete: true } } }), /旧历史已转换/);
assert.doesNotMatch(renderConnection({ conversation: { ...aiDirect, readOnly: true } }), />开启端到端加密<\/button>/);
assert.doesNotMatch(renderConnection({ conversation: { ...group, members: group.members.map(m => ({ ...m, role: "member" })) } }), />开启端到端加密<\/button>/);
assert.match(renderConnection({ conversation: group }), />开启端到端加密<\/button>/);
assert.match(renderConnection({}), /开启同机私聊/);
assert.doesNotMatch(renderConnection({ conversation: group }), /开启同机私聊/);
assert.match(renderConnection({ localSelected: true }), /重新连接本机/);
assert.match(renderConnection({ localSelected: true, localChat }), /<button(?![^>]*disabled)[^>]*>返回云端发送/);
for (const overrides of [{ localChat: { ...localChat, running: true } }, { localChat: { ...localChat, syncState: "conflict" } }, { localChat: { ...localChat, syncState: "pending" } }, { localChat, pending: true }]) {
  assert.match(renderConnection({ localSelected: true, ...overrides }), /<button[^>]*disabled[^>]*>返回云端发送/);
}
let enabled = 0, prepared = 0, synced = 0, closed = 0, stopped = 0, reconnected = 0;
let tree = ChatConnectionDetails({ ...connectionProps, enableEncryption: async () => { enabled++; }, prepareLocal: async () => { prepared++; } });
confirmEnable = false;
connectionButton(tree, "开启端到端加密").props.onClick();
assert.equal(enabled, 0);
confirmEnable = true;
connectionButton(tree, "开启端到端加密").props.onClick();
connectionButton(tree, "开启同机私聊").props.onClick();
assert.equal(enabled, 1); assert.equal(prepared, 1);
tree = ChatConnectionDetails({ ...connectionProps, localSelected: true, localChat: { ...localChat, running: true }, syncLocal: async () => { synced++; }, stopLocal: async () => { stopped++; } });
connectionButton(tree, "重试云端同步").props.onClick();
connectionButton(tree, "停止本机回复").props.onClick();
assert.equal(synced, 1); assert.equal(stopped, 1);
tree = ChatConnectionDetails({ ...connectionProps, localSelected: true, localChat, closeLocal: async () => { closed++; } });
connectionButton(tree, "返回云端发送").props.onClick();
assert.equal(closed, 1);
tree = ChatConnectionDetails({ ...connectionProps, localSelected: true, reconnect: async () => { reconnected++; } });
connectionButton(tree, "重新连接本机").props.onClick();
assert.equal(reconnected, 1);
console.log("PASS: compact chat header, reachable encryption/local actions, trusted-key gating, group permissions, explicit history status and cloud-return guards");

// Exercise the current composer callbacks, retaining valuable old host assertions.
const interactionExports = {};
vm.runInNewContext(ts.transpileModule(fs.readFileSync(path.join(root, 'cloud/Workspace.tsx'), 'utf8'), {
 compilerOptions: {jsx: ts.JsxEmit.ReactJSX, target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.CommonJS}
}).outputText, {exports: interactionExports, location, URL, console, setTimeout, clearTimeout,
 require(spec) {
  if(spec==='react')return {...React,useState:value=>[value,()=>{}],useRef:value=>({current:value}),useEffect(){},useMemo:fn=>fn(),useCallback:fn=>fn};
  if(spec==='@/lib/desktop')return desktop;
  if(spec==='@/store/ui')return {useUiStore:fn=>fn(ui)};
  if(spec.endsWith('.css'))return {};
  if(spec.startsWith('@/'))return load(spec.slice(2));
  if(spec.startsWith('.'))return load(path.relative(root,path.resolve(root,'cloud',spec)).replaceAll('\\','/'));
  return require(spec);
 }
});
const nodes=node=>!node||typeof node!=='object'?[]:[node,...[node.props?.children].flat(Infinity).flatMap(nodes)];
let sends=0,changes=[],saved=[];
const picture={id:'img',name:'中文.png',mime:'image/png',data:'AQID',byteLength:3};
const generic={id:'file',name:'notes.txt',mime:'text/plain',data:'BAUG',byteLength:3};
const interactionProps={...p,record:{draft:{...p.record.draft,images:[picture],files:[generic]},pending:null},
 send:()=>sends++,attach:(files,image)=>changes.push({files,image}),save:record=>saved.push(record)};
const interaction=overrides=>nodes(interactionExports.Workspace({...interactionProps,...overrides}));
let elements=interaction(),editor=elements.find(n=>n.type==='textarea'&&n.props['aria-label']==='消息输入框');
for(const event of [{isComposing:true,keyCode:13},{isComposing:false,keyCode:229}]) editor.props.onKeyDown({key:'Enter',shiftKey:false,nativeEvent:{isComposing:event.isComposing},keyCode:event.keyCode,preventDefault(){throw Error('IME must stay native')}});
assert.equal(sends,0);editor.props.onKeyDown({key:'Enter',shiftKey:false,nativeEvent:{isComposing:false},keyCode:13,preventDefault(){}});assert.equal(sends,1);
let prevented=false;editor.props.onPaste({clipboardData:{items:[{kind:'string'}]},preventDefault(){prevented=true}});assert.equal(prevented,false);
const selection={type:'image/png',name:'clipboard.png'};
editor.props.onPaste({clipboardData:{items:[{kind:'file',getAsFile:()=>selection}]},preventDefault(){prevented=true}});assert.equal(prevented,true);assert.equal(changes.at(-1).files[0],selection);
elements.find(n=>n.props?.onDrop).props.onDrop({dataTransfer:{files:[selection]},preventDefault(){}});assert.equal(changes.at(-1).files[0],selection);
const chooser=elements.find(n=>n.type==='input'&&n.props.type==='file');const choice={target:{files:[selection],value:'selected'}};chooser.props.onChange(choice);assert.equal(choice.target.value,'');
elements.find(n=>n.props?.label==='移除图片 中文.png').props.onClick();assert.equal(saved.at(-1).draft.images.length,0);assert.equal(saved.at(-1).draft.files[0].id,'file');
elements.find(n=>n.props?.label==='移除文件 notes.txt').props.onClick();assert.equal(saved.at(-1).draft.files.length,0);assert.equal(saved.at(-1).draft.images[0].id,'img');
assert.equal(interaction({record:{draft:{...p.record.draft,text:'',images:[],files:[generic]},pending:null}}).find(n=>n.props?.['aria-label']==='发送消息').props.disabled,false,'file-only messages remain sendable');
assert.equal(interaction({record:{...interactionProps.record,pending:{...interactionProps.record.draft,requestId:'frozen'}}}).find(n=>n.type==='textarea'&&n.props['aria-label']==='消息输入框').props.disabled,true,'pending commands lock editable input');
html=render({page:{...p.page,messages:[{...message,attachments:[{index:0,kind:'file',name:'<script>alert(1)</script>.txt',mime:'text/plain',byteLength:3}]}]}});
assert.match(html,/&lt;script&gt;/);assert.doesNotMatch(html,/<script>alert/);assert.match(html,/3 B/);
console.log('PASS: current Workspace IME/paste/drop/file selection, file-only sends, attachment removal, pending editing lock and escaped attachment metadata');
