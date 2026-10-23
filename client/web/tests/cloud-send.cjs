const assert = require("node:assert/strict"), fs = require("node:fs"), vm = require("node:vm"), path = require("node:path"), ts = require("typescript"), React = require("react"), { randomUUID } = require("node:crypto");
class HttpError extends Error { constructor(status) { super(`HTTP ${status}`); this.status = status; } }
const me = { id: "me", name: "自己", kind: "human", status: "active" };
const conversation = { id: "group", kind: "group", revision: "7", directBlocked: false };
const draft = { text: "@名字只是文字", requestId: null, mentions: ["ai-b", "ai-a"], replyTo: "earlier", reference: { actorId: "ai-a", selection: { conversationId: "source-group", sources: [{ messageId: "group-source", revision: "3" }] } }, images: [{ id: "image", mime: "image/png", data: "YQ==" }], files: [] };
function harness(record, desktop = false, storage = new Map()) {
  const states = [], refs = [], calls = [], saves = [], effects = [], listeners = {};
  const repository = { records: {} };
  let si = 0, ri = 0;
  // Seed an authenticated, open conversation and a recovered draft. Effects stay off:
  // these cases exercise explicit sends, without any automatic network work.
  Object.assign(states, { 4: me, 5: true, 8: { self: me, contacts: [], conversations: [conversation], cursor: "1" }, 9: "group", 15: record, 16: true });
  const hooks = { ...React,
    useState(initial) { const i = si++; if (!(i in states)) states[i] = typeof initial === "function" ? initial() : initial; return [states[i], value => { states[i] = typeof value === "function" ? value(states[i]) : value; }]; },
    useRef(initial) { const i = ri++; return refs[i] ||= { current: initial }; }, useEffect(fn) { if (desktop && String(fn).includes("drafts-prepare-close")) effects.push(fn); },
  };
  const api = { server: "https://example.test", encryptions: 0, invalidate() {}, HttpError, DownloadCancelled: class extends Error {}, async request(path, method, body) { calls.push({ path, method, body }); return api.respond(path, method, body); }, respond: async () => ({}) };
  const exports = {};
  vm.runInNewContext(ts.transpileModule(fs.readFileSync(path.join(__dirname, "../src/cloud/App.tsx"), "utf8"), { compilerOptions: { jsx: ts.JsxEmit.ReactJSX, target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.CommonJS } }).outputText, {
    exports, location: { hash: "" }, crypto: { randomUUID }, URL, setTimeout, window: { localStorage: { getItem: key => storage.get(key) ?? null, setItem: (key, value) => storage.set(key, value), removeItem: key => storage.delete(key) }, addEventListener() {}, removeEventListener() {} },
    require(name) {
      if (name === "react") return hooks;
      if (name === "./api") return api;
      if (name === "./local-api") return {
        sendLocal: async (id, requestId, text, sealed, messageId, keyMessages, options) => { calls.push({ local: true, id, requestId, text, sealed, messageId, keyMessages, options }); throw new Error("local acknowledgement lost"); },
        syncLocal: async () => api.localRespond(),
        localPage: data => ({ messages: data.messages, hasMore: false, latestPosition: "0" }),
      };
      if (name === "./Workspace") return { Workspace: "workspace" };
      if (name === "./Consciousness") return { clearConsciousnessCommands() {} };
      if (name === "./AiConfiguration") return { clearAiConfigurationCommands() {} };
      if (name === "./Participation") return { clearParticipationCommands() {} };
      if (name === "./crypto/engine") return { lockCrypto() {}, cryptoDevice: () => undefined, localKeyMessages: () => [{ event_type: "m.room.encrypted", txn_id: "frozen-key", messages: { "@ai:llmn": { node: { ciphertext: "olm" } } } }] };
      if (name === "./crypto/attachments") return { prepareAttachments: (images = [], files = []) => ({ descriptors: [...images, ...files].map((a, index) => ({ index, kind: "file", mime: a.mime, byteLength: 1, mediaEncryptionInfo: "sdk-secret" })), encryptedAttachments: [...images, ...files].map((a, index) => ({ index, data: "cipher-only" })) }) };
      if (name === "./crypto/content") return { encryptMessage: async () => ({ version: 1, ciphertext: `frozen-${++api.encryptions}` }), projectPage: async p => p, projectSnapshot: async s => s };
      if (name === "@/lib/desktop") return { IS_DESKTOP: desktop };
      if (name === "@tauri-apps/api/event") return { listen: async (name, fn) => { listeners[name] = fn; return () => delete listeners[name]; } };
      if (name === "@tauri-apps/api/core") return { invoke: async () => {} };
      if (name === "../social/drafts") return { emptyDraft: () => ({ text: "", requestId: null, mentions: [], replyTo: null }) };
      if (name === "../social/draft-storage") return { DraftDatabase: class { async load(owner, ids) { return Object.fromEntries(ids.filter(id => repository.records[id]).map(id => [id, repository.records[id]])); } async save(owner, id, next) { saves.push(structuredClone({ owner, id, next })); } } };
      if (name === "@/hooks/useDesktopReady") return { useDesktopReady() {} };
      if (name.startsWith("@/") || name.startsWith("./") || name.startsWith("../") || name.startsWith("@tauri")) return {};
      return require(name);
    },
  });
  return { calls, saves, api, repository, effects, listeners, render() { si = ri = 0; return exports.default().props; } };
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
  const localPending = { text: "offline text", requestId: "local-request", route: "local", mentions: [], replyTo: null };
  const local = harness({ draft: { ...localPending, text: "later" }, pending: localPending });
  await local.render().send();
  assert.equal(local.calls.length, 1);
  assert.equal(local.calls[0].local, true, "recovered local command never falls through to cloud transport");
  assert.equal(local.calls[0].requestId, "local-request");
  assert.equal(local.render().record.pending.route, "local", "lost local acknowledgement retains route and request identity");
  const unsupportedLocal = harness({ draft, pending: { ...localPending, images: draft.images } });
  await unsupportedLocal.render().send();
  assert.equal(unsupportedLocal.calls.length, 0, "unsupported local attachment is rejected without silently dropping its bytes or cloud fallback");
  const rememberedRoute = new Map([[`llmn-local-chat-route:${JSON.stringify(["https://example.test/me", "group"])}`, "local"]]);
  const refreshedLocal = harness({ draft: localPending, pending: null }, true, rememberedRoute);
  await refreshedLocal.render().send();
  assert.equal(refreshedLocal.calls[0].local, true, "a remembered local route survives reload even without an unconfirmed command or in-memory view");
  assert.equal(refreshedLocal.render().record.pending.route, "local");
  const oldResponse = harness({ draft: localPending, pending: null }, true, rememberedRoute);
  let resolveLocal;
  oldResponse.api.localRespond = () => new Promise(resolve => { resolveLocal = resolve; });
  const beforeLogout = oldResponse.render();
  const synchronization = beforeLogout.syncLocal();
  beforeLogout.endSession();
  resolveLocal({ human: { id: "me" }, conversation: { id: "group" }, messages: [] });
  await synchronization;
  assert.equal(oldResponse.render().localChat, undefined, "an already resolved native response cannot restore local data after logout");
  conversation.encryption = { version: 1, activationPosition: "0" };
  const encrypted = harness({ draft: { text: "private words", requestId: null, mentions: [], replyTo: null }, pending: null });
  encrypted.api.respond = async () => { throw Error("lost ciphertext acknowledgement"); };
  await encrypted.render().send();
  await encrypted.render().send();
  assert.equal(encrypted.api.encryptions, 1, "unknown encrypted send retries its frozen envelope instead of advancing the SDK again");
  assert.deepEqual(encrypted.calls[0].body, encrypted.calls[1].body);
  assert.equal(encrypted.calls[0].body.text, "");
  assert.ok(encrypted.calls[0].body.messageId);
  assert.equal(JSON.stringify(encrypted.calls[0].body).includes("private words"), false, "plaintext is not part of the cloud request");
  const encryptedSocial = harness({ draft, pending: null });
  encryptedSocial.api.respond = async () => { throw Error("lost social ciphertext acknowledgement"); };
  await encryptedSocial.render().send(); await encryptedSocial.render().send();
  assert.equal(encryptedSocial.api.encryptions, 1);
  assert.deepEqual(encryptedSocial.calls[0].body, encryptedSocial.calls[1].body);
  assert.equal(encryptedSocial.calls[0].body.expectedConversationRevision, "7");
  assert.equal(JSON.stringify(encryptedSocial.calls[0].body.images), "[]");
  assert.equal(encryptedSocial.calls[0].body.encryptedAttachments[0].data, "cipher-only");
  assert.equal(JSON.stringify(encryptedSocial.calls[0].body).includes("YQ=="), false, "original attachment bytes never join sealed requests");
  delete conversation.encryption;
  const frozenLocal = { ...localPending, encryptedAttachments: [{ index: 0, data: "frozen-cipher" }], expectedConversationRevision: "7", replyTo: "earlier", messageId: "encrypted-local", sealed: { version: 1, ciphertext: "megolm" }, keyMessages: [{ event_type: "m.room.encrypted", txn_id: "original-olm", messages: { "@ai:llmn": { node: { ciphertext: "olm" } } } }] };
  const ipc = harness({ draft: { ...frozenLocal, text: "later" }, pending: frozenLocal }, true);
  await ipc.render().send();
  await ipc.render().send();
  assert.deepEqual(ipc.calls[0], ipc.calls[1], "lost IPC ack retries frozen Megolm and Olm key package together");
  assert.deepEqual(ipc.calls[0].keyMessages, frozenLocal.keyMessages);
  assert.equal(ipc.calls[0].text, "");
  assert.deepEqual(ipc.calls[0].options.encryptedAttachments, frozenLocal.encryptedAttachments);
  assert.equal(ipc.calls[0].options.replyTo, "earlier");
  assert.equal(ipc.calls[0].options.expectedConversationRevision, "7");
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
  const desktop = harness({ draft, pending: null }, true);
  desktop.render(); const cleanup = desktop.effects[0](); await new Promise(resolve => setImmediate(resolve));
  await desktop.listeners['drafts-prepare-close']({payload:'close'});
  assert.throws(() => desktop.render().text('still closing'), /正在保存并关闭/);
  desktop.listeners['desktop-close-retained']();
  desktop.render().text('托盘重新打开后继续输入'); await new Promise(resolve => setImmediate(resolve));
  assert.equal(desktop.saves.at(-1).next.draft.text, '托盘重新打开后继续输入'); cleanup();
  console.log("PASS: cloud structured sends persist before dispatch, unlock only first explicit rejections and preserve uncertain/recovered command snapshots");
})().catch(error => { console.error(error); process.exitCode = 1; });
