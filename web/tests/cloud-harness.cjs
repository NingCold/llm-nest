const assert = require("node:assert/strict"), fs = require("node:fs"), vm = require("node:vm"), path = require("node:path"), ts = require("typescript"), React = require("react"), { randomUUID } = require("node:crypto");
const { Window } = require('happy-dom');
const browser = new Window({ url: 'https://example.test' });
global.window = browser; global.document = browser.document; global.navigator = browser.navigator;
const { createRoot } = require('react-dom/client');
const { flushSync } = require('react-dom');
class HttpError extends Error { constructor(status) { super(`HTTP ${status}`); this.status = status; } }
const me = { id: "me", name: "自己", kind: "human", status: "active" };
const conversation = { id: "group", kind: "group", revision: "7", directBlocked: false };
const draft = { text: "@名字只是文字", requestId: null, mentions: ["ai-b", "ai-a"], replyTo: "earlier", reference: { actorId: "ai-a", selection: { conversationId: "source-group", sources: [{ messageId: "group-source", revision: "3" }] } }, images: [{ id: "image", mime: "image/png", data: "YQ==" }], files: [] };
const mounted = new Set();
const tick = async () => { await new Promise(resolve => setImmediate(resolve)); flushSync(() => {}); };
function deferred() { let resolve, reject; const promise = new Promise((yes, no) => { resolve = yes; reject = no; }); return { promise, resolve, reject }; }
async function harness(record = { draft, pending: null }, desktop = false, storage = new Map(), account = me, timing, configure = () => {}) {
  const calls = [], saves = [], listeners = {}, timers = new Map();
  const repository = { records: { group: record } }, lifecycle = { locks: 0, logouts: 0, imageReads: 0, fileReads: 0 };
  const target = browser.document.createElement('div'); browser.document.body.append(target);
  const events = new browser.EventTarget();
  let latest, root, disposed = false;
  // The UI boundary is a small interactive adapter. ChatApp runs with real React
  // state, effects, commits and cleanup, without depending on its hook order.
  function Workspace(props) {
    latest = props;
    return React.createElement('div', null,
      ...['send', 'lock', 'logout', 'history', 'refresh'].map(action => React.createElement('button', { key: action, 'data-action': action, onClick: () => { api.action = props[action](); } }, action)));
  }
  const snapshot = () => ({ self: account, contacts: [], conversations: [{ ...conversation }], cursor: '1' });
  const api = { server: "https://example.test", encryptions: 0, invalidate() {}, HttpError, DownloadCancelled: class extends Error {},
    async request(path, method, body) { calls.push({ path, method, body }); return api.respond(path, method, body); },
    respond: async path => path === '/chat/init' ? snapshot() : { messages: [], hasMore: false, latestPosition: '0' },
    subscribe(_cursor, update) { api.event = update; return () => { api.event = undefined; }; },
    localView: async () => { throw new Error('Agent offline'); },
  };
  const exports = {};
  configure({ api, repository, lifecycle });
  vm.runInNewContext(ts.transpileModule(fs.readFileSync(path.join(__dirname, "../src/cloud/ChatApp.tsx"), "utf8"), { compilerOptions: { jsx: ts.JsxEmit.ReactJSX, target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.CommonJS } }).outputText, {
    performance: timing, requestAnimationFrame: timing ? fn => { api.frame = fn; return 1; } : undefined, cancelAnimationFrame() { api.frame = undefined; },
    setInterval(fn, ms) { timers.set(fn, ms); return fn; }, clearInterval(fn) { timers.delete(fn); }, exports, location: { hash: "" }, crypto: { randomUUID }, URL, setTimeout,
    document: { addEventListener: (...args) => events.addEventListener(...args), removeEventListener: (...args) => events.removeEventListener(...args), hasFocus: () => false, querySelector: () => ({ scrollTop: 123 }) },
    window: { localStorage: { getItem: key => storage.get(key) ?? null, setItem: (key, value) => storage.set(key, value), removeItem: key => storage.delete(key) }, addEventListener: (...args) => events.addEventListener(...args), removeEventListener: (...args) => events.removeEventListener(...args) },
    require(name) {
      if (name === "react") return React;
      if (name === "./api") return api;
      if (name === "./local-api") return {
        localView: async id => api.localView(id),
        sendLocal: async (id, requestId, text, sealed, messageId, keyMessages, options) => { calls.push({ local: true, id, requestId, text, sealed, messageId, keyMessages, options }); if (api.sendLocalRespond) return api.sendLocalRespond(); throw new Error("local acknowledgement lost"); },
        syncLocal: async () => api.localRespond(),
        localPage: data => ({ messages: data.messages, hasMore: false, latestPosition: "0" }),
      };
      if (name === "./Workspace") return { Workspace };
      if (name === "./Consciousness") return { clearConsciousnessCommands() {} };
      if (name === "./AiConfiguration") return { clearAiConfigurationCommands() {} };
      if (name === "./Participation") return { clearParticipationCommands() {} };
      if (name === "./crypto/engine") return { receiveLocalKeys: async () => { if (api.receiveLocalKeys) await api.receiveLocalKeys(); }, lockCrypto() {}, consumeCryptoWarning() {}, cryptoDevice: () => api.unlocked ? 'device' : undefined, localKeyMessages: () => [{ event_type: "m.room.encrypted", txn_id: "frozen-key", messages: { "@ai:llmn": { node: { ciphertext: "olm" } } } }] };
      if (name === "./crypto/history") {
        const history = {};
        vm.runInNewContext(ts.transpileModule(fs.readFileSync(path.join(__dirname, '../src/cloud/crypto/history.ts'), 'utf8'), { compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 } }).outputText, {
          exports: history,
          require: name => name === '../api' ? api : name === './content' ? { projectPage: async p => api.projectPage ? api.projectPage(p) : p } : {},
        });
        return history;
      }
      if (name === "./crypto/resources") return { synchronizeResources: async () => { api.emptySyncs = (api.emptySyncs || 0) + 1; } };
      if (name === "./crypto/attachments") return { prepareAttachments: (images = [], files = []) => ({ descriptors: [...images, ...files].map((a, index) => ({ index, kind: "file", mime: a.mime, byteLength: 1, mediaEncryptionInfo: "sdk-secret" })), encryptedAttachments: [...images, ...files].map((a, index) => ({ index, data: "cipher-only" })) }) };
      if (name === "./crypto/content") return { pinWindow: c => c, projectMessage: async (m, c) => { assert.equal(m.conversationId, c.id); return m; }, encryptMessage: async () => ({ version: 1, ciphertext: `frozen-${++api.encryptions}` }), projectPage: async p => api.projectPage ? api.projectPage(p) : p, projectSnapshot: async s => s };
      if (name === "@/lib/desktop") return { IS_DESKTOP: desktop };
      if (name === "@tauri-apps/api/event") return { listen: async (name, fn) => { listeners[name] = fn; return () => delete listeners[name]; } };
      if (name === "@tauri-apps/api/core") return { invoke: async () => {} };
      if (name === "../chat/drafts") return { emptyDraft: () => ({ text: "", requestId: null, mentions: [], replyTo: null }) };
      if (name === "../chat/images") return { readImages: async (...args) => { lifecycle.imageReads++; return repository.readImages ? repository.readImages(...args) : []; } };
      if (name === "../chat/files") return { readFiles: async (...args) => { lifecycle.fileReads++; return repository.readFiles ? repository.readFiles(...args) : []; } };
      if (name === "../chat/draft-storage") return { DraftDatabase: class { async load(owner, ids) { return Object.fromEntries(ids.filter(id => repository.records[id]).map(id => [id, repository.records[id]])); } async save(owner, id, next) { saves.push(structuredClone({ owner, id, next })); if (repository.save) await repository.save(owner, id, next); repository.records[id] = structuredClone(next); } } };
      if (name === "@/hooks/useDesktopReady") return { useDesktopReady() {} };
      if (name.startsWith("@/") || name.startsWith("./") || name.startsWith("../") || name.startsWith("@tauri")) return {};
      return require(name);
    },
  });
  const props = { account, location: { active: 'group', tab: 'messages', accountSection: 'profile' }, rememberLocation() {}, onLock() { lifecycle.locks++; }, async onLogout() { lifecycle.logouts++; }, onEndSession() { h.unmount(); } };
  const element = () => React.createElement(exports.default, props);
  const h = { calls, saves, api, repository, lifecycle, listeners,
    render() { if (!disposed) flushSync(() => root.render(element())); return latest; },
    async click(action) { target.querySelector(`[data-action="${action}"]`).click(); await api.action; await tick(); },
    async poll(ms) { for (const [fn, interval] of [...timers]) if (interval === ms) fn(); await tick(); await tick(); },
    async cryptoChange() { events.dispatchEvent(new browser.Event('llmn-crypto-change')); await tick(); await tick(); },
    unmount() { if (!disposed) { disposed = true; flushSync(() => root.unmount()); target.remove(); mounted.delete(h); } },
  };
  root = createRoot(target); mounted.add(h); h.render(); await tick(); await tick(); h.render();
  return h;
}
module.exports = { harness, deferred, tick, me, conversation, draft, HttpError, mounted };
