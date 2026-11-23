const assert = require('node:assert/strict');
const { harness, deferred, tick, me, conversation, draft, HttpError, mounted } = require('./cloud-harness.cjs');
// Discard only mount reads, not sends: assertions below observe explicit user commands.
async function ready(...args) { const h = await harness(...args); h.calls.length = 0; return h; }
async function refreshCases() {
  const requests = []; let concurrent = 0, maximum = 0;
  const snapshot = cursor => ({ self: me, contacts: [], conversations: [conversation], cursor });
  const h = await harness(undefined, false, new Map(), me, undefined, ({ api }) => {
    api.respond = path => {
      if (path !== '/chat/init') return { messages: [], hasMore: false, latestPosition: '0' };
      concurrent++; maximum = Math.max(maximum, concurrent);
      const pending = deferred(); requests.push(pending);
      return pending.promise.finally(() => concurrent--);
    };
  });
  assert.equal(requests.length, 1, 'mount requests a snapshot without waiting for SSE');
  const notification = h.api.event();
  const overlapping = [h.render().refresh(), h.render().refresh()];
  assert.equal(requests.length, 1);
  requests[0].resolve(snapshot('first')); await tick();
  assert.equal(h.render().snapshot.cursor, 'first', 'publish first result while follow-up waits');
  assert.equal(requests.length, 2);
  requests[1].resolve(snapshot('latest')); await Promise.all([notification, ...overlapping]); await tick();
  assert.equal(requests.length, 2); assert.equal(maximum, 1);
  assert.equal(h.render().snapshot.cursor, 'latest');
  const failure = assert.rejects(h.render().refresh(), /offline/);
  requests[2].reject(new Error('offline')); await failure;
  assert.equal(h.render().snapshot.cursor, 'latest');
  const retry = h.render().refresh(); requests[3].resolve(snapshot('retry')); await retry;
  assert.equal(h.render().snapshot.cursor, 'retry');
  const late = h.render().refresh(), queued = h.render().refresh(); h.unmount();
  requests[4].resolve(snapshot('late')); await Promise.all([late, queued]);
  assert.equal(requests.length, 5); assert.equal(h.render().snapshot.cursor, 'retry');
  const replacement = await ready(undefined, false, new Map(), { ...me, id: 'other' });
  assert.equal(replacement.render().me.id, 'other');

  const recovering = await ready(); recovering.api.unlocked = true;
  // Re-rendering with the same account does not reinstall listeners; simulate the
  // supported unlock notification, then exercise the real mounted retry timer.
  let snapshots = 0, histories = 0;
  recovering.api.respond = async path => {
    if (path === '/chat/init') { if (++snapshots === 1) throw new Error('snapshot offline'); return snapshot('recovered'); }
    if (++histories === 1) throw new Error('history offline');
    return { messages: [], hasMore: false, latestPosition: '0' };
  };
  await recovering.cryptoChange(); assert.match(recovering.render().error, /snapshot offline/);
  await recovering.poll(3000); assert.equal(snapshots, 2); assert.equal(histories, 1);
  assert.match(recovering.render().historyError, /history offline/);
  await recovering.poll(3000); assert.equal(histories, 2); assert.equal(recovering.render().historyError, '');
  await recovering.poll(3000); assert.equal(snapshots, 2); assert.equal(histories, 2);
  console.log('PASS: real mounted snapshot coalescing, visible first result, retries and session isolation');
}
async function startupTimingCases() {
  let clock = 0; const entries = [], cleared = [], pending = deferred();
  const timing = { now: () => ++clock, clearMeasures: name => cleared.push(name), measure: (name, bounds) => entries.push({ name, ...bounds }) };
  const h = await harness(undefined, false, new Map(), me, timing, ({ api }) => {
    api.respond = path => path === '/chat/init' ? pending.promise : { messages: [], hasMore: false, latestPosition: '0' };
  });
  assert.equal(cleared.length, 5); assert.equal(entries.length, 0);
  pending.resolve({ self: me, contacts: [], conversations: [conversation], cursor: 'timed' }); await tick(); h.render();
  assert.deepEqual(entries.map(e => e.name), ['initRequest', 'snapshotProjection', 'firstSnapshotReady', 'firstSnapshotCommit'].map(s => `llmn.chat.startup.${s}`));
  const frame = h.api.frame;
  await h.click('refresh'); assert.equal(h.api.frame, frame, 'later snapshot preserves pending first frame');
  frame(); assert.equal(entries.at(-1).name, 'llmn.chat.startup.firstSnapshotFrame');
  await h.click('refresh'); assert.equal(entries.length, 5);
  assert.ok(entries.every(e => Object.keys(e).sort().join() === 'end,name,start' && e.end >= e.start));
  h.unmount(); frame(); assert.equal(entries.length, 5);
  const delayed = deferred(), count = entries.length;
  const late = await harness(undefined, false, new Map(), me, timing, ({ api }) => { api.respond = path => path === '/chat/init' ? delayed.promise : { messages: [], hasMore: false }; });
  late.unmount(); delayed.resolve({ self: me, conversations: [], cursor: 'late' }); await tick();
  assert.equal(entries.length, count);
}
async function historyCases() {
  const message = (id, position, text = id) => ({ id, position: String(position), text, attachments: [] });
  for (const invalidate of ['switch', 'unmount']) {
    const h = await ready();
    h.api.respond = async path => path === '/chat/init' ? { self: me, contacts: [], conversations: [conversation], cursor: '2' } : { messages: [message('old', 1), message('new', 200)], hasMore: false, latestPosition: '200' };
    await h.click('history');
    const delayed = deferred(); let pages = 0, decryptions = 0;
    h.api.projectPage = async page => { decryptions += page.messages.filter(m => m.sealed).length; return page; };
    h.api.respond = async path => {
      if (path === '/chat/init') return { self: me, contacts: [], conversations: [conversation], cursor: '3' };
      if (path.includes('/other/')) return { messages: [], hasMore: false, latestPosition: '0' };
      pages++; return delayed.promise;
    };
    const reading = h.api.event(); await tick(); assert.equal(pages, 1);
    if (invalidate === 'switch') { h.render().selectConversation('other'); h.render(); }
    else h.unmount();
    delayed.resolve({ messages: [{ ...message('new', 200), sealed: {} }], hasMore: true, latestPosition: '200' });
    await reading; await tick(); assert.equal(pages, 1, 'stale page loop must not request the next page');
    assert.equal(decryptions, 0, 'stale response is discarded before decryption');
  }
  const h = await ready();
  h.api.respond = async path => path === '/chat/init' ? { self: me, contacts: [], conversations: [conversation], cursor: '2' } : { messages: [message('old', 1), message('new', 200)], hasMore: false, latestPosition: '200' };
  await h.click('history');
  h.api.respond = async path => path === '/chat/init' ? { self: me, contacts: [], conversations: [conversation], cursor: '3' } : path.includes('before=') ? { messages: [message('old', 1, 'edited historical text')], hasMore: false, latestPosition: '201' } : { messages: [message('new', 200), message('latest', 201)], hasMore: true, latestPosition: '201' };
  await h.api.event(); await tick();
  assert.deepEqual(Array.from(h.render().page.messages, m => m.id), ['old', 'new', 'latest']);
  assert.equal(h.render().page.messages[0].text, 'edited historical text', 'full visible range still refreshes old edits');
  const pending = deferred(); let reads = 0;
  h.api.respond = async path => path === '/chat/init' ? { self: me, contacts: [], conversations: [conversation], cursor: '4' } : (++reads === 1 ? pending.promise : { messages: [], hasMore: false, latestPosition: '202' });
  const initial = h.render().history(); const notification = h.api.event(); await tick();
  assert.equal(reads, 1); pending.resolve({ messages: [], hasMore: false, latestPosition: '201' });
  await Promise.all([initial, notification]); await tick(); assert.equal(reads, 2, 'notification coalesces to one subsequent history read');
}
async function privateSearchCases() {
  const rooms = [{ ...conversation, encryption: { version: 1, activationPosition: '0' } }, { ...conversation, id: 'plain' }];
  const h = await harness(undefined, false, new Map(), me, undefined, ({ api }) => {
    api.respond = async path => path === '/chat/init' ? { self: me, contacts: [], conversations: rooms, cursor: '1' } : {
      messages: [{ id: path.includes('/plain/') ? 'plain-hit' : 'sealed-hit', position: '1', text: 'private NEEDLE', status: 'committed', createdAt: '2027-01-01' }, { id: 'bad', position: '2', text: '', cryptoError: 'missing key' }], hasMore: false, latestPosition: '2',
    };
  });
  h.render().queryChange('needle'); h.render(); h.calls.length = 0;
  await h.render().search(false, 'all');
  assert.deepEqual(Array.from(h.render().results, m => m.id), ['sealed-hit', 'plain-hit']);
  assert.match(h.render().notice, /2 条消息尚不可解密/);
  assert.equal(h.render().searchMore, false);
  assert.equal(h.calls.length, 2); assert.ok(h.calls.every(c => !c.path.includes('needle') && !c.path.includes('/search')));
  h.calls.length = 0;
  await h.render().search(false, 'current');
  assert.deepEqual(Array.from(h.render().results, m => m.id), ['sealed-hit']);
  assert.equal(h.calls.length, 1);
}
async function suspendDraftCases() {
  const frozen = { text: 'unconfirmed send', requestId: 'original-request', mentions: [], replyTo: null };
  const waiting = await ready({ draft, pending: frozen }), write = deferred();
  waiting.repository.save = () => write.promise;
  const originalWrite = waiting.render().save({ draft, pending: frozen }); await tick();
  waiting.render().lock(); await tick();
  assert.equal(waiting.lifecycle.locks, 0, 'chat cannot unmount while a draft transaction is uncommitted');
  assert.equal(waiting.render().busy, true);
  waiting.render().text('late text'); waiting.render().attach([{}], true); waiting.render().attach([{}], false); await waiting.render().send();
  await assert.rejects(waiting.render().save({ draft: { ...draft, text: 'late save' }, pending: null }), /正在锁定或退出/);
  assert.equal(waiting.saves.length, 1, 'suspension freezes new draft writes');
  assert.equal(waiting.lifecycle.imageReads, 0); assert.equal(waiting.lifecycle.fileReads, 0); assert.equal(waiting.calls.length, 0);
  write.resolve(); await originalWrite; await tick();
  assert.equal(waiting.lifecycle.locks, 1);
  assert.deepEqual(waiting.saves[0].next.pending, frozen, 'locking preserves the exact unconfirmed command for manual retry');

  const failed = await ready({ draft, pending: null }), failedWrite = deferred();
  failed.repository.save = () => failedWrite.promise;
  failed.render().text('must remain visible'); await tick(); failed.render().lock(); await tick();
  failedWrite.reject(new Error('draft disk full')); await tick(); await tick();
  assert.equal(failed.lifecycle.locks, 0, 'failed persistence leaves chat unlocked');
  assert.match(failed.render().error, /draft disk full/); assert.equal(failed.render().busy, false);
  failed.repository.save = undefined; failed.render().text('retry after error'); await tick();
  assert.equal(failed.saves.at(-1).next.draft.text, 'retry after error', 'a failed suspension restores editing');

  const reading = await ready({ draft: { ...draft, images: [] }, pending: null }), image = deferred(), imageWrite = deferred();
  reading.repository.readImages = () => image.promise; reading.repository.save = () => imageWrite.promise;
  reading.render().attach([{}], true); await tick();
  reading.render().lock(); await tick();
  assert.equal(reading.lifecycle.locks, 0, 'suspension waits for the original in-progress file read');
  reading.render().attach([{}], true); assert.equal(reading.lifecycle.imageReads, 1);
  image.resolve([{ id: 'selected-image', mime: 'image/png', data: 'isolated-fixture' }]); await tick();
  assert.equal(reading.saves.length, 1, 'a file read started before suspension may still save its result');
  assert.equal(reading.saves[0].next.draft.images[0].id, 'selected-image');
  assert.equal(reading.lifecycle.locks, 0, 'the completed file read must also wait for its draft write');
  imageWrite.resolve(); await tick(); await tick();
  assert.equal(reading.lifecycle.locks, 1);

  const exiting = await ready({ draft, pending: frozen }), exitWrite = deferred();
  exiting.repository.save = () => exitWrite.promise;
  const save = exiting.render().save({ draft, pending: frozen }); await tick();
  const logout = exiting.render().logout(); await tick();
  assert.equal(exiting.lifecycle.logouts, 0, 'logout also waits for durable drafts');
  exiting.render().text('late logout input'); await exiting.render().send();
  assert.equal(exiting.saves.length, 1); assert.equal(exiting.calls.length, 0);
  exitWrite.resolve(); await save; await logout;
  assert.equal(exiting.lifecycle.logouts, 1);
  const failedExit = await ready({ draft, pending: frozen }), rejectedExitWrite = deferred();
  failedExit.repository.save = () => rejectedExitWrite.promise;
  const rejectedSave = assert.rejects(failedExit.render().save({ draft, pending: frozen }), /draft unavailable/); await tick();
  const rejectedLogout = assert.rejects(failedExit.render().logout(), /draft unavailable/); await tick();
  rejectedExitWrite.reject(new Error('draft unavailable')); await rejectedSave; await rejectedLogout;
  assert.equal(failedExit.lifecycle.logouts, 0, 'failed draft persistence prevents logout as well as lock');
  assert.equal(failedExit.render().busy, false);
  console.log('PASS: lock/logout freeze new editing and dispatch, preserve unconfirmed drafts, wait existing attachment work, and keep chat open on persistence failure');
}
(async () => {
  const durable = await ready(), disk = deferred();
  durable.repository.save = () => disk.promise;
  durable.api.respond = async () => { throw new Error('ack lost'); };
  const dispatch = durable.click('send'); await tick();
  assert.equal(durable.saves.length, 1); assert.equal(durable.calls.length, 0, 'dispatch waits for durable command storage');
  disk.resolve(); await dispatch;
  assert.equal(durable.calls.length, 1); assert.ok(durable.render().record.pending);
  const first = await ready({ draft, pending: null });
  first.api.respond = async () => { throw new HttpError(409); };
  await first.click("send");
  assert.equal(first.saves.length, 2);
  assert.equal(first.saves[0].next.pending.replyTo, "earlier");
  assert.equal(first.saves[1].next.pending, null, "first explicit rejection unlocks editing");
  assert.deepEqual(first.saves[1].next.draft, draft, "all editable text, targets and attachments survive");
  assert.match(first.render().error, /HTTP 409/);

  const uncertain = await ready({ draft, pending: null });
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

  const recovered = await ready({ draft: { ...draft, text: "later input", mentions: ["new-ai"], replyTo: "new-reply" }, pending });
  assert.equal(recovered.calls.length, 0, "recovery never sends automatically");
  recovered.api.respond = async () => { throw new HttpError(404); };
  await recovered.render().send();
  assert.equal(recovered.calls[0].body.text, draft.text);
  assert.equal(recovered.calls[0].body.replyTo, draft.replyTo);
  assert.equal(JSON.stringify(recovered.calls[0].body.mentions), JSON.stringify(draft.mentions));
  assert.equal(recovered.render().record.draft.text, "later input");
  const localPending = { text: "offline text", requestId: "local-request", route: "local", mentions: [], replyTo: null };
  const local = await ready({ draft: { ...localPending, text: "later" }, pending: localPending });
  await local.render().send();
  assert.equal(local.calls.length, 1);
  assert.equal(local.calls[0].local, true, "recovered local command never falls through to cloud transport");
  assert.equal(local.calls[0].requestId, "local-request");
  assert.equal(local.render().record.pending.route, "local", "lost local acknowledgement retains route and request identity");
  const unsupportedLocal = await ready({ draft, pending: { ...localPending, images: draft.images } });
  await unsupportedLocal.render().send();
  assert.equal(unsupportedLocal.calls.length, 0, "unsupported local attachment is rejected without silently dropping its bytes or cloud fallback");
  const rememberedRoute = new Map([[`llmn-local-chat-route:${JSON.stringify(["https://example.test/me", "group"])}`, "local"]]);
  const refreshedLocal = await ready({ draft: localPending, pending: null }, true, rememberedRoute);
  await refreshedLocal.render().send();
  assert.equal(refreshedLocal.calls[0].local, true, "a remembered local route survives reload even without an unconfirmed command or in-memory view");
  assert.equal(refreshedLocal.render().record.pending.route, "local");
  const oldResponse = await ready({ draft: localPending, pending: null }, true, rememberedRoute);
  let resolveLocal;
  oldResponse.api.localRespond = () => new Promise(resolve => { resolveLocal = resolve; });
  const beforeLogout = oldResponse.render();
  const synchronization = beforeLogout.syncLocal();
  beforeLogout.endSession();
  resolveLocal({ human: { id: "me" }, conversation: { id: "group" }, messages: [] });
  await synchronization;
  assert.equal(oldResponse.render().localChat, undefined, "an already resolved native response cannot restore local data after logout");
  conversation.encryption = { version: 1, activationPosition: "0" };
  const encrypted = await ready({ draft: { text: "private words", requestId: null, mentions: [], replyTo: null }, pending: null });
  encrypted.api.respond = async () => { throw Error("lost ciphertext acknowledgement"); };
  await encrypted.render().send();
  await encrypted.render().send();
  assert.equal(encrypted.api.encryptions, 1, "unknown encrypted send retries its frozen envelope instead of advancing the SDK again");
  assert.deepEqual(encrypted.calls[0].body, encrypted.calls[1].body);
  assert.equal(encrypted.calls[0].body.text, "");
  assert.ok(encrypted.calls[0].body.messageId);
  assert.equal(JSON.stringify(encrypted.calls[0].body).includes("private words"), false, "plaintext is not part of the cloud request");
  const encryptedSocial = await ready({ draft, pending: null });
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
  const ipc = await ready({ draft: { ...frozenLocal, text: "later" }, pending: frozenLocal }, true);
  for (const failure of ['receiveLocalKeys','projectPage']) {
    const confirmed = await ready({ draft: { ...frozenLocal, text: 'already sent' }, pending: frozenLocal }, true);
    confirmed.api.sendLocalRespond = async () => ({ human: me, ai: { id: 'ai-a' }, conversation: { id: 'group' }, messages: [{ id: 'confirmed', position: '11', attachments: [], sealed: frozenLocal.sealed }] });
    confirmed.api[failure] = async () => { throw new Error(`${failure} failed`); };
    await confirmed.render().send();
    assert.equal(confirmed.render().record.pending, null, 'confirmed native send clears pending even when local decryption fails');
    assert.equal(confirmed.saves.at(-1).next.pending, null, 'confirmed result is persisted');
    assert.equal(confirmed.render().record.draft.text, '');
    assert.match(confirmed.render().historyError, /本机聊天记录解密失败/);
    assert.equal(confirmed.calls.filter(c => c.local).length, 1);
  }
  await ipc.render().send();
  await ipc.render().send();
  assert.deepEqual(ipc.calls[0], ipc.calls[1], "lost IPC ack retries frozen Megolm and Olm key package together");
  assert.deepEqual(ipc.calls[0].keyMessages, frozenLocal.keyMessages);
  assert.equal(ipc.calls[0].text, "");
  assert.deepEqual(ipc.calls[0].options.encryptedAttachments, frozenLocal.encryptedAttachments);
  assert.equal(ipc.calls[0].options.replyTo, "earlier");
  assert.equal(ipc.calls[0].options.expectedConversationRevision, "7");
  const target = { ...conversation, id: "direct", kind: "direct", peer: { id: "ai-a" } };
  const continuation = await ready({ draft, pending: null });
  const targetDraft = { draft: { ...draft, text: "保留私聊输入", reference: null }, pending: null };
  continuation.repository.records.direct = targetDraft;
  continuation.api.respond = async path => path === "/chat/direct" ? target : path.endsWith("reference-preview") ? { conversation: { id: "source-group" }, messages: [] } : path.includes("/messages") ? { messages: [], hasMore: false, latestPosition: "0" } : { self: me, contacts: [], conversations: [conversation, target], cursor: "2" };
  await continuation.render().continuePrivately({ id: "ai-a" }, draft.reference);
  assert.equal(continuation.saves.length, 1);
  assert.equal(continuation.saves[0].id, "direct");
  assert.equal(continuation.saves[0].next.draft.text, "保留私聊输入");
  assert.deepEqual(continuation.saves[0].next.draft.reference, draft.reference);
  assert.equal(continuation.render().active, "direct");
  assert.equal(continuation.calls.some(call => call.method === "POST" && call.path.endsWith("/messages")), false, "continuation never auto-sends");
  const blocked = await ready({ draft, pending: null });
  blocked.repository.records.direct = { ...targetDraft, pending };
  blocked.api.respond = continuation.api.respond;
  await assert.rejects(blocked.render().continuePrivately({ id: "ai-a" }, draft.reference), /尚未确认的发送/);
  assert.equal(blocked.saves.length, 0, "continuation cannot replace an uncertain target command");
  const invalid = await ready({ draft, pending: null });
  invalid.api.respond = async path => { if (path === "/chat/direct") return target; throw new HttpError(403); };
  await assert.rejects(invalid.render().continuePrivately({ id: "ai-a" }, draft.reference), /HTTP 403/);
  assert.equal(invalid.saves.length, 0, "preview rejection cannot put invalid source metadata into a draft");
  const desktop = await ready({ draft, pending: null }, true);
  await tick();
  await desktop.listeners['drafts-prepare-close']({payload:'close'});
  assert.throws(() => desktop.render().text('still closing'), /正在保存并关闭/);
  desktop.listeners['desktop-close-retained']();
  desktop.render().text('托盘重新打开后继续输入'); await new Promise(resolve => setImmediate(resolve));
  assert.equal(desktop.saves.at(-1).next.draft.text, '托盘重新打开后继续输入'); desktop.unmount();
  await suspendDraftCases();
  await refreshCases();
  await startupTimingCases();
  await historyCases();
  await privateSearchCases();
  console.log("PASS: cloud structured sends persist before dispatch, unlock only first explicit rejections and preserve uncertain/recovered command snapshots");
})().catch(error => { console.error(error); process.exitCode = 1; }).finally(() => { for (const h of mounted) h.unmount(); });
