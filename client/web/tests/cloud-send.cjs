const assert = require("node:assert/strict"), fs = require("node:fs"), vm = require("node:vm"), path = require("node:path"), ts = require("typescript"), React = require("react"), { randomUUID } = require("node:crypto");
class HttpError extends Error { constructor(status) { super(`HTTP ${status}`); this.status = status; } }
const me = { id: "me", name: "自己", kind: "human", status: "active" };
const conversation = { id: "group", kind: "group", directBlocked: false };
const draft = { text: "@名字只是文字", requestId: null, mentions: ["ai-b", "ai-a"], replyTo: "earlier", reference: { actorId: "ai-a", selection: { conversationId: "source-group", sources: [{ messageId: "group-source", revision: "3" }] } }, images: [{ id: "image", mime: "image/png", data: "YQ==" }], files: [] };
function harness(record) {
  const states = [], refs = [], calls = [], saves = [];
  const repository = { records: {} };
  let si = 0, ri = 0;
  // Seed an authenticated, open conversation and a recovered draft. Effects stay off:
  // these cases exercise explicit sends, without any automatic network work.
  Object.assign(states, { 4: me, 5: true, 8: { self: me, contacts: [], conversations: [conversation], cursor: "1" }, 9: "group", 15: record, 16: true });
  const hooks = { ...React,
    useState(initial) { const i = si++; if (!(i in states)) states[i] = typeof initial === "function" ? initial() : initial; return [states[i], value => { states[i] = typeof value === "function" ? value(states[i]) : value; }]; },
    useRef(initial) { const i = ri++; return refs[i] ||= { current: initial }; }, useEffect() {},
  };
  const api = { server: "https://example.test", HttpError, DownloadCancelled: class extends Error {}, async request(path, method, body) { calls.push({ path, method, body }); return api.respond(path, method, body); }, respond: async () => ({}) };
  const exports = {};
  vm.runInNewContext(ts.transpileModule(fs.readFileSync(path.join(__dirname, "../src/cloud/App.tsx"), "utf8"), { compilerOptions: { jsx: ts.JsxEmit.ReactJSX, target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.CommonJS } }).outputText, {
    exports, location: { hash: "" }, crypto: { randomUUID }, URL, setTimeout,
    require(name) {
      if (name === "react") return hooks;
      if (name === "./api") return api;
      if (name === "./Workspace") return { Workspace: "workspace" };
      if (name === "../social/drafts") return { emptyDraft: () => ({ text: "", requestId: null, mentions: [], replyTo: null }) };
      if (name === "../social/draft-storage") return { DraftDatabase: class { async load(owner, ids) { return Object.fromEntries(ids.filter(id => repository.records[id]).map(id => [id, repository.records[id]])); } async save(owner, id, next) { saves.push(structuredClone({ owner, id, next })); } } };
      if (name === "@/hooks/useDesktopReady") return { useDesktopReady() {} };
      if (name.startsWith("@/") || name.startsWith("./") || name.startsWith("../") || name.startsWith("@tauri")) return {};
      return require(name);
    },
  });
  return { calls, saves, api, repository, render() { si = ri = 0; return exports.default().props; } };
}
(async () => {
  const first = harness({ draft, pending: null });
  first.api.respond = async () => { throw new HttpError(409); };
  await first.render().send();
  assert.equal(first.saves.length, 2);
  assert.equal(first.saves[0].next.pending.replyTo, "earlier");
  assert.equal(first.saves[1].next.pending, null, "first explicit rejection unlocks editing");
  assert.deepEqual(first.saves[1].next.draft, draft, "all editable text, targets and attachments survive");
  assert.match(first.render().error, /HTTP 409/);

  const uncertain = harness({ draft, pending: null });
  uncertain.api.respond = async () => { throw new Error("connection lost"); };
  await uncertain.render().send();
  const pending = uncertain.render().record.pending;
  assert.ok(pending.requestId); assert.equal(uncertain.saves.length, 1);
  uncertain.api.respond = async () => { throw new HttpError(403); };
  await uncertain.render().send();
  assert.equal(uncertain.render().record.pending.requestId, pending.requestId, "a later access rejection cannot disprove the original commit");
  assert.equal(uncertain.saves.at(-1).next.pending.requestId, pending.requestId);
  assert.deepEqual(uncertain.calls[0].body, uncertain.calls[1].body, "manual retry sends the exact original snapshot including ordered mentions, reply and images");
  assert.equal(JSON.stringify(uncertain.calls[0].body.mentions), '["ai-b","ai-a"]');
  assert.equal(uncertain.calls[0].body.replyTo, "earlier");
  assert.deepEqual(uncertain.calls[0].body.reference, draft.reference.selection);
  assert.equal(JSON.stringify(uncertain.calls[0].body.reference).includes(draft.text), false, "reference sends identifiers and revisions only");

  const recovered = harness({ draft: { ...draft, text: "later input", mentions: ["new-ai"], replyTo: "new-reply" }, pending });
  assert.equal(recovered.calls.length, 0, "recovery never sends automatically");
  recovered.api.respond = async () => { throw new HttpError(404); };
  await recovered.render().send();
  assert.equal(recovered.calls[0].body.text, draft.text);
  assert.equal(recovered.calls[0].body.replyTo, draft.replyTo);
  assert.equal(JSON.stringify(recovered.calls[0].body.mentions), JSON.stringify(draft.mentions));
  assert.equal(recovered.render().record.draft.text, "later input");
  const target = { ...conversation, id: "direct", kind: "direct", peer: { id: "ai-a" } };
  const continuation = harness({ draft, pending: null });
  const targetDraft = { draft: { ...draft, text: "保留私聊输入", reference: null }, pending: null };
  continuation.repository.records.direct = targetDraft;
  continuation.api.respond = async path => path === "/chat/direct" ? target : path.endsWith("reference-preview") ? { conversation: { id: "source-group" }, messages: [] } : { self: me, contacts: [], conversations: [conversation, target], cursor: "2" };
  await continuation.render().continuePrivately({ id: "ai-a" }, draft.reference);
  assert.equal(continuation.saves.length, 1);
  assert.equal(continuation.saves[0].id, "direct");
  assert.equal(continuation.saves[0].next.draft.text, "保留私聊输入");
  assert.deepEqual(continuation.saves[0].next.draft.reference, draft.reference);
  assert.equal(continuation.render().active, "direct");
  assert.equal(continuation.calls.some(call => call.method === "POST" && call.path.endsWith("/messages")), false, "continuation never auto-sends");
  const blocked = harness({ draft, pending: null });
  blocked.repository.records.direct = { ...targetDraft, pending };
  blocked.api.respond = continuation.api.respond;
  await assert.rejects(blocked.render().continuePrivately({ id: "ai-a" }, draft.reference), /尚未确认的发送/);
  assert.equal(blocked.saves.length, 0, "continuation cannot replace an uncertain target command");
  const invalid = harness({ draft, pending: null });
  invalid.api.respond = async path => { if (path === "/chat/direct") return target; throw new HttpError(403); };
  await assert.rejects(invalid.render().continuePrivately({ id: "ai-a" }, draft.reference), /HTTP 403/);
  assert.equal(invalid.saves.length, 0, "preview rejection cannot put invalid source metadata into a draft");
  console.log("PASS: cloud structured sends persist before dispatch, unlock only first explicit rejections and preserve uncertain/recovered command snapshots");
})().catch(error => { console.error(error); process.exitCode = 1; });
