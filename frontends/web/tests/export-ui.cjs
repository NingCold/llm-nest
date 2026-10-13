// Exercise the real HTTP/download adapter and the real export controls.
const fs = require('node:fs'), path = require('node:path'), vm = require('node:vm'), assert = require('node:assert/strict'), ts = require('typescript');
const compile = file => ts.transpileModule(fs.readFileSync(path.resolve(__dirname, '../src/social', file), 'utf8'), { compilerOptions: { jsx: ts.JsxEmit.ReactJSX, target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.CommonJS } }).outputText;
const flush = () => new Promise(resolve => setImmediate(resolve));
const all = n => !n || typeof n !== 'object' ? [] : [n, ...[n.props?.children].flat(Infinity).flatMap(all)];

(async () => {
  const blobs = [], links = [], timers = [], revoked = [], requests = [], download = {};
  let fail = false, clickFails = false;
  vm.runInNewContext(compile('export.ts'), { exports: download, Blob,
    URL: { createObjectURL: blob => { blobs.push(blob); return 'blob:fixture' }, revokeObjectURL: url => revoked.push(url) },
    document: { createElement: () => { const link = { click() { if (clickFails) throw new Error('download denied'); this.clicked = true }, remove() { this.removed = true } }; links.push(link); return link }, body: { appendChild: link => { link.attached = true } } },
    setTimeout: fn => timers.push(fn),
  });
  const file = { fileName: '中文🙂.json', mediaType: 'application/json;charset=utf-8', messageCount: 225, content: '{"text":"你好🙂\\n第二行"}\n' };
  const http = {}, transport = {};
  vm.runInNewContext(compile('transport.ts'), { exports: transport, Error });
  vm.runInNewContext(compile('http.ts'), { exports: http, Headers, URLSearchParams, require: name => name === './export' ? download : transport,
    fetch: async (url, options) => { requests.push({ url, options }); return new Response(JSON.stringify(fail ? { code: 'forbidden', message: '已退出该群' } : file), { status: fail ? 403 : 200, headers: { 'content-type': 'application/json' } }) },
  });
  const result = await http.socialClient.exportConversation('conversation-id', 'json');
  assert.equal(result.outcome, 'downloaded'); assert.equal(result.messageCount, 225);
  assert.equal(requests[0].url, '/api/social/conversations/conversation-id/export?format=json');
  assert.equal(requests[0].options.headers.get('X-LLMN-Client'), '1');
  assert.equal(await blobs[0].text(), file.content); assert.equal(blobs[0].type, file.mediaType);
  assert.equal(links[0].download, file.fileName); assert.ok(links[0].attached && links[0].clicked && links[0].removed);
  assert.equal(revoked.length, 0); timers.shift()(); assert.equal(revoked[0], 'blob:fixture');
  fail = true; await assert.rejects(http.socialClient.exportConversation('departed', 'markdown'), /已退出该群/); assert.equal(blobs.length, 1);
  clickFails = true; assert.throws(() => download.downloadExport(file), /download denied/); assert.ok(links[1].removed); timers.shift()(); assert.equal(revoked.length, 2);

  const slots = [], calls = []; let index = 0, complete;
  const react = {
    useState(initial) { const i = index++; if (!(i in slots)) slots[i] = initial; return [slots[i], value => { slots[i] = value }] },
    useRef(initial) { const i = index++; return slots[i] ??= { current: initial } },
  };
  const exported = {}, jsx = (type, props) => ({ type, props });
  const client = { exportConversation: (id, format) => { calls.push({ id, format }); return new Promise((resolve, reject) => { complete = { resolve, reject } }) } };
  vm.runInNewContext(compile('ConversationExportPanel.tsx'), { exports: exported, Error, require: name => name === 'react' ? react : name === 'react/jsx-runtime' ? { jsx, jsxs: jsx } : name === './client' ? { socialClient: client } : {} });
  const render = () => { index = 0; return all(exported.ConversationExportPanel({ conversationId: 'current-window' })) };
  const button = () => render().find(n => n.type === 'button');
  const status = () => render().find(n => n.props.role === 'status')?.props.children;
  render().find(n => n.type === 'select').props.onChange({ target: { value: 'json' } });
  button().props.onClick(); button().props.onClick(); assert.equal(calls.length, 1);
  assert.ok(render().find(n => n.type === 'fieldset').props.disabled);
  complete.reject(new Error('磁盘空间不足')); await flush();
  assert.ok(render().some(n => n.props.role === 'alert')); assert.equal(status(), undefined);
  assert.equal(render().find(n => n.type === 'select').props.value, 'json');
  button().props.onClick(); assert.deepEqual(calls[1], calls[0]);
  complete.resolve({ outcome: 'cancelled' }); await flush(); assert.equal(status(), '已取消导出');
  button().props.onClick(); complete.resolve({ outcome: 'saved', messageCount: 225, fileName: 'chat.json' }); await flush();
  assert.match(status(), /^已保存 225 条消息/);
  render().find(n => n.type === 'select').props.onChange({ target: { value: 'markdown' } }); assert.equal(status(), undefined);
  button().props.onClick(); complete.resolve({ outcome: 'downloaded', messageCount: 0, fileName: 'empty.md' }); await flush();
  assert.match(status(), /^已开始下载 0 条消息/);
  console.log('PASS: full-file browser download, Unicode, cleanup, denied access, IPC save/cancel UI, duplicate clicks and retry');
})().catch(e => { console.error(e); process.exitCode = 1 });
