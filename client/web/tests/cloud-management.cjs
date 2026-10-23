const assert = require("node:assert/strict"), fs = require("node:fs"), vm = require("node:vm"), path = require("node:path"), ts = require("typescript"), React = require("react"), { randomUUID } = require("node:crypto");
class HttpError extends Error { constructor(status, message, code) { super(message); this.status = status; this.code = code; } }
const profile = { revision: "1", persona: "first", providerId: "fixture", modelId: "small", reasoningEffort: null, temperature: null, maxTokens: null, autoDailyTokens: null };
const view = { profile, models: [] };
const tick = () => new Promise(resolve => setImmediate(resolve));
const deferred = () => { let resolve; const promise = new Promise(r => { resolve = r; }); return { promise, resolve }; };
function harness(file = "AiConfiguration") {
  const slots = [], effects = [], calls = []; let index = 0;
  const hooks = { ...React,
    useState(initial) { const i = index++; if (!(i in slots)) slots[i] = typeof initial === "function" ? initial() : initial; return [slots[i], value => { slots[i] = typeof value === "function" ? value(slots[i]) : value; }]; },
    useRef(initial) { const i = index++; return slots[i] ||= { current: initial }; },
    useEffect(fn, deps) { const i = index++, old = slots[i]; if (!old || deps.some((d, n) => d !== old.deps[n])) { old?.cleanup?.(); const record = { deps }; slots[i] = record; effects.push(() => { record.cleanup = fn(); }); } },
  };
  const api = { server: "https://fixture.invalid", HttpError, async request(path, method, body) { calls.push(structuredClone({ path, method, body })); return api.respond(path, method, body); }, respond: async () => view };
  const exports = {};
  vm.runInNewContext(ts.transpileModule(fs.readFileSync(path.join(__dirname, `../src/cloud/${file}.tsx`), "utf8"), { compilerOptions: { jsx: ts.JsxEmit.ReactJSX, target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.CommonJS } }).outputText + (file === "LocalAgentPanel" ? "\nexports.LocalSetup = LocalSetup;" : ""), {
    exports, crypto: { randomUUID }, FormData: class { constructor(form) { this.fields = form; } get(name) { return this.fields[name]; } }, require(name) { if (name === "react") return hooks; if (name === "./api") return api; if (name === "@tauri-apps/api/core") return { invoke() { throw Error("unexpected native call"); } }; return require(name); },
  });
  return { exports, api, calls, remount() { slots.forEach(slot => slot?.cleanup?.()); slots.length = 0; }, render(fn, ...props) { index = 0; return fn(...props); }, async effects() { while (effects.length) effects.shift()(); await tick(); } };
}
const operation = { ...profile, type: "saveProfile", expectedRevision: "1", persona: "new" }; delete operation.revision;
function elements(node, test, result = []) { if (node && typeof node === "object") { if (test(node)) result.push(node); React.Children.forEach(node.props?.children, child => elements(child, test, result)); } return result; }
(async () => {
  const h = harness(), render = () => h.render(h.exports.useAiConfiguration, "ai", "human");
  render(); await h.effects();
  h.api.respond = async () => { throw new HttpError(409, "保存未确认", "node_timeout"); };
  await render().save(operation); const first = h.calls.at(-1).body;
  assert.equal(render().uncertain, true);
  h.remount(); assert.equal(render().uncertain, true, "navigation retains original uncertain command"); await h.effects();
  h.api.respond = async () => { throw new HttpError(409, "版本冲突", "revision_conflict"); };
  await render().save({ ...operation, persona: "must not replace" });
  assert.deepEqual(h.calls.at(-1).body, first);
  assert.equal(render().uncertain, true, "later rejection cannot disprove previous save");
  h.api.respond = async () => ({ ...view, profile: { ...profile, revision: "3", persona: "current" } });
  await render().save(); assert.equal(render().uncertain, false); assert.equal(render().saved, true);
  assert.equal(render().view.profile.persona, "current", "replay displays current authoritative node view");
  assert.equal(h.calls.at(-1).path, "/ai/ai/management");
  assert.equal("operatorId" in first, false, "operator never comes from renderer request");
  h.api.respond = async () => { throw new HttpError(409, "离线", "node_offline"); };
  await render().save(operation); assert.equal(render().uncertain, false, "known undispatched save unlocks");
  h.api.respond = async () => { throw new HttpError(409, "不支持管理", "unsupported_capability"); };
  await render().save(operation); assert.equal(render().uncertain, false, "unsupported capability is an undispatched save");
  h.api.respond = async () => { throw new HttpError(400, "invalid node response", "bad_request"); };
  await render().save(operation); assert.equal(render().uncertain, true, "malformed response could follow a successful save");
  h.api.respond = async () => { throw new HttpError(403, "已失去管理权限"); };
  await render().load(); assert.equal(render().view, null); assert.equal(render().uncertain, false);
  const late = harness(), pending = deferred();
  late.api.respond = () => pending.promise;
  const r = () => late.render(late.exports.useAiConfiguration, "ai", "human"); r(); await late.effects();
  late.api.respond = async () => ({ ...view, profile: { ...profile, revision: "new" } });
  await r().load(); pending.resolve(view); await tick(); assert.equal(r().view.profile.revision, "new");
  h.exports.clearAiConfigurationCommands(); h.remount(); assert.equal(render().uncertain, false);

  const race = harness(), oldSave = deferred(), newSave = deferred();
  const rr = () => race.render(race.exports.useAiConfiguration, "ai", "human");
  rr(); await race.effects();
  race.api.respond = () => oldSave.promise;
  const savingOld = rr().save(operation);
  race.remount(); race.api.respond = async () => view;
  rr(); await race.effects(); rr().finishReview();
  race.api.respond = () => newSave.promise;
  const savingNew = rr().save({ ...operation, persona: "second pending save" });
  const newerCommand = race.calls.at(-1).body;
  oldSave.resolve(view); await savingOld;
  race.remount(); rr();
  assert.equal(rr().uncertain, true, "old response cannot remove a new unconfirmed save after navigation");
  race.api.respond = async () => view; await race.effects();
  await rr().save(); assert.deepEqual(race.calls.at(-1).body, newerCommand);
  newSave.resolve(view); await savingNew;

  const local = harness("LocalAgentPanel"), saved = [];
  const props = { configuration: { nodeName: "fixture", provider: "fixture", model: "small", protocol: "openai", baseUrl: "https://model.invalid", persona: "preserve", hasSavedKey: true }, catalog: { providers: [{ id: "fixture", name: "Fixture", models: [{ id: "small", name: "small" }] }] }, busy: false, save: async value => { saved.push(value); return true; } };
  let tree = local.render(local.exports.LocalSetup, props);
  await tree.props.onSubmit({ preventDefault() {}, currentTarget: { nodeLabel: "fixture", baseUrl: props.configuration.baseUrl, protocol: "openai" } });
  assert.equal(saved[0].nodeName, "fixture"); assert.equal(saved[0].baseUrl, props.configuration.baseUrl); assert.equal(saved[0].persona, "preserve"); assert.equal(saved[0].apiKey, undefined);
  elements(tree, n => n.type === "select" && n.props.value === "keep")[0].props.onChange({ target: { value: "key" } });
  tree = local.render(local.exports.LocalSetup, props);
  elements(tree, n => n.type === "input" && n.props.type === "password")[0].props.onChange({ target: { value: "local-fixture-secret" } });
  tree = local.render(local.exports.LocalSetup, props);
  await tree.props.onSubmit({ preventDefault() {}, currentTarget: { nodeLabel: "fixture" } });
  assert.equal(saved[1].apiKey, "local-fixture-secret"); assert.equal(saved[1].baseUrl, "", "empty input explicitly clears an address override"); assert.equal(saved[1].protocol, ""); assert.equal(local.calls.length, 0, "key setup never calls cloud API");
  tree = local.render(local.exports.LocalSetup, props);
  assert.equal(elements(tree, n => n.type === "input" && n.props.type === "password")[0].props.value, "", "clear typed key after local save");
  console.log("PASS: AI online settings retain uncertain commands, fence late/revoked reads and show current acknowledgements; local key setup preserves config and never calls cloud");
})().catch(error => { console.error(error); process.exitCode = 1; });
