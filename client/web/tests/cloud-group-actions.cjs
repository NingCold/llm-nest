const assert = require("node:assert/strict"), fs = require("node:fs"), vm = require("node:vm"), path = require("node:path"), ts = require("typescript"), React = require("react");
const { randomUUID } = require("node:crypto");
function harness() {
  const state = [], refs = [], calls = [], tasks = [];
  let stateIndex = 0, refIndex = 0;
  const api = { async request(path, method = "GET", body) { calls.push({ path, method, body }); return api.respond(path, method, body); }, respond: async () => ({}) };
  const hooks = { ...React,
    useState(initial) { const i = stateIndex++; if (!(i in state)) state[i] = typeof initial === "function" ? initial() : initial; return [state[i], value => { state[i] = typeof value === "function" ? value(state[i]) : value; }]; },
    useRef(initial) { const i = refIndex++; return refs[i] ||= { current: initial }; },
  };
  const exports = {};
  vm.runInNewContext(ts.transpileModule(fs.readFileSync(path.join(__dirname, "../src/cloud/Groups.tsx"), "utf8"), { compilerOptions: { jsx: ts.JsxEmit.ReactJSX, target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.CommonJS } }).outputText, {
    exports, crypto: { randomUUID }, require(name) { if (name === "react") return hooks; if (name === "./api") return api; if (name.startsWith("@/components")) return { Dialog: "dialog", DialogContent: "section", DialogTitle: "h2" }; return require(name); },
  });
  return { exports, calls, api, tasks,
    render(Component, props) { stateIndex = refIndex = 0; return Component(props); },
    act(fn) { const task = fn(); tasks.push(task); return task.catch(() => {}); },
  };
}
function all(node, predicate, result = []) { if (node && typeof node === "object") { if (predicate(node)) result.push(node); React.Children.forEach(node.props?.children, child => all(child, predicate, result)); } return result; }
const find = (tree, predicate) => { const result = all(tree, predicate); assert.equal(result.length, 1); return result[0]; };
const text = node => typeof node === "string" ? node : React.Children.toArray(node?.props?.children).map(text).join("");
const button = (tree, label) => find(tree, node => node.type === "button" && text(node) === label);
const peer = { id: "peer", name: "朋友", kind: "human" }, ai = { id: "ai", name: "AI", kind: "ai" };
(async () => {
  const h = harness(), done = [];
  const props = { contacts: [peer, ai], me: "self", act: h.act, busy: false, error: "", refresh: async () => done.push("refresh"), select: id => done.push(id), close: () => done.push("close") };
  let tree = h.render(h.exports.CreateGroup, props);
  find(tree, node => node.type === "input" && node.props.required).props.onChange({ target: { value: "一起讨论" } });
  const boxes = all(tree, node => node.type === "input" && node.props.type === "checkbox");
  boxes[1].props.onChange({ target: { checked: true } });
  tree = h.render(h.exports.CreateGroup, props);
  const submit = () => find(tree, node => node.type === "form").props.onSubmit({ preventDefault() {} });
  h.api.respond = async () => { throw new Error("lost response"); };
  submit(); await assert.rejects(h.tasks.at(-1), /lost response/);
  h.api.respond = async () => ({ id: "created" });
  tree = h.render(h.exports.CreateGroup, props); submit(); await h.tasks.at(-1);
  assert.equal(h.calls[0].body.requestId, h.calls[1].body.requestId, "lost group-create response reuses command identity");
  assert.equal(JSON.stringify(h.calls[1].body.memberIds), '["ai"]');
  assert.equal(h.calls[1].body.name, "一起讨论"); assert.deepEqual(done, ["created", "close", "refresh"]);

  const b = harness();
  const blockProps = { account: peer, act: b.act, busy: false, refresh: async () => {} };
  b.api.respond = async () => ({ blocked: false, revision: "7" });
  tree = b.render(b.exports.ContactBlock, blockProps); button(tree, "查看屏蔽状态").props.onClick(); await b.tasks.at(-1);
  tree = b.render(b.exports.ContactBlock, blockProps);
  b.api.respond = async () => { throw new Error("offline"); };
  button(tree, "屏蔽私聊").props.onClick(); await assert.rejects(b.tasks.at(-1), /offline/);
  b.api.respond = async () => ({ blocked: true, revision: "8" });
  tree = b.render(b.exports.ContactBlock, blockProps); button(tree, "屏蔽私聊").props.onClick(); await b.tasks.at(-1);
  assert.equal(b.calls[1].body.requestId, b.calls[2].body.requestId);
  assert.equal(b.calls[2].body.expectedRevision, "7");
  tree = b.render(b.exports.ContactBlock, blockProps); button(tree, "解除屏蔽").props.onClick(); await b.tasks.at(-1);
  assert.equal(b.calls[3].body.expectedRevision, "8"); assert.equal(b.calls[3].body.blocked, false);
  console.log("PASS: cloud group creation and block/unblock use versioned commands with stable lost-response retry identities");
})().catch(error => { console.error(error); process.exitCode = 1; });
