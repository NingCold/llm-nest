/* Fourth-batch human history conversion. Called by crypto_acceptance.cjs with its disposable fixture.
 * Loads the product crypto modules and real official WASM SDK; never prints recovery data or cookies.
 */
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const { createRequire } = require('node:module');
const { randomUUID, webcrypto, createHash } = require('node:crypto');
const vm = require('node:vm');
const root = path.resolve(__dirname, '..');
const webRequire = createRequire(path.join(root, 'client/web/package.json'));
const ts = webRequire('typescript');
const sha = bytes => createHash('sha256').update(bytes).digest('hex');

exports.run = async c => {
  const { sdk, signup, device, bootstrap, trust, http, sql, origin, setRequireEncryption } = c;
  assert.equal(typeof setRequireEncryption, 'function', 'fixture must provide isolated strict-mode restart');
  const a = await device(await signup('历史转换者')), b = await device(await signup('旧记录作者'));
  await bootstrap(a); await bootstrap(b);
  let conversation = await http(a, '/chat/direct', 'POST', { accountId: b.account });
  await trust(a, b); await trust(b, a);
  const png = Buffer.from('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR4nGP4z8DwHwAFAAH/iZk9HQAAAABJRU5ErkJggg==', 'base64');
  const originalFile = Buffer.from('旧附件-完整字节\0\xff', 'utf8');
  const markerA = `历史甲-${randomUUID()}`, markerB = `历史乙-${randomUUID()}`;
  const first = await http(a, `/chat/conversations/${conversation.id}/messages`, 'POST', { requestId: randomUUID(), text: markerA, images: [{ mime: 'image/png', data: png.toString('base64') }] });
  const second = await http(b, `/chat/conversations/${conversation.id}/messages`, 'POST', { requestId: randomUUID(), text: markerB, replyTo: first.id, files: [{ name: '历史文件.txt', mime: 'text/plain', data: originalFile.toString('base64') }] });
  const formerAi = randomUUID(), intentId = randomUUID(), intentText = `历史约定-${randomUUID()}`;
  sql(`INSERT INTO accounts(id,name,kind) VALUES('${formerAi}','旧终态约定目标','ai')`);
  sql(`INSERT INTO cloud_contact_intents(id,conversation,creator_id,ai_id,content,due_at,expires_at,utc_offset_minutes,status,revision) VALUES('${intentId}','${conversation.id}','${b.account}','${formerAi}','${intentText}',now()-interval '2 day',now()-interval '1 day',480,'cancelled',7)`);
  sql(`UPDATE cloud_conversations SET read_only=true WHERE id='${conversation.id}'`);
  conversation = (await http(a, '/chat/init')).conversations.find(v => v.id === conversation.id);
  const originalMetadata = sql(`SELECT string_agg(id::text||':'||revision::text||':'||position::text||':'||sender::text,',' ORDER BY position) FROM cloud_messages WHERE conversation='${conversation.id}'`);
  const originalReads = sql(`SELECT string_agg(owner::text||':'||read_position::text,',' ORDER BY owner) FROM cloud_list WHERE conversation='${conversation.id}'`);
  assert(c.directory, 'fixture must provide its private target directory');
  const snapshotPage = await http(a, `/chat/conversations/${conversation.id}/messages?limit=100`);
  const snapshotIntents = (await http(a, `/chat/conversations/${conversation.id}/participation`)).intents;
  const snapshotAttachments = [];
  for (const message of snapshotPage.messages) for (const attachment of message.attachments) {
    const r = await fetch(`${origin}/api/chat/messages/${message.id}/attachments/${attachment.index}?revision=${message.revision}`, { headers: { Cookie: a.cookie, 'X-LLMN-Client': '1' } });
    assert.equal(r.status, 200);
    snapshotAttachments.push({ messageId: message.id, index: attachment.index, revision: message.revision, data: Buffer.from(await r.arrayBuffer()).toString('base64') });
  }
  const snapshotBytes = Buffer.from(JSON.stringify({ conversation, messages: snapshotPage.messages, intents: snapshotIntents, attachments: snapshotAttachments }));
  fs.writeFileSync(path.join(c.directory, `history-snapshot-${conversation.id}.private.json`), snapshotBytes, { mode: 0o600 });
  const storage = new Map(), modules = new Map(), uploads = [];
  let loseFirstUpload = true;
  class HttpError extends Error { constructor(status, message) { super(message); this.status = status; } }
  class ConnectionError extends Error {}
  const api = {
    server: origin, HttpError, ConnectionError,
    request: async (route, method = 'GET', body) => {
      const r = await fetch(origin + '/api' + route, { method, headers: { Cookie: a.cookie, Origin: origin, 'X-LLMN-Client': '1', ...(body === undefined ? {} : { 'Content-Type': 'application/json' }) }, body: body === undefined ? undefined : JSON.stringify(body) });
      const text = await r.text();
      if (!r.ok) throw new HttpError(r.status, JSON.parse(text).message || text);
      const result = text ? JSON.parse(text) : null;
      if (method === 'PUT' && route.includes('/history-migrations/')) {
        uploads.push(structuredClone({ route, body }));
        if (loseFirstUpload) { loseFirstUpload = false; throw new ConnectionError('simulated lost acknowledgement after successful history commit'); }
      }
      return result;
    },
    downloadBytes: async route => {
      const r = await fetch(origin + '/api' + route, { headers: { Cookie: a.cookie, Origin: origin, 'X-LLMN-Client': '1' } });
      assert.equal(r.status, 200); return new Uint8Array(await r.arrayBuffer());
    },
  };
  function load(file) {
    const absolute = path.resolve(file);
    if (absolute === path.join(root, 'client/web/src/cloud/api.ts')) return api;
    if (modules.has(absolute)) return modules.get(absolute);
    const result = {}; modules.set(absolute, result);
    const source = fs.readFileSync(absolute, 'utf8');
    const seed = absolute.endsWith(path.join('crypto', 'engine.ts')) ? '\nexports.seed = state => { active = { sdk: state.sdk, machine: state.machine, account: state.account, owner: ownerOf(state.account), deviceId: state.device, cursor: state.cursor }; };' : '';
    vm.runInNewContext(ts.transpileModule(source, { compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 } }).outputText + seed, {
      exports: result, crypto: webcrypto, indexedDB: webRequire('fake-indexeddb').indexedDB, TextEncoder, TextDecoder, Uint8Array, Map, Blob, atob, btoa, Event, CustomEvent,
      localStorage: { getItem: key => storage.get(key) ?? null, setItem: (key, value) => storage.set(key, value), removeItem: key => storage.delete(key) }, window: { dispatchEvent() {} },
      // This fixture's only image is a known 1x1 PNG. Assert its recovered bytes before emulating browser image dimensions.
      createImageBitmap: async blob => { assert.deepEqual(Buffer.from(await blob.arrayBuffer()), png); return { width: 1, height: 1, close() {} }; },
      require: name => {
        if (name === '@tauri-apps/api/core') return { invoke: () => { throw Error('fixture must not use native transport'); } };
        if (!name.startsWith('.')) return webRequire(name);
        const target = path.resolve(path.dirname(absolute), name);
        return load(fs.existsSync(target + '.ts') ? target + '.ts' : target + '.tsx');
      },
    });
    return result;
  }
  const engine = load(path.join(root, 'client/web/src/cloud/crypto/engine.ts'));
  const migration = load(path.join(root, 'client/web/src/cloud/crypto/migration.ts'));
  const content = load(path.join(root, 'client/web/src/cloud/crypto/content.ts'));
  const attachments = load(path.join(root, 'client/web/src/cloud/crypto/attachments.ts'));
  engine.seed({ ...a, sdk });
  try {
    const file = await engine.createRecoveryFile(); await engine.commitRecoveryFile(file);
    const recoveryDigest = sha(JSON.stringify(file));
    const preview = await migration.previewMigration(conversation.id);
    assert.equal(preview.messageCount, 2); assert.equal(preview.contactIntentCount, 1); assert.equal(preview.attachmentCount, 2);
    // This proves the fixture snapshot; production PostgreSQL/attachments/node consistency is covered by the separate backup acceptance.
    const backupDigest = sha(snapshotBytes);
    await migration.beginMigration(a.account, conversation, preview, backupDigest, recoveryDigest, file);
    await assert.rejects(migration.convertMigrationPage(a.account, conversation), /lost acknowledgement/);
    const pending = (await migration.migrationSession(a.account, conversation.id)).pending;
    assert(pending); assert(!JSON.stringify(pending).includes(markerA)); assert(!JSON.stringify(pending).includes(markerB));
    await migration.retryMigration(a.account, conversation.id);
    assert.deepEqual(uploads[0], uploads[1]);
    loseFirstUpload = true;
    await assert.rejects(migration.convertMigrationPage(a.account, conversation), /lost acknowledgement/);
    const submittedCount = uploads.length;
    await migration.confirmMigrationResult(a.account, conversation.id);
    assert.equal((await migration.migrationSession(a.account, conversation.id)).pending, undefined);
    assert.equal(uploads.length, submittedCount, 'receipt confirmation must not resubmit or re-encrypt');
    let state = await migration.refreshMigration(a.account, conversation.id);
    while (state.status.remaining) state = await migration.convertMigrationPage(a.account, conversation);
    const all = []; let after = '0';
    for (;;) { const page = await migration.migrationItems(state.status.id, after, true); all.push(...page.items); if (!page.hasMore) break; after = page.next; }
    assert.equal(all.length, 3);
    const reader = await engine.recoveryReader(file);
    try {
      for (const item of all) {
        assert.equal(item.record.historyMigration.actorId, a.account);
        assert.equal(item.record.sealed.sender, c.userId(a.account));
        if (item.kind === 'message') {
          const plain = await content.projectMessage(item.record, conversation, reader.unseal);
          assert.equal(plain.cryptoError, undefined);
          assert.equal(plain.text, item.id === first.id ? markerA : markerB);
          assert.equal(plain.senderId, item.id === first.id ? a.account : b.account);
          assert.equal(plain.replyTo, item.id === second.id ? first.id : null);
          const bytes = await attachments.attachmentBytes(plain, plain.attachments[0], `/chat/messages/${item.id}/attachments/0?revision=${item.revision}`);
          assert.deepEqual(Buffer.from(bytes), item.id === first.id ? png : originalFile);
          const wrong = structuredClone(item.record); wrong.historyMigration.actorId = b.account;
          assert((await content.projectMessage(wrong, conversation, reader.unseal)).cryptoError);
        } else {
          const plain = await content.projectIntent(item.record, conversation, reader.unseal);
          assert.equal(plain.cryptoError, undefined); assert.equal(plain.content, intentText); assert.equal(plain.creatorId, b.account);
          assert.equal(plain.revision, '7'); assert.equal(plain.sealed.eventId, `$${intentId}.1.migration.${state.status.id}`);
        }
      }
    } finally { reader.close(); }
    await migration.verifyAndFinishMigration(a.account, conversation, file, recoveryDigest);
    state = await migration.refreshMigration(a.account, conversation.id);
    assert.equal(state.status.state, 'complete'); assert.match(state.status.verificationDigest, /^[a-f0-9]{64}$/);
    const complete = (await http(a, '/chat/init')).conversations.find(v => v.id === conversation.id);
    assert.equal(complete.readOnly, true); assert.equal(complete.encryption.historyComplete, true); assert.equal(complete.encryption.activationPosition, '0');
    assert.equal(sql(`SELECT string_agg(id::text||':'||revision::text||':'||position::text||':'||sender::text,',' ORDER BY position) FROM cloud_messages WHERE conversation='${conversation.id}'`), originalMetadata);
    assert.equal(sql(`SELECT string_agg(owner::text||':'||read_position::text,',' ORDER BY owner) FROM cloud_list WHERE conversation='${conversation.id}'`), originalReads);
    const stored = sql(`SELECT string_agg(body||COALESCE(sealed::text,''),'') FROM cloud_messages WHERE conversation='${conversation.id}'`);
    assert(!stored.includes(markerA)); assert(!stored.includes(markerB));
    assert.equal(sql(`SELECT content FROM cloud_contact_intents WHERE id='${intentId}'`), '');
    // A valid migration signature does not prove equality with the previewed plaintext.
    const badGroup = await http(a, '/chat/groups', 'POST', { requestId: randomUUID(), name: '签名正确但内容不匹配', memberIds: [b.account] });
    const badOriginal = await http(b, `/chat/conversations/${badGroup.id}/messages`, 'POST', { requestId: randomUUID(), text: 'must remain original' });
    const badPreview = await migration.previewMigration(badGroup.id);
    const badSnapshot = Buffer.from(JSON.stringify(badOriginal));
    fs.writeFileSync(path.join(c.directory, `history-snapshot-${badGroup.id}.private.json`), badSnapshot, { mode: 0o600 });
    await migration.beginMigration(a.account, badGroup, badPreview, sha(badSnapshot), recoveryDigest, file);
    const badStatus = (await migration.refreshMigration(a.account, badGroup.id)).status;
    const badItem = (await migration.migrationItems(badStatus.id)).items[0];
    const badProof = { id: badStatus.id, actorId: a.account, sourceDigest: badItem.sourceDigest };
    const badInner = { version: 1, server: origin, kind: 'historyMessage', conversationId: badGroup.id, conversationRevision: badStatus.conversationRevision, messageId: badItem.id, senderId: b.account, revision: badItem.revision, text: 'validly signed but changed', replyTo: null, mentions: [], runId: null, sequence: null, reference: null, attachments: [], historyMigration: badProof };
    const badRoom = c.roomId(badGroup.id);
    const badContent = await engine.seal(badRoom, [a.account, b.account], 'com.llmn.content.v1', badInner, false, [a.account], true);
    const badSealed = await engine.signEnvelope({ version: 1, roomId: badRoom, sender: c.userId(a.account), deviceId: a.device, eventId: `$${badItem.id}.${badItem.revision}.migration.${badStatus.id}`, content: badContent });
    await http(a, `/crypto/history-migrations/${badStatus.id}/items/message/${badItem.id}`, 'PUT', { requestId: randomUUID(), deviceId: a.device, expectedRevision: badItem.revision, sourceDigest: badItem.sourceDigest, sealed: badSealed, encryptedAttachments: [] });
    assert.equal((await engine.unseal(badSealed)).content.text, badInner.text, 'real SDK and both signatures accept this envelope');
    await assert.rejects(migration.verifyAndFinishMigration(a.account, badGroup, file, recoveryDigest), /与预览原内容不一致/);
    assert.equal((await http(a, `/crypto/history-migrations/${badStatus.id}`)).state, 'active', 'mismatched content is never marked complete');
    await setRequireEncryption(true);
    try {
      assert.equal((await http(a, '/chat/init')).requireEncryption, true);
      const group = await http(a, '/chat/groups', 'POST', { requestId: randomUUID(), name: '强制加密边界', memberIds: [b.account] });
      await http(a, `/chat/conversations/${group.id}/messages`, 'POST', { requestId: randomUUID(), text: 'must reject plaintext' }, 409);
    } finally { await setRequireEncryption(false); }
    if (c.serveUI) {
      fs.writeFileSync(path.join(c.directory, 'ui-login.private.json'), JSON.stringify({ origin, email: a.email, password: a.password, accountId: a.account, conversationId: conversation.id }), { mode: 0o600 });
      fs.writeFileSync(path.join(c.directory, 'ui-recovery.private.json'), JSON.stringify(file), { mode: 0o600 });
    }
    console.log('PASS: real frontend/WASM history conversion preserves different original authors, revisions/read state/replies, image/file bytes and terminal contact; lost upload retries exactly; fresh recovery decrypts entire manifest before finish; archive and strict encryption remain enforced; correctly signed altered content cannot finish.');
  } finally { engine.lockCrypto(); b.machine.close(); }
};
