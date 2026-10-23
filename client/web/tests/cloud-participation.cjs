const assert = require("node:assert/strict"), fs = require("node:fs"), vm = require("node:vm"), path = require("node:path"), ts = require("typescript"), React = require("react"), { randomUUID } = require("node:crypto");
class HttpError extends Error { constructor(status, message) { super(message); this.status = status; } }
const tick = () => new Promise(resolve => setImmediate(resolve));
const defer = () => { let resolve; const promise = new Promise(r => { resolve = r; }); return { promise, resolve }; };
const policy = { mode: "mentionedOnly", directMergeMs: 800, maxConsecutiveAi: 3, maxBoutCalls: 12, maxDailyCalls: 48, idleSeconds: 300 };
const intent = { id: "intent", aiId: "ai", creatorId: "self", content: "明天提醒我", dueAt: "2027-01-02T10:00:00Z", expiresAt: "2027-01-02T11:00:00Z", utcOffsetMinutes: 480, status: "pending", revision: "4", reason: null, taskId: null };
const initial = { revision: "1", canManage: false, policy, state: { paused: false, pausedAiIds: [], boutId: null, consecutiveAi: 2, boutCalls: 4, dailyCalls: 5, reason: "turn_limit" }, settings: { revision: "2", enabled: true, quietUntil: null, quietAction: "defer" }, intents: [intent] };
const conversation = { id: "window", kind: "group", members: [{ account: { id: "ai", name: "伙伴", kind: "ai", status: "active" } }, { account: { id: "self", name: "自己", kind: "human", status: "active" } }] };
function harness() {
  const slots = [], effects = [], calls = [];
  let index = 0;
  const hooks = { ...React,
    useState(initial) { const i = index++; if (!(i in slots)) slots[i] = typeof initial === "function" ? initial() : initial; return [slots[i], value => { slots[i] = typeof value === "function" ? value(slots[i]) : value; }]; },
    useRef(initial) { const i = index++; return slots[i] ||= { current: initial }; },
    useEffect(fn, deps) { const i = index++, previous = slots[i]; if (!previous || deps.some((d, n) => d !== previous.deps[n])) { previous?.cleanup?.(); const record = { deps }; slots[i] = record; effects.push(() => { record.cleanup = fn(); }); } },
  };
  const api = { HttpError, server: "https://example.test", async request(path, method = "GET", body) { calls.push(structuredClone({ path, method, body })); return api.respond(path, method, body); }, respond: async () => structuredClone(initial) };
  const exports = {};
  vm.runInNewContext(ts.transpileModule(fs.readFileSync(path.join(__dirname, "../src/cloud/Participation.tsx"), "utf8"), { compilerOptions: { jsx: ts.JsxEmit.ReactJSX, target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.CommonJS } }).outputText + "\nexports.PolicyEditor = PolicyEditor; exports.SettingsEditor = SettingsEditor; exports.IntentEditor = IntentEditor;", {
    exports, crypto: { randomUUID }, require(name) { if (name === "react") return hooks; if (name === "./api") return api; return require(name); },
  });
  return { exports, api, calls, remount() { slots.forEach(slot => slot?.cleanup?.()); slots.length = 0; }, render(fn, ...props) { index = 0; return fn(...props); }, async effects() { while (effects.length) effects.shift()(); await tick(); } };
}
function all(node, predicate, result = []) { if (node && typeof node === "object") { if (predicate(node)) result.push(node); React.Children.forEach(node.props?.children, child => all(child, predicate, result)); } return result; }
const text = node => typeof node === "string" || typeof node === "number" ? String(node) : React.Children.toArray(node?.props?.children).map(text).join("");
const button = (tree, label) => { const result = all(tree, node => node.type === "button" && text(node) === label); assert.equal(result.length, 1, label); return result[0]; };
const change = (node, value) => node.props.onChange({ target: { value } });
const submit = tree => all(tree, node => node.type === "form")[0].props.onSubmit({ preventDefault() {} });
(async () => {
  const h = harness(), read = (cursor = "1") => h.render(h.exports.useParticipation, "window", "self", cursor);
  read(); await h.effects(); assert.equal(read().view.revision, "1");
  h.api.respond = async (_path, method) => { if (method === "GET") return structuredClone(initial); throw new Error("lost response"); };
  await read().execute("/contact-intents", "POST", { aiId: "ai", content: "原始内容", dueAt: intent.dueAt, expiresAt: intent.expiresAt, utcOffsetMinutes: 480 }, "创建一次性约定");
  const original = h.calls.at(-1);
  assert.equal(read().pending.body.content, "原始内容");
  h.remount(); read("2"); await h.effects();
  assert.equal(read("2").reviewed, false, "automatic SSE reload cannot mark an unknown command reviewed");
  h.api.respond = async (_path, method) => { if (method === "GET") return structuredClone(initial); throw new HttpError(409, "version conflict after lost response"); };
  await read("2").execute("/contact-settings", "PUT", { enabled: false }, "不能替换原请求");
  assert.deepEqual(h.calls.at(-1), original, "retry after navigation uses the entire original command and path");
  assert.ok(read("2").pending, "a later rejection does not disprove an earlier unknown write");
  await read("2").load(true); assert.equal(read("2").reviewed, true);
  read("2").release(); assert.equal(read("2").pending, null);
  h.api.respond = async (_path, method) => { if (method === "GET") return structuredClone(initial); throw new HttpError(409, "revision conflict"); };
  await read("2").execute("/participation/control", "POST", { expectedRevision: "1", action: "pause" });
  assert.equal(read("2").pending, null, "a first explicit conflict allows a fresh user command");
  h.api.respond = async (_path, method) => { if (method === "GET") return structuredClone(initial); throw new Error("lost cancel result"); };
  await read("2").execute("/contact-intents/intent/cancel", "POST", { expectedRevision: "4" });
  const cancelCommand = h.calls.at(-1); h.api.respond = async () => structuredClone(initial);
  await read("2").execute(); assert.deepEqual(h.calls.at(-2), cancelCommand); assert.equal(read("2").pending, null);

  const session = harness(), oldWrite = defer();
  const s = () => session.render(session.exports.useParticipation, "window", "self", "1");
  s(); await session.effects(); session.api.respond = async () => oldWrite.promise;
  const oldExecution = s().execute("/contact-intents", "POST", { content: "old session" });
  session.remount(); session.exports.clearParticipationCommands(); session.api.respond = async () => initial;
  s(); await session.effects(); session.api.respond = async () => { throw new Error("new session lost response"); };
  await s().execute("/contact-intents", "POST", { content: "new session" });
  oldWrite.resolve(intent); await oldExecution;
  session.remount(); assert.equal(s().pending.body.content, "new session", "an old session completion cannot clear a new session's command");

  const late = harness(), first = defer(); late.api.respond = async () => first.promise;
  const l = cursor => late.render(late.exports.useParticipation, "window", "self", cursor);
  l("1"); await late.effects(); late.api.respond = async () => ({ ...initial, revision: "3" });
  l("2"); await late.effects(); first.resolve({ ...initial, revision: "old" }); await tick();
  assert.equal(l("2").view.revision, "3", "late snapshot cannot replace a newer SSE state");
  const background = defer(); late.api.respond = async () => background.promise;
  l("3"); await late.effects(); assert.equal(l("3").loading, false, "stream updates do not repeatedly lock an open editor"); background.resolve(initial); await tick();
  late.api.respond = async () => { throw new Error("temporary offline"); }; await l("3").load();
  assert.equal(l("3").view.revision, "1", "transient network failures retain the view and mounted form drafts");
  assert.match(l("3").error, /temporary offline/);

  const member = harness(), props = { conversation, operatorId: "self", cursor: "1" };
  member.render(member.exports.ParticipationPanel, props); await member.effects();
  let tree = member.render(member.exports.ParticipationPanel, props);
  assert.equal(all(tree, node => node.type === "button" && /编辑接话|暂停窗口|结束当前/.test(text(node))).length, 0, "ordinary members cannot edit global controls despite AI access");
  assert.equal(button(tree, "新建一次性约定").props.disabled, false);
  assert.equal(button(tree, "取消这次约定").props.disabled, false);
  assert.match(text(tree), /已达到本次接话上限/); assert.match(text(tree), /今日自动调用（UTC）5 \/ 48/);
  assert.equal(member.exports.canCancelIntent({ ...intent, creatorId: "other" }, "self", false), false);
  assert.equal(member.exports.canCancelIntent({ ...intent, creatorId: "other" }, "self", true), true);
  assert.equal(member.exports.canCancelIntent({ ...intent, status: "uncertain" }, "self", true), false);
  member.api.respond = async () => ({ ...initial, canManage: true });
  const p2 = { ...props, cursor: "2" }; member.render(member.exports.ParticipationPanel, p2); await member.effects(); tree = member.render(member.exports.ParticipationPanel, p2);
  button(tree, "暂停窗口接话").props.onClick(); await tick();
  const pause = member.calls.find(call => call.method === "POST"); assert.equal(pause.path, "/chat/conversations/window/participation/control"); assert.equal(pause.body.action, "pause"); assert.equal(pause.body.expectedRevision, "1"); assert.equal("aiId" in pause.body, false);
  tree = member.render(member.exports.ParticipationPanel, p2); button(tree, "暂停 伙伴").props.onClick(); await tick();
  assert.equal(member.calls.findLast(call => call.method === "POST").body.aiId, "ai");

  const editor = harness(), saved = [];
  const state = { view: { canManage: true, settings: { enabled: true } }, busy: false, pending: null, loading: false, execute: async (...args) => { saved.push(args); return true; } };
  let editorProps = { value: policy, revision: "frozen", group: true, state, done() {} };
  tree = editor.render(editor.exports.PolicyEditor, editorProps);
  change(all(tree, n => n.type === "input")[0], "5");
  editorProps = { ...editorProps, state: { ...state, view: { ...state.view, revision: "new-SSE" } } };
  tree = editor.render(editor.exports.PolicyEditor, editorProps); await submit(tree);
  assert.equal(saved[0][2].policy.maxConsecutiveAi, 5); assert.equal(saved[0][2].expectedRevision, "frozen", "SSE cannot rewrite the user's form or its optimistic revision");

  const time = harness().exports, now = new Date("2027-01-01T00:00:00Z"), due = new Date("2027-01-02T03:04:00Z"), expiry = new Date(due.getTime() + 3600000);
  const converted = time.contactTimes(time.localDateTime(due), time.localDateTime(expiry), now);
  assert.equal(converted.dueAt, due.toISOString()); assert.equal(converted.expiresAt, expiry.toISOString()); assert.equal(converted.utcOffsetMinutes, -now.getTimezoneOffset());
  assert.throws(() => time.contactTimes(time.localDateTime(now), time.localDateTime(expiry), now), /未来一年/);
  assert.throws(() => time.contactTimes(time.localDateTime(due), time.localDateTime(new Date(due.getTime() + 8 * 86400000)), now), /最多延后七天/);
  assert.throws(() => time.contactTimes(time.localDateTime(new Date("2028-01-02T00:00:00Z")), time.localDateTime(new Date("2028-01-02T01:00:00Z")), now), /未来一年/);
  const previousTimezone = process.env.TZ; process.env.TZ = "America/New_York";
  try { assert.throws(() => time.contactTimes("2027-03-14T02:30", "2027-03-14T04:30", now), /夏令时/); }
  finally { if (previousTimezone === undefined) delete process.env.TZ; else process.env.TZ = previousTimezone; }
  for (const code of ["node_call_limit", "node_token_limit", "budget_clock_rollback", "token_limit", "automatic_quota", "authorization_closed", "authorization_changed", "server_restarted", "node_restart_uncertain"]) assert.notEqual(time.participationReason(code), code, `common state reason ${code} is readable Chinese`);

  const settings = harness(), settingWrites = [];
  const settingProps = { value: initial.settings, state: { ...state, execute: async (...args) => { settingWrites.push(args); return true; } }, done() {} };
  tree = settings.render(settings.exports.SettingsEditor, settingProps);
  change(all(tree, n => n.type === "select")[0], "skip"); tree = settings.render(settings.exports.SettingsEditor, settingProps); await submit(tree);
  assert.equal(settingWrites[0][0], "/contact-settings"); assert.equal(settingWrites[0][2].quietAction, "skip"); assert.equal(settingWrites[0][2].expectedRevision, "2");
  console.log("PASS: cloud participation permissions, stable editors and retries, UTC contact validation, and quiet-time commands");
})().catch(error => { console.error(error); process.exitCode = 1; });
