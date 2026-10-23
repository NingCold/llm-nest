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
      console,
      setTimeout,
      clearTimeout,
      require: (spec) => {
        if (spec === "@/lib/desktop")
          return { CUSTOM_WINDOW_CHROME: false, IS_DESKTOP: false };
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
