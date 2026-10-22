// Real cloud transport: session cookie, mutation guard, stale-response isolation and SSE framing.
const assert = require("node:assert/strict"),
  fs = require("node:fs"),
  vm = require("node:vm"),
  ts = require("typescript");
function harness(desktop = false) {
  const calls = [],
    events = [],
    channels = [],
    sources = [],
    timers = [],
    values = new Map();
  let handler = async () => ({ status: 200, text: async () => '{"ok":true}' });
  class Channel {
    constructor() {
      channels.push(this);
    }
  }
  class EventSource {
    constructor(url, options) {
      this.url = url;
      this.options = options;
      sources.push(this);
    }
    addEventListener(name, fn) {
      this[name] = fn;
    }
    close() {
      this.closed = true;
    }
  }
  const exports = {};
  vm.runInNewContext(
    ts.transpileModule(
      fs.readFileSync(require.resolve("../src/cloud/api.ts"), "utf8"),
      {
        compilerOptions: {
          module: ts.ModuleKind.CommonJS,
          target: ts.ScriptTarget.ES2022,
        },
      },
    ).outputText,
    {
      exports,
      location: { origin: "https://nest.test" },
      localStorage: {
        getItem: (key) => values.get(key) || null,
        setItem(key, value) {
          values.set(key, value);
        },
      },
      window: { dispatchEvent: (e) => events.push(e.type) },
      Event,
      EventSource,
      URL,
      Blob,
      Uint8Array,
      atob,
      setTimeout: (fn) => {
        timers.push(fn);
        return timers.length;
      },
      clearTimeout: (id) => {
        timers[id - 1] = null;
      },
      fetch: async (...args) => {
        calls.push(args);
        return handler(...args);
      },
      require: (name) =>
        name === "@/lib/desktop"
          ? { IS_DESKTOP: desktop }
          : {
              Channel,
              invoke: async (name, args) => {
                calls.push([name, args]);
                if (name === "cloud_subscribe") return "sub";
                return handler(name, args);
              },
            },
    },
  );
  return {
    api: exports,
    calls,
    events,
    channels,
    sources,
    timers,
    values,
    setHandler(fn) {
      handler = fn;
    },
  };
}
(async () => {
  const h = harness();
  assert.deepEqual(JSON.parse(JSON.stringify(await h.api.request("/me"))), {
    ok: true,
  });
  assert.equal(h.calls[0][0], "/api/me");
  assert.equal(h.calls[0][1].credentials, "include");
  await h.api.request("/chat/direct", "POST", { accountId: "target" });
  assert.equal(h.calls[1][1].headers["X-LLMN-Client"], "1");
  assert.equal(h.calls[1][1].body, '{"accountId":"target"}');
  h.setHandler(async () => ({
    status: 401,
    text: async () => '{"message":"登录已过期"}',
  }));
  await assert.rejects(h.api.request("/me"), /登录已过期/);
  assert.deepEqual(h.events, ["cloud-unauthorized"]);
  let resolve;
  h.setHandler(() => new Promise((r) => (resolve = r)));
  const old = h.api.request("/me");
  h.api.invalidate();
  resolve({ status: 200, text: async () => '{"id":"old-account"}' });
  await assert.rejects(old, /账号已切换/);
  const connection = harness(true);
  connection.setHandler(async (name, args) => {
    if (name === "cloud_set_server") {
      if (new URL(args.url).pathname !== "/")
        throw new Error("服务器须为 HTTPS origin");
      return;
    }
    return { status: 401, body: '{"message":"请登录"}' };
  });
  assert.equal(
    await connection.api.connectServer("https://EXAMPLE.test:443/"),
    null,
  );
  assert.equal(connection.api.server, "https://example.test");
  assert.equal(
    connection.values.get("llmn-cloud-server"),
    "https://example.test",
  );
  assert.equal(connection.calls.at(-1)[0], "cloud_request");
  assert.equal(connection.calls.at(-1)[1].path, "/api/me");
  assert.equal(connection.calls.at(-1)[1].method, "GET");
  await assert.rejects(
    connection.api.configure("https://example.test/not-an-origin"),
    /HTTPS origin/,
  );
  assert.equal(
    connection.calls.at(-1)[1].url,
    "https://example.test/not-an-origin",
    "native receives the original path for validation",
  );
  assert.equal(connection.api.server, "https://example.test");
  connection.setHandler(async (name) => {
    if (name === "cloud_request") throw new Error("无法连接服务器");
  });
  await assert.rejects(
    connection.api.connectServer("https://other.test/"),
    /无法连接服务器/,
  );
  connection.setHandler(async (name) =>
    name === "cloud_request"
      ? { status: 200, body: '{"id":"current-account"}' }
      : undefined,
  );
  assert.equal(
    (await connection.api.connectServer("https://other.test")).id,
    "current-account",
  );
  const n = harness(true);
  let changes = 0;
  const stop = n.api.subscribe("9", () => changes++, assert.fail);
  await new Promise((r) => setImmediate(r));
  const initial = changes;
  n.channels[0].onmessage("event: change\nda");
  assert.equal(changes, initial);
  n.channels[0].onmessage('ta: {"cursor":"10"}\n\n');
  assert.equal(changes, initial + 1);
  stop();
  n.channels[0].onmessage("data: {}\n\n");
  assert.equal(changes, initial + 1);
  assert.equal(n.calls.at(-1)[0], "cloud_unsubscribe");
  const web = harness();
  let refreshCount = 0;
  const end = web.api.subscribe(
    "0",
    async () => {
      refreshCount++;
      return "42";
    },
    assert.fail,
  );
  web.sources[0].onopen();
  await new Promise((r) => setImmediate(r));
  web.sources[0].onerror();
  assert.equal(web.sources[0].closed, true);
  await new Promise((r) => setImmediate(r));
  web.timers.filter(Boolean).at(-1)();
  assert.equal(web.sources[1].url, "/api/chat/events?after=42");
  assert.ok(refreshCount >= 2);
  end();
  const retry = harness();
  let attempts = 0,
    failures = 0;
  const done = retry.api.subscribe(
    "0",
    async () => {
      if (!attempts++) throw new Error("offline");
      return "11";
    },
    () => failures++,
  );
  retry.sources[0].onopen();
  await new Promise((r) => setImmediate(r));
  assert.equal(failures, 1);
  retry.timers.filter(Boolean).at(-1)();
  await new Promise((r) => setImmediate(r));
  assert.equal(attempts, 2);
  done();
  console.log("Cloud transport checks passed");
})().catch((e) => {
  console.error(e);
  process.exitCode = 1;
});
