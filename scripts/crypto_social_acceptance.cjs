/* Third-batch scenarios, run by crypto_acceptance.cjs with its disposable fixture. */
const assert = require('node:assert/strict');
const { randomUUID } = require('node:crypto');

function helpers(c) {
  const { sdk, http, origin, userId, roomId, sign, outgoing, query, sync } = c;
  async function share(sender, peers, cid) {
    const users = [sender, ...peers].map(p => new sdk.UserId(userId(p.account)));
    await query(sender, [sender, ...peers].map(p => p.account));
    const missing = await sender.machine.getMissingSessions(users);
    if (missing) await outgoing(sender, missing);
    const settings = new sdk.EncryptionSettings();
    settings.algorithm = sdk.EncryptionAlgorithm.MegolmV1AesSha2;
    settings.sharingStrategy = sdk.CollectStrategy.onlyTrustedDevices();
    const room = new sdk.RoomId(roomId(cid));
    await sender.machine.invalidateGroupSession(room);
    for (const request of await sender.machine.shareRoomKey(room, [sender, ...peers].map(p => new sdk.UserId(userId(p.account))), settings)) await outgoing(sender, request);
    for (const peer of peers) if (peer.machine) await sync(peer);
  }
  function attachments(items) {
    return items.reduce((out, item, index) => {
      const encoded = sdk.Attachment.encrypt(item.bytes);
      out.uploads.push({ index, data: Buffer.from(encoded.encryptedData).toString('base64') });
      out.metadata.push({ index, kind: item.kind, mime: item.mime, name: item.name, byteLength: item.bytes.length, mediaEncryptionInfo: encoded.mediaEncryptionInfo });
      return out;
    }, { uploads: [], metadata: [] });
  }
  async function command(sender, conversation, text, options = {}) {
    const id = options.id || randomUUID();
    const inner = { version: 1, server: origin, kind: options.kind || 'message', conversationId: conversation.id, conversationRevision: conversation.revision, messageId: id, senderId: sender.account, revision: '1', text, replyTo: options.replyTo || null, mentions: options.mentions || [], runId: null, sequence: null, reference: options.reference || null, attachments: options.metadata || [], ...options.inner };
    const sealed = { version: 1, roomId: roomId(conversation.id), sender: userId(sender.account), deviceId: sender.device, eventId: `$${id}.1.final`, content: JSON.parse(await sender.machine.encryptRoomEvent(new sdk.RoomId(roomId(conversation.id)), 'com.llmn.content.v1', JSON.stringify(inner))) };
    await sign(sender, sealed);
    return { requestId: randomUUID(), deviceId: sender.device, messageId: id, text: '', sealed, expectedConversationRevision: conversation.revision, replyTo: inner.replyTo, mentions: inner.mentions, reference: inner.reference, encryptedAttachments: options.uploads || [] };
  }
  async function download(session, message, index) {
    const response = await fetch(`${origin}/api/chat/messages/${message.id}/attachments/${index}?revision=${message.revision}`, { headers: { Cookie: session.cookie, 'X-LLMN-Client': '1' } });
    assert.equal(response.status, 200);
    return new Uint8Array(await response.arrayBuffer());
  }
  return { share, attachments, command, download };
}

exports.human = async c => {
  const { a, b, http, sdk, signup, device, bootstrap, trust, sync, decrypt, sql, roomId, userId } = c;
  const { share, attachments, command, download } = helpers(c);
  const newcomer = await device(await signup('群历史验收'));
  await bootstrap(newcomer);
  // Give trust establishment an ordinary account relationship before joining the group.
  await http(a, '/chat/direct', 'POST', { accountId: newcomer.account });
  await trust(a, newcomer); await trust(newcomer, a);
  let group = await http(a, '/chat/groups', 'POST', { requestId: randomUUID(), name: '密文群验收', memberIds: [b.account] });
  await share(a, [b], group.id);
  await http(b, `/chat/conversations/${group.id}/encryption`, 'POST', { deviceId: b.device, requestId: randomUUID(), expectedConversationRevision: group.revision }, 403);
  await http(a, `/chat/conversations/${group.id}/encryption`, 'POST', { deviceId: a.device, requestId: randomUUID(), expectedConversationRevision: group.revision });
  group = (await http(a, '/chat/init')).conversations.find(g => g.id === group.id);
  const bytes = Buffer.from('只有成员可读的文件正文\n第二行😀');
  const files = attachments([{ kind: 'file', mime: 'text/plain', name: '私密文件.txt', bytes }]);
  const body = await command(a, group, '入群前的完整历史', files);
  const first = await http(a, `/chat/conversations/${group.id}/messages`, 'POST', body);
  await http(a, `/chat/conversations/${group.id}/messages`, 'POST', body);
  const decoded = await decrypt(b, first.sealed);
  assert.equal(decoded.text, '入群前的完整历史');
  assert.equal(first.attachments[0].kind, 'encrypted');
  assert(!JSON.stringify(first.attachments).includes('私密文件'));
  const cipher = await download(b, first, 0);
  assert.deepEqual(Buffer.from(sdk.Attachment.decrypt(new sdk.EncryptedAttachment(cipher, decoded.attachments[0].mediaEncryptionInfo))), bytes);
  const altered = Uint8Array.from(cipher); altered[0] ^= 1;
  assert.throws(() => sdk.Attachment.decrypt(new sdk.EncryptedAttachment(altered, decoded.attachments[0].mediaEncryptionInfo)));
  assert(!sql(`SELECT body||sealed::text||attachments::text FROM cloud_messages WHERE id='${first.id}'`).includes('私密文件'));
  const oldGroup = group;
  group = await http(a, `/chat/groups/${group.id}/members`, 'POST', { requestId: randomUUID(), expectedRevision: group.revision, accountId: newcomer.account });
  await assert.rejects(() => decrypt(newcomer, first.sealed), 'joining alone cannot invent historical keys');
  const stale = await command(a, oldGroup, '按旧成员表准备');
  await http(a, `/chat/conversations/${group.id}/messages`, 'POST', stale, 409);
  await share(a, [b, newcomer], group.id);
  const keys = JSON.parse(await a.machine.exportRoomKeys(key => key.roomId.toString() === roomId(group.id)));
  assert(keys.length);
  const target = await a.machine.getDevice(new sdk.UserId(userId(newcomer.account)), new sdk.DeviceId(newcomer.device));
  const encrypted = JSON.parse(await target.encryptToDeviceEvent('com.llmn.group-history.v1', { conversationId: group.id, revision: group.revision, keys }, sdk.CollectStrategy.onlyTrustedDevices()));
  const history = { deviceId: a.device, requestId: randomUUID(), kind: 'toDevice', body: { event_type: 'm.room.encrypted', txn_id: randomUUID(), llmn_purpose: 'conversationHistory', llmn_conversation_id: group.id, llmn_expected_revision: group.revision, messages: { [userId(newcomer.account)]: { [newcomer.device]: encrypted } } } };
  await http(a, '/crypto/requests', 'POST', history);
  const received = await sync(newcomer);
  const event = received.filter(e => e.type === sdk.ProcessedToDeviceEventType.Decrypted).map(e => JSON.parse(e.rawEvent)).find(e => e.type === 'com.llmn.group-history.v1');
  assert(event); assert(event.content.keys.every(key => key.room_id === roomId(group.id)));
  await newcomer.machine.importExportedRoomKeys(JSON.stringify(event.content.keys), () => {});
  assert.equal((await decrypt(newcomer, first.sealed)).text, '入群前的完整历史');
  group = await http(a, `/chat/groups/${group.id}/members/${newcomer.account}`, 'DELETE', { requestId: randomUUID(), expectedRevision: group.revision });
  await http(a, '/crypto/requests', 'POST', history, 409);
  await http(a, '/crypto/requests', 'POST', { ...history, requestId: randomUUID(), body: { ...history.body, llmn_expected_revision: group.revision } }, 403);
  await http(newcomer, `/chat/conversations/${group.id}/messages`, 'GET', undefined, 404);
  await share(a, [b], group.id);
  const latest = await http(a, `/chat/conversations/${group.id}/messages`, 'POST', await command(a, group, '退群后新消息'));
  await assert.rejects(() => decrypt(newcomer, latest.sealed));
  // Fetch every page and decrypt locally, including messages older than the first page.
  const texts = []; let before;
  do {
    const page = await http(b, `/chat/conversations/${group.id}/messages?limit=1${before ? `&before=${before}` : ''}`);
    for (const m of page.messages) texts.push((await decrypt(b, m.sealed)).text);
    before = page.hasMore ? page.messages[0].position : null;
  } while (before);
  assert(texts.includes('入群前的完整历史'));
  newcomer.machine.close();
  console.log('PASS: encrypted group history, member-only enable, attachment bytes/hash, opaque metadata, retry, roster conflict, Olm history sharing, removal and rejected replay, future key rotation, complete local history pagination.');
};

exports.agent = async c => {
  const { human, peer, aiId, cid, http, sql, sdk, decrypt, sync, waitFor, modelInputs, rpc, stopCloud, startCloud, localShare, outgoing } = c;
  const { share, attachments, command, download } = helpers(c);
  const png = Buffer.from('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR4nGP4z8DwHwAFAAH/iZk9HQAAAABJRU5ErkJggg==', 'base64');
  let group = await http(human, '/chat/groups', 'POST', { requestId: randomUUID(), name: 'AI 密文群', memberIds: [aiId] });
  await share(human, [peer], group.id);
  await http(human, `/chat/conversations/${group.id}/encryption`, 'POST', { requestId: randomUUID(), deviceId: human.device, expectedConversationRevision: group.revision });
  group = (await http(human, '/chat/init')).conversations.find(g => g.id === group.id);
  async function completed(conversation, body) {
    const route = `/chat/conversations/${conversation.id}/messages`;
    await http(human, route, 'POST', body);
    const page = await waitFor(() => http(human, route), page => {
      const response = page.messages.find(m => m.id === body.messageId)?.aiResponses?.find(r => r.aiId === aiId);
      assert(!response || ['queued', 'running', 'completed'].includes(response.status), `AI response failed: ${JSON.stringify(response)}`);
      return response?.status === 'completed';
    }, 'social encrypted response');
    await sync(human);
    assert.match((await decrypt(human, page.messages.filter(m => m.senderId === aiId).at(-1).sealed)).text, /^节点密文回复-/);
  }
  const file = attachments([{ kind: 'image', mime: 'image/png', name: '群图.png', bytes: png }]);
  const groupBody = await command(human, group, '群图片讨论来源', { ...file, mentions: [aiId] });
  const baseline = modelInputs.length;
  await completed(group, groupBody);
  assert(JSON.stringify(modelInputs.at(-1)).includes('data:image/png;base64,'), 'model receives decrypted image');
  const reference = { conversationId: group.id, sources: [{ messageId: groupBody.messageId, revision: '1' }] };
  let direct = (await http(human, '/chat/init')).conversations.find(g => g.id === cid);
  await share(human, [peer], cid);
  await completed(direct, await command(human, direct, '在私聊继续群话题', { reference }));
  assert(JSON.stringify(modelInputs.at(-1)).includes('群图片讨论来源'));
  const participation = await http(human, `/chat/conversations/${cid}/participation`);
  await http(human, `/chat/conversations/${cid}/contact-settings`, 'PUT', { requestId: randomUUID(), expectedRevision: participation.settings.revision, enabled: true, quietUntil: null, quietAction: 'defer' });
  const dueAt = new Date(Date.now() + 1800).toISOString(), expiresAt = new Date(Date.now() + 90000).toISOString();
  await share(human, [peer], cid);
  const contact = await command(human, direct, '约定密文主题', { kind: 'contactIntent', inner: { aiId, dueAt, expiresAt, utcOffsetMinutes: 480 } });
  const intentBody = { requestId: contact.requestId, intentId: contact.messageId, deviceId: human.device, content: '', aiId, dueAt, expiresAt, utcOffsetMinutes: 480, sealed: contact.sealed, expectedConversationRevision: direct.revision };
  await http(human, `/chat/conversations/${cid}/contact-intents`, 'POST', intentBody);
  await http(human, `/chat/conversations/${cid}/contact-intents`, 'POST', intentBody);
  await waitFor(() => http(human, `/chat/conversations/${cid}/participation`), p => {
    const intent = p.intents.find(i => i.id === contact.messageId);
    assert(!intent || !['failed', 'cancelled', 'uncertain'].includes(intent.status), `contact ${JSON.stringify(intent)}`);
    return intent?.status === 'delivered';
  }, 'sealed contact execution');
  assert(JSON.stringify(modelInputs.at(-1)).includes('约定密文主题'));
  assert(!sql(`SELECT content||sealed::text FROM cloud_contact_intents WHERE id='${contact.messageId}'`).includes('约定密文主题'));
  const bound = (await http(human, '/ai')).find(row => row.account.id === aiId);
  const grant = await http(human, `/chat/conversations/${cid}/direct`, 'POST', { nodeId: bound.node.id, bindingVersion: bound.bindingVersion });
  await rpc({ type: 'prepare', aiId, grant: grant.token });
  await rpc({ type: 'prepareReference', conversationId: cid, selection: reference });
  const localFile = attachments([{ kind: 'file', mime: 'text/plain', name: '离线文件.txt', bytes: Buffer.from('离线文件密文附件') }]);
  await stopCloud();
  const keyRequests = await localShare();
  const local = await command(human, direct, '离线引用与文件', { ...localFile, reference });
  const localBody = { ...local, type: 'send', conversationId: cid, keyMessages: keyRequests.map(r => ({ ...JSON.parse(r.body), event_type: r.event_type, txn_id: r.txn_id })) };
  await rpc(localBody); await rpc(localBody);
  const view = await waitFor(() => rpc({ type: 'view', conversationId: cid }), v => !v.running && v.messages.some(m => m.local && !m.synced && m.status === 'committed' && m.message.senderId === aiId), 'offline encrypted attachment and reference');
  const localMessage = view.messages.find(m => m.message.id === local.messageId).message;
  const localCipher = await rpc({ type: 'attachment', conversationId: cid, messageId: local.messageId, index: 0, revision: '1' });
  assert.deepEqual(Buffer.from(sdk.Attachment.decrypt(new sdk.EncryptedAttachment(Buffer.from(localCipher.data, 'base64'), localFile.metadata[0].mediaEncryptionInfo))), Buffer.from('离线文件密文附件'));
  assert(JSON.stringify(modelInputs.at(-1)).includes('群图片讨论来源'));
  assert(!JSON.stringify(modelInputs.at(-1)).includes('离线文件密文附件'), 'ordinary file bytes do not become model input');
  await startCloud();
  for (const request of keyRequests) await outgoing(human, request);
  await rpc({ type: 'sync', conversationId: cid });
  const finalPage = await waitFor(() => http(human, `/chat/conversations/${cid}/messages`), p => p.messages.some(m => m.id === local.messageId), 'local attachment synchronization');
  const uploaded = finalPage.messages.find(m => m.id === local.messageId);
  assert.deepEqual(Buffer.from(await download(human, uploaded, 0)), Buffer.from(localFile.uploads[0].data, 'base64'));
  assert.equal(modelInputs.length, baseline + 4, 'each action generated exactly once');
  assert.equal(sql(`SELECT count(*) FROM cloud_node_tasks WHERE input_id='${local.messageId}'`), '0');
  await http(human, `/chat/messages/${groupBody.messageId}/withdraw`, 'POST', { requestId: randomUUID(), expectedRevision: '1' });
  const updated = await http(human, `/chat/conversations/${cid}/messages`);
  assert(updated.messages.filter(m => m.contextReference).every(m => m.contextReference.stale));
  console.log('PASS: real Agent encrypted group image, private group-topic continuation, encrypted one-shot contact, offline file/reference over Windows IPC, byte-identical synchronization, no repeated calls, source withdrawal invalidates references.');
};
