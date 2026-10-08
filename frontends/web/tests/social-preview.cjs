// Browser preview contracts: isolated storage, identities, lifecycle and source invalidation.
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const assert = require('node:assert/strict');
const { webcrypto } = require('node:crypto');
const ts = require('typescript');
const root = path.resolve(__dirname, '../src/social');

function harness(values = new Map()) {
  let failWrite = false, sequence = 0;
  const timers = new Map(), modules = new Map();
  const storage = {
    getItem: key => values.get(key) ?? null,
    setItem: (key, value) => { if (failWrite) throw new Error('disk full'); values.set(key, value); },
  };
  function load(name) {
    if (modules.has(name)) return modules.get(name);
    const exports = {}; modules.set(name, exports);
    const source = ts.transpileModule(fs.readFileSync(path.join(root, name + '.ts'), 'utf8'), {
      compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.CommonJS },
    }).outputText;
    vm.runInNewContext(source, {
      exports, require: specifier => {
        if (specifier === 'zustand') return { create: initialize => {
          let state;
          const hook = selector => selector(state);
          hook.getState = () => state;
          state = initialize(update => { state = { ...state, ...(typeof update === 'function' ? update(state) : update) }; }, hook.getState);
          return hook;
        } };
        if (!specifier.startsWith('./')) throw new Error(`Unexpected preview dependency: ${specifier}`);
        return load(specifier.slice(2));
      }, crypto: webcrypto, TextEncoder, structuredClone, Date, BigInt, URLSearchParams, localStorage: storage,
      setTimeout: callback => { const id = ++sequence; timers.set(id, callback); return id; },
      clearTimeout: id => timers.delete(id),
    }, { filename: name });
    return exports;
  }
  const { createPreviewApi, PREVIEW_STORAGE_KEY } = load('mock-api');
  const api = createPreviewApi(storage);
  let snapshot;
  api.subscribe(data => { snapshot = data; });
  const send = (conversationId, text, extras = {}) => api.sendMessage({
    clientRequestId: webcrypto.randomUUID(), conversationId, text, mentions: [], replyTo: null, contextReferences: [], ...extras,
  });
  return {
    api, storage, values, key: PREVIEW_STORAGE_KEY, send, load,
    async init() { snapshot = await api.init(); return snapshot; },
    snapshot: () => structuredClone(snapshot),
    fail: value => { failWrite = value; },
    flush() {
      for (let steps = 0; timers.size; steps++) {
        assert.ok(steps < 2000, 'preview timers should finish');
        const [id, callback] = timers.entries().next().value; timers.delete(id); callback();
      }
    },
    tick() { const entry = timers.entries().next().value; if (entry) { timers.delete(entry[0]); entry[1](); } },
    reopen: () => createPreviewApi(storage),
  };
}

(async () => {
  const h = harness();
  const seed = await h.init();
  const actor = seed.contacts[0], direct = seed.conversations.find(c => c.kind === 'direct' && c.memberIds.includes(actor.id));
  const group = seed.conversations.find(c => c.kind === 'group' && c.memberIds.includes(actor.id));
  assert.equal((await h.api.getDirectConversation(actor.id)).id, direct.id);
  await h.api.updateConversation(direct.id, { status: 'archived' });
  assert.equal((await h.api.getDirectConversation(actor.id)).id, direct.id, 'archiving must not fork a private chat');
  await assert.rejects(h.send(direct.id, '归档时不能发送'), /恢复/);
  const archivedMessage = seed.messages.find(m => m.conversationId === direct.id && m.senderId === seed.self.id);
  await assert.rejects(h.api.editMessage(archivedMessage.id, '归档时不能编辑', archivedMessage.revision), /恢复/);
  await h.api.updateConversation(direct.id, { status: 'active' });

  const request = { clientRequestId: webcrypto.randomUUID(), conversationId: direct.id, text: 'retry fixture', mentions: [], replyTo: null, contextReferences: [] };
  const first = await h.api.sendMessage(request);
  assert.equal((await h.api.sendMessage(request)).id, first.id);
  assert.equal((await h.api.sendMessage({ text: request.text, replyTo: null, contextReferences: [], mentions: [], conversationId: direct.id, clientRequestId: request.clientRequestId })).id, first.id, 'request key order cannot change idempotency');
  await assert.rejects(h.api.sendMessage({ ...request, text: 'different payload' }), /请求已存在/);
  h.flush();
  assert.equal(h.snapshot().messages.filter(m => m.id === first.id).length, 1);
  assert.equal(h.snapshot().runs.filter(r => r.conversationId === direct.id).length, 1);

  const runCount = h.snapshot().runs.length;
  await h.send(group.id, '无人被邀请的群聊消息'); h.flush();
  assert.equal(h.snapshot().runs.length, runCount, 'group idle chatter should not wake every AI');
  const memberIds = group.memberIds.filter(id => id !== seed.self.id);
  await h.send(group.id, '请两位一起聊聊', { mentions: memberIds });
  const running = h.snapshot().runs.find(r => r.status === 'running');
  h.tick();
  await h.api.cancelRun(running.id);
  assert.equal(h.snapshot().runs.find(r => r.id === running.id).status, 'cancelled');
  assert.ok(h.snapshot().runs.some(r => r.status === 'running'), 'stop one reply should allow the next invited member');
  await h.api.endBout(group.id); h.flush();
  assert.ok(!h.snapshot().runs.some(r => r.status === 'running'));
  assert.equal(h.snapshot().conversations.find(c => c.id === group.id).bout, 'ended');
  await h.send(group.id, '新一轮', { mentions: [actor.id] }); h.flush();
  assert.equal(h.snapshot().runs.at(-1).status, 'succeeded');
  await h.api.setPaused(group.id, true);
  const pausedCount = h.snapshot().runs.length;
  await h.send(group.id, '暂停期间仍保存', { mentions: [actor.id] }); h.flush();
  await h.api.setPaused(group.id, false); h.flush();
  assert.equal(h.snapshot().runs.length, pausedCount, 'resume must not replay paused input');

  const source = await h.send(group.id, '接续来源 fixture');
  const refs = [{ conversationId: group.id, messageId: source.id, messageRevision: source.revision, actorId: actor.id }];
  await h.send(direct.id, '我们单独讨论', { contextReferences: refs });
  const continuation = h.snapshot().runs.find(r => r.status === 'running');
  const groupCount = h.snapshot().messages.filter(m => m.conversationId === group.id).length;
  await h.api.refreshSummary(group.id);
  await h.api.saveMemory({ kind: 'conversation', id: group.id }, '来源记忆', source.id);
  await h.api.editMessage(source.id, '编辑后的来源', source.revision); h.flush();
  assert.equal(h.snapshot().runs.find(r => r.id === continuation.id).status, 'cancelled');
  assert.equal(h.snapshot().summaries.find(s => s.conversationId === group.id).status, 'stale');
  assert.equal(h.snapshot().memories.at(-1).status, 'stale');
  assert.equal(h.snapshot().messages.filter(m => m.conversationId === group.id).length, groupCount, 'private continuation cannot write back to the group');
  await assert.rejects(h.send(direct.id, '旧引用', { contextReferences: refs }), /引用来源已变化/);
  const other = seed.contacts.find(c => !group.memberIds.includes(c.id));
  const otherDirect = await h.api.getDirectConversation(other.id);
  const updated = h.snapshot().messages.find(m => m.id === source.id);
  await assert.rejects(h.send(otherDirect.id, '越界引用', { contextReferences: [{ ...refs[0], messageRevision: updated.revision, actorId: other.id }] }), /共同可见/);
  const record = h.snapshot().memories.at(-1);
  await h.api.forgetMemory(record.id, record.revision);
  assert.equal(h.snapshot().memories.at(-1).text, '');
  assert.equal(h.snapshot().messages.find(m => m.id === source.id).text, '编辑后的来源');

  const before = h.snapshot(); h.fail(true);
  await assert.rejects(h.send(group.id, '保存失败'), /未能保存/);
  assert.deepEqual(h.snapshot(), before, 'failed storage must not update in-memory state');
  h.fail(false);
  await h.send(direct.id, '刷新中断测试'); h.tick();
  const partial = h.snapshot().messages.at(-1);
  const reopened = await h.reopen().init();
  assert.equal(reopened.messages.find(m => m.id === partial.id).status, 'interrupted');
  assert.equal(reopened.messages.find(m => m.id === partial.id).text, partial.text);
  await h.api.endBout(direct.id); h.flush();
  const restored = await h.reopen().init();
  assert.equal(restored.contacts.length, seed.contacts.length);
  assert.equal(restored.messages.length, h.snapshot().messages.length);
  h.values.set(h.key, '{invalid');
  await assert.rejects(h.reopen().init(), /原数据已保留/);
  assert.equal(h.storage.getItem(h.key), '{invalid');

  const { parseSocialRoute } = h.load('router');
  assert.equal(parseSocialRoute('#/messages/id/details/memories').memories, true);
  assert.equal(parseSocialRoute('#/contacts/id/memories').memories, true);
  assert.equal(parseSocialRoute('#/settings/appearance').page, 'settings');
  assert.equal(parseSocialRoute('#/chat').page, 'messages');

  const storeHarness = harness();
  const store = storeHarness.load('store').useSocialStore;
  await Promise.all([store.getState().initialize(), store.getState().initialize()]);
  const space = store.getState().snapshot.conversations.find(c => c.kind === 'direct');
  const another = store.getState().snapshot.conversations.find(c => c.kind === 'direct' && c.id !== space.id);
  store.getState().updateDraft(another.id, { text: '另一个窗口的草稿' });
  store.getState().updateDraft(space.id, { text: '发送失败要保留' });
  storeHarness.fail(true);
  assert.equal(await store.getState().send(space.id), false);
  assert.equal(store.getState().drafts[space.id].text, '发送失败要保留');
  assert.ok(store.getState().error);
  storeHarness.fail(false);
  const sentCount = store.getState().snapshot.messages.length;
  const result = await Promise.all([store.getState().send(space.id), store.getState().send(space.id)]);
  assert.deepEqual(result, [true, false]);
  storeHarness.flush();
  assert.equal(store.getState().snapshot.messages.length, sentCount + 2, 'one human message and one AI answer');
  assert.equal(store.getState().drafts[space.id].text, '');
  assert.equal(store.getState().drafts[another.id].text, '另一个窗口的草稿');
  store.getState().updateDraft(space.id, { text: '正在发送的消息' });
  const pending = store.getState().send(space.id);
  store.getState().updateDraft(space.id, { text: '发送期间新输入的草稿' });
  assert.equal(await pending, true); storeHarness.flush();
  assert.equal(store.getState().drafts[space.id].text, '发送期间新输入的草稿');
  const reloadedStore = harness(storeHarness.values).load('store').useSocialStore;
  await reloadedStore.getState().initialize();
  assert.equal(reloadedStore.getState().drafts[space.id].text, '发送期间新输入的草稿');
  assert.equal(reloadedStore.getState().drafts[another.id].text, '另一个窗口的草稿');
  console.log('PASS: social preview private-chat uniqueness, send idempotency, group invitations, stop/pause/end, shared-source access, invalidation, write-through and reload recovery');
  console.log('PASS: social frontend failed-send draft retention, duplicate-send suppression, per-window drafts, late acknowledgement and draft reload');
})().catch(error => { console.error(error); process.exitCode = 1; });
