const assert = require("node:assert/strict"), fs = require("node:fs"), vm = require("node:vm"), path = require("node:path"), ts = require("typescript"), React = require("react"), { randomUUID } = require("node:crypto");
class HttpError extends Error { constructor(status, message, code) { super(message); this.status = status; this.code = code; } }
const empty = { memories: [], summaries: [], settings: null, models: [] };
const tick = () => new Promise(resolve => setImmediate(resolve));
const defer = () => { let resolve; const promise = new Promise(r => { resolve = r; }); return { promise, resolve }; };
function harness(file = "Consciousness") {
  const slots = [], effects = [], calls = [];
  let index = 0;
  const hooks = { ...React,
    useState(initial) { const i = index++; if (!(i in slots)) slots[i] = typeof initial === "function" ? initial() : initial; return [slots[i], value => { slots[i] = typeof value === "function" ? value(slots[i]) : value; }]; },
    useRef(initial) { const i = index++; return slots[i] ||= { current: initial }; },
    useEffect(fn, deps) { const i = index++, previous = slots[i]; if (!previous || deps.some((d, n) => d !== previous.deps[n])) { previous?.cleanup?.(); const record = { deps }; slots[i] = record; effects.push(() => { record.cleanup = fn(); }); } },
  };
  const api = { HttpError, async request(path, method, body) { calls.push(structuredClone({ path, method, body })); return api.respond(path, method, body); }, respond: async () => empty };
  const exports = {};
  vm.runInNewContext(ts.transpileModule(fs.readFileSync(path.join(__dirname, `../src/cloud/${file}.tsx`), "utf8"), { compilerOptions: { jsx: ts.JsxEmit.ReactJSX, target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.CommonJS } }).outputText + (file === "Consciousness" ? "\nexports.testSummaries = Summaries;" : ""), {
    exports, crypto: { randomUUID }, require(name) { if (name === "react") return hooks; if (name === "./api") return api; if (name === "./crypto/content") return { projectReference: async p => p }; if (name === "./crypto/ResourcePanel") return { EncryptedResourcePanel: "encrypted-resources" }; if (name.startsWith("@/components")) return { Dialog: "dialog", DialogContent: "section", DialogTitle: "h2" }; return require(name); },
  });
  return { exports, api, calls, remount() { slots.forEach(slot => slot?.cleanup?.()); slots.length = 0; }, render(fn, ...props) { index = 0; return fn(...props); }, async effects() { while (effects.length) effects.shift()(); await tick(); } };
}
function all(node, predicate, result = []) { if (node && typeof node === "object") { if (predicate(node)) result.push(node); React.Children.forEach(node.props?.children, child => all(child, predicate, result)); } return result; }
const text = node => typeof node === "string" ? node : React.Children.toArray(node?.props?.children).map(text).join("");
const button = (tree, label) => { const result = all(tree, node => node.type === "button" && text(node) === label); assert.equal(result.length, 1, label); return result[0]; };
(async () => {
  const h = harness();
  const render = (cursor = "1") => h.render(h.exports.useConsciousness, "window", "ai", "memories", cursor);
  h.api.respond = async () => { throw new HttpError(409, "在线节点不可用", "node_offline"); };
  render(); await h.effects();
  assert.equal(render().view, null, "offline is never an empty-success view"); assert.match(render().error, /在线节点不可用/);
  const operation = { type: "createMemory", kind: "fact", text: "待确认条目", source: { messageId: "source", revision: "1" } };
  await render().execute(operation);
  assert.equal(render().uncertain, false, "first dispatch explicitly not relayed can be edited");

  h.api.respond = async () => { throw new HttpError(409, "保存后的同步未确认", "node_storage_failed"); };
  await render().execute(operation);
  const original = h.calls.at(-1).body;
  assert.equal(render().uncertain, true);
  h.remount();
  assert.equal(render().uncertain, true, "navigation back restores the unconfirmed command");
  h.api.respond = async () => { throw new HttpError(409, "来源已变化", "source_changed"); };
  await render().execute({ ...operation, text: "不能替换未知原命令" });
  assert.deepEqual(h.calls.at(-1).body, original, "retry retains request and complete original operation");
  assert.equal(render().uncertain, true, "a later explicit rejection does not disprove the original write");
  h.api.respond = async () => ({ ...empty, memories: [{ id: "saved", text: operation.text, status: "confirmed" }] });
  await render().execute();
  assert.deepEqual(h.calls.at(-1).body, original);
  assert.equal(render().uncertain, false); assert.equal(render().completed, 1);
  assert.equal(render().view.memories[0].id, "saved");
  assert.match(h.calls.at(-1).path, /^\/chat\/conversations\/window\/ai\/ai\/consciousness$/);

  const rejected = harness();
  const r = () => rejected.render(rejected.exports.useConsciousness, "window", "ai", "memories", "1");
  r(); await rejected.effects();
  rejected.api.respond = async () => { throw new HttpError(409, "来源已变化", "source_changed"); };
  await r().execute(operation); const rejectedId = rejected.calls.at(-1).body.requestId;
  assert.equal(r().uncertain, false);
  rejected.api.respond = async () => empty;
  await r().execute({ ...operation, source: null });
  assert.notEqual(rejected.calls.at(-1).body.requestId, rejectedId);

  const late = harness(), first = defer();
  late.api.respond = async () => first.promise;
  late.render(late.exports.useConsciousness, "window", "ai", "memories", "1"); await late.effects();
  late.api.respond = async () => ({ ...empty, memories: [{ id: "current" }] });
  late.render(late.exports.useConsciousness, "window", "ai", "memories", "2"); await late.effects();
  first.resolve({ ...empty, memories: [{ id: "old" }] }); await tick();
  assert.equal(late.render(late.exports.useConsciousness, "window", "ai", "memories", "2").view.memories[0].id, "current");

  const ref = harness("References"), preview = defer();
  const selection = { conversationId: "group", sources: [{ messageId: "source", revision: "1" }] };
  const props = { directId: "direct", selection, cursor: "1" };
  let tree = ref.render(ref.exports.ReferenceCard, props);
  button(tree, "群话题引用 · 1 条原文").props.onClick();
  ref.api.respond = async () => preview.promise;
  ref.render(ref.exports.ReferenceCard, props); await ref.effects();
  tree = ref.render(ref.exports.ReferenceCard, { ...props, cursor: "2", stale: true }); await ref.effects();
  preview.resolve({ conversation: { name: "旧来源" }, messages: [{ id: "source", text: "迟到旧正文", attachments: [] }] }); await tick();
  tree = ref.render(ref.exports.ReferenceCard, { ...props, cursor: "2", stale: true });
  assert.match(text(tree), /来源或成员权限已变化/); assert.doesNotMatch(text(tree), /迟到旧正文/);
  assert.equal(ref.calls.length, 1, "stale reference does not read old source again");
  const continuing = harness("References"), previewResponse = defer();
  const groupMessage = { id: "source", senderId: "ai", senderName: "A", text: "群原文", revision: "1", status: "committed" };
  const groupConversation = { id: "group", revision: "1", members: [{ account: { id: "ai", name: "A", kind: "ai", status: "active" } }] };
  let continuationResponse = defer();
  let continueCalls = 0;
  let continueProps = { conversation: groupConversation, message: groupMessage, messages: [groupMessage], cursor: "1", continueWith: async () => { continueCalls++; return continuationResponse.promise; } };
  let continuingTree = continuing.render(continuing.exports.ContinuePrivately, continueProps);
  button(continuingTree, "私聊继续").props.onClick();
  continuingTree = continuing.render(continuing.exports.ContinuePrivately, continueProps); await continuing.effects();
  continuing.api.respond = async path => path === "/chat/direct" ? { id: "direct" } : previewResponse.promise;
  const preparing = button(continuingTree, "预览所选来源").props.onClick(); await tick();
  // Opening an existing direct conversation emits an SSE cursor change too.
  continueProps = { ...continueProps, cursor: "2" };
  continuing.render(continuing.exports.ContinuePrivately, continueProps); await continuing.effects();
  previewResponse.resolve({ conversation: { name: "共同群" }, messages: [groupMessage] }); await preparing;
  continuingTree = continuing.render(continuing.exports.ContinuePrivately, continueProps);
  assert.equal(button(continuingTree, "确认预览，带入私聊草稿").props.disabled, false, "the preview survives its own direct-open notification");
  const accepting = button(continuingTree, "确认预览，带入私聊草稿").props.onClick();
  continueProps = { ...continueProps, cursor: "3" };
  continuing.render(continuing.exports.ContinuePrivately, continueProps); await continuing.effects();
  continuationResponse.resolve(Promise.reject(new Error("私聊尚有未确认发送"))); await accepting;
  continuingTree = continuing.render(continuing.exports.ContinuePrivately, continueProps);
  assert.match(text(continuingTree), /私聊尚有未确认发送/, "a notification during confirmation cannot swallow its actual failure");
  assert.equal(button(continuingTree, "确认预览，带入私聊草稿").props.disabled, false);
  continuationResponse = defer();
  const acceptingAgain = button(continuingTree, "确认预览，带入私聊草稿").props.onClick();
  continueProps = { ...continueProps, cursor: "4" };
  continuing.render(continuing.exports.ContinuePrivately, continueProps); await continuing.effects();
  continuationResponse.resolve(); await acceptingAgain;
  continuingTree = continuing.render(continuing.exports.ContinuePrivately, continueProps);
  assert.equal(all(continuingTree, node => node.type === "dialog")[0].props.open, false, "successful continuation closes even when the account cursor changed");
  assert.equal(continueCalls, 2);
  continueProps = { ...continueProps, messages: [{ ...groupMessage, revision: "2" }] };
  continuingTree = continuing.render(continuing.exports.ContinuePrivately, continueProps);
  assert.equal(button(continuingTree, "确认预览，带入私聊草稿").props.disabled, true, "a selected source revision change still invalidates preview");

  const summary = harness();
  const summaryCodes = ["source_changed", "window_changed", "settings_changed", "node_restarted", "summary_failed", "summary_interrupted", "custom-readable-message"];
  summary.api.respond = async () => ({ ...empty, settings: { revision: "1", enabled: false, binding: null }, summaries: summaryCodes.map((error, i) => ({ id: String(i), revision: "1", status: "stale", text: "", fromPosition: "1", throughPosition: "2", sources: [], error })) });
  const summaryProps = { conversationId: "window", ai: { id: "ai", name: "A" }, cursor: "1", operatorId: "human", onLock() {} };
  summary.render(summary.exports.testSummaries, summaryProps); await summary.effects();
  const summaryText = text(summary.render(summary.exports.testSummaries, summaryProps));
  for (const code of summaryCodes.slice(0, -1)) assert.equal(summaryText.includes(code), false, `known summary error ${code} is translated`);
  assert.match(summaryText, /来源消息已修改或撤回/); assert.match(summaryText, /窗口成员或权限已变化/);
  assert.match(summaryText, /摘要设置已修改/); assert.match(summaryText, /节点已重启/);
  assert.match(summaryText, /摘要整理失败/); assert.match(summaryText, /摘要整理已中断/);
  assert.match(summaryText, /custom-readable-message/, "unknown safe error text remains visible");

  const storageExports = {};
  vm.runInNewContext(ts.transpileModule(fs.readFileSync(path.join(__dirname, "../src/social/draft-storage.ts"), "utf8"), { compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.CommonJS } }).outputText, { exports: storageExports, Blob, Uint8Array, atob, btoa });
  const factory = new (require("fake-indexeddb").IDBFactory)(), legacy = { getItem: () => null };
  const database = new storageExports.DraftDatabase(factory, legacy);
  await database.load("server/human", ["direct"]);
  const reference = { actorId: "ai", selection };
  const record = { draft: { text: "保留私聊草稿", requestId: null, reference }, pending: { text: "原待确认文字", requestId: "exact-original-id", reference } };
  await database.save("server/human", "direct", record);
  const reopened = new storageExports.DraftDatabase(factory, legacy);
  const restored = (await reopened.load("server/human", ["direct"])).direct;
  assert.equal(JSON.stringify(restored.draft.reference), JSON.stringify(reference));
  assert.equal(JSON.stringify(restored.pending.reference), JSON.stringify(reference));
  assert.equal(restored.pending.requestId, "exact-original-id");
  restored.draft.reference = null;
  assert.equal(restored.pending.reference.selection.sources[0].messageId, "source", "editable removal cannot alter the restored pending reference");
  assert.deepEqual(Object.keys(restored.pending.reference.selection.sources[0]).sort(), ["messageId", "revision"], "draft reference stores IDs and revisions, without copied source text");
  console.log("PASS: online consciousness failures stay explicit, unknown mutations retry exactly, first definitive rejection unlocks, and late invalidated sources cannot reappear");
})().catch(error => { console.error(error); process.exitCode = 1; });
