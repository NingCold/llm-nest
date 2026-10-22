// Exercise the real HTTP/download adapter and the real export controls.
const fs = require('node:fs'), path = require('node:path'), vm = require('node:vm'), assert = require('node:assert/strict'), ts = require('typescript');
const compile = file => ts.transpileModule(fs.readFileSync(path.resolve(__dirname, '../src/social', file), 'utf8'), { compilerOptions: { jsx: ts.JsxEmit.ReactJSX, target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.CommonJS } }).outputText;
const flush = () => new Promise(resolve => setImmediate(resolve));
const all = n => !n || typeof n !== 'object' ? [] : [n, ...[n.props?.children].flat(Infinity).flatMap(all)];

(async () => {
  const blobs = [], links = [], timers = [], revoked = [], requests = [], download = {};
  let fail = false, clickFails = false, binary = null;
  vm.runInNewContext(compile('export.ts'), { exports: download, Blob,
    URL: { createObjectURL: blob => { blobs.push(blob); return 'blob:fixture' }, revokeObjectURL: url => revoked.push(url) },
    document: { createElement: () => { const link = { click() { if (clickFails) throw new Error('download denied'); this.clicked = true }, remove() { this.removed = true } }; links.push(link); return link }, body: { appendChild: link => { link.attached = true } } },
    setTimeout: fn => timers.push(fn),
  });
  const file = { fileName: '中文🙂.json', mediaType: 'application/json;charset=utf-8', messageCount: 225, content: '{"text":"你好🙂\\n第二行"}\n' };
  const http = {}, transport = {};
  vm.runInNewContext(compile('transport.ts'), { exports: transport, Error });
  vm.runInNewContext(compile('http.ts'), { exports: http, Headers, URLSearchParams, require: name => name === './export' ? download : transport,
    fetch: async (url, options) => { requests.push({ url, options }); return binary && !fail ? new Response(binary.bytes, { headers: { 'content-type': binary.type, 'content-disposition': `attachment; filename*=UTF-8''${encodeURIComponent(binary.name)}`, 'X-LLMN-Message-Count':'225' } }) : new Response(JSON.stringify(fail ? { code: 'forbidden', message: '已退出该群' } : file), { status: fail ? 403 : 200, headers: { 'content-type': 'application/json' } }) },
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
  fail = clickFails = false;
  binary = { bytes: new Uint8Array([0,255,137,80,78,71]), type:'image/png', name:'原图.png' };
  const original = await http.socialClient.saveImage('message', 2, '9007199254740993');
  assert.equal(original.fileName, '原图.png'); assert.equal(requests.at(-1).url, '/api/social/messages/message/images/2/download?revision=9007199254740993');
  assert.deepEqual(new Uint8Array(await blobs.at(-1).arrayBuffer()), binary.bytes);
  await http.socialClient.saveImage('message', 2, '3', 'ai');
  assert.equal(requests.at(-1).url, '/api/social/ai-accounts/ai/view/messages/message/images/2/download?revision=3');
  binary = { bytes:new Uint8Array([80,75,3,4,0,255]), type:'application/zip', name:'中文聊天.zip' };
  const archive = await http.socialClient.exportConversation('chat', 'markdown', true);
  assert.equal(archive.messageCount,225); assert.equal(archive.fileName,binary.name);
  assert.equal(requests.at(-1).url,'/api/social/conversations/chat/export?format=markdown&includeImages=true');
  assert.deepEqual(new Uint8Array(await blobs.at(-1).arrayBuffer()),binary.bytes);
  for (const [includeImages, includeFiles, query] of [[false,true,'&includeFiles=true'],[true,true,'&includeImages=true&includeFiles=true']]) {
    const result = await http.socialClient.exportConversation('chat','json',includeImages,includeFiles);
    assert.equal(requests.at(-1).url,`/api/social/conversations/chat/export?format=json${query}`);
    assert.equal(result.outcome,'downloaded');assert.equal(result.messageCount,225);assert.equal(result.fileName,binary.name);
    assert.deepEqual(new Uint8Array(await blobs.at(-1).arrayBuffer()),binary.bytes);
    assert.equal(links.at(-1).download,binary.name);assert.ok(links.at(-1).clicked);assert.equal(blobs.at(-1).type,'application/zip');
  }
  const count = blobs.length; fail=true;
  await assert.rejects(http.socialClient.saveImage('message',2,'3'),/已退出该群/);
  await assert.rejects(http.socialClient.exportConversation('chat','json',true),/已退出该群/);
  assert.equal(blobs.length,count,'denied requests never create downloads');

  const slots = [], calls = []; let index = 0, complete;
  const react = {
    useState(initial) { const i = index++; if (!(i in slots)) slots[i] = initial; return [slots[i], value => { slots[i] = value }] },
    useRef(initial) { const i = index++; return slots[i] ??= { current: initial } },
  };
  const exported = {}, jsx = (type, props) => ({ type, props });
  const client = { exportConversation: (id, format, includeImages, includeFiles) => { calls.push({ id, format, includeImages, includeFiles }); return new Promise((resolve, reject) => { complete = { resolve, reject } }) } };
  vm.runInNewContext(compile('ConversationExportPanel.tsx'), { exports: exported, Error, require: name => name === 'react' ? react : name === 'react/jsx-runtime' ? { jsx, jsxs: jsx } : name === './client' ? { socialClient: client } : {} });
  const render = () => { index = 0; return all(exported.ConversationExportPanel({ conversationId: 'current-window' })) };
  const button = () => render().find(n => n.type === 'button');
  const status = () => render().find(n => n.props.role === 'status')?.props.children;
  assert.deepEqual(render().filter(n=>n.type==='input').map(n=>n.props.checked),[false,false]);
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
  render().find(n=>n.type==='input').props.onChange({target:{checked:true}});
  button().props.onClick(); assert.equal(calls.at(-1).includeImages,true);
  complete.resolve({outcome:'downloaded',messageCount:2,fileName:'chat.zip'}); await flush(); assert.match(status(),/chat.zip/);

  const imageOption=()=>render().filter(n=>n.type==='input')[0];
  const fileOption=()=>render().filter(n=>n.type==='input')[1];
  imageOption().props.onChange({target:{checked:false}});fileOption().props.onChange({target:{checked:true}});
  button().props.onClick();assert.equal(calls.at(-1).includeImages,false);assert.equal(calls.at(-1).includeFiles,true);
  complete.reject(new Error('文件导出失败'));await flush();assert.equal(fileOption().props.checked,true);assert.equal(imageOption().props.checked,false);
  const failedCall=calls.at(-1);button().props.onClick();assert.deepEqual(calls.at(-1),failedCall);
  complete.resolve({outcome:'cancelled'});await flush();assert.equal(status(),'已取消导出');assert.equal(fileOption().props.checked,true);
  imageOption().props.onChange({target:{checked:true}});button().props.onClick();assert.equal(calls.at(-1).includeImages,true);assert.equal(calls.at(-1).includeFiles,true);
  complete.resolve({outcome:'downloaded',messageCount:2,fileName:'all.zip'});await flush();assert.match(status(),/all.zip/);

  slots.length=0; index=0; calls.length=0;
  client.saveImage=(...args)=>{ calls.push(args); return new Promise((resolve,reject)=>{ complete={resolve,reject} }) };
  const imageUI={};
  vm.runInNewContext(compile('ImageSaveButton.tsx'),{exports:imageUI,Error,require:name=>name==='react'?react:name==='react/jsx-runtime'?{jsx,jsxs:jsx}:name==='./client'?{socialClient:client}:{}});
  const imageView=()=>{index=0;return all(imageUI.ImageSaveButton({id:'m',index:2,revision:'4',account:'ai'}))};
  const imageButton=()=>imageView().find(n=>n.type==='button');
  imageButton().props.onClick();imageButton().props.onClick();assert.equal(calls.length,1);assert.equal(imageButton().props.disabled,true);
  assert.deepEqual(calls[0],['m',2,'4','ai']);
  complete.reject(new Error('消息已修改或撤回'));await flush();assert.ok(imageView().some(n=>n.props.role==='alert'));assert.ok(!imageView().some(n=>n.props.role==='status'));
  imageButton().props.onClick();complete.resolve({outcome:'cancelled'});await flush();assert.ok(imageView().some(n=>n.props.children==='已取消保存'));
  imageButton().props.onClick();complete.resolve({outcome:'saved',fileName:'m.png'});await flush();assert.ok(imageView().some(n=>n.props.children==='已保存：m.png'));
  console.log('PASS: full-file browser download, Unicode, cleanup, denied access, IPC save/cancel UI, duplicate clicks and retry');
})().catch(e => { console.error(e); process.exitCode = 1 });
