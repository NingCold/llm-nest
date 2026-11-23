const assert = require('node:assert/strict'), fs = require('node:fs'), vm = require('node:vm'), ts = require('typescript'), { indexedDB } = require('fake-indexeddb');
class HttpError extends Error { constructor(status) { super(`HTTP ${status}`); this.status = status; } }
const canonical = {}; vm.runInNewContext(ts.transpileModule(fs.readFileSync(require.resolve('../src/cloud/crypto/canonical.ts'), 'utf8'), { compilerOptions: { module: ts.ModuleKind.CommonJS } }).outputText, { exports: canonical });
const calls = [], encrypted = [], signed = [], readers = [], checked = [];
const status = { id: 'migration', conversationId: 'room', conversationRevision: '7', sourceDigest: 'a'.repeat(64), state: 'active', remaining: 1, backupDigest: 'b'.repeat(64), recoveryDigest: 'c'.repeat(64), verificationDigest: null };
const original = { id: 'old', conversationId: 'room', senderId: 'old-author', revision: '3', position: '1', text: 'old plaintext', mentions: [], replyTo: null, contextReference: null, status: 'committed', attachments: [], sealed: null, historyMigration: null, audienceRevision: '5' };
const item = { originalAudienceRevision: '5', kind: 'message', id: 'old', revision: '3', sourceDigest: require('node:crypto').createHash('sha256').update(canonical.canonicalJson([original, []])).digest('hex'), converted: false, record: original };
const retainedRecord = { ...original, id: 'retained', text: '', attachments: [{ index: 0, kind: 'encrypted', mime: 'application/octet-stream', name: null, byteLength: 2 }], sealed: { cipher: 'already-sealed' }, historyMigration: { id: 'previous-migration', actorId: 'previous', sourceDigest: 'prior' } };
const retained = { kind: 'message', id: 'retained', revision: '3', converted: true, sourceDigest: require('node:crypto').createHash('sha256').update(canonical.canonicalJson([retainedRecord, [require('node:crypto').createHash('sha256').update(Buffer.from([1, 2])).digest('hex')]])).digest('hex'), record: retainedRecord };
const receipts = new Map();
const fingerprint = (operation, body) => require('node:crypto').createHash('sha256').update(canonical.canonicalJson([operation, body])).digest('hex');
let lose = true, rejectBegin = false, revokedUpload = false, currentDevice = 'device', recoveredText = 'old plaintext', receiptMode = 'valid', failBeforeBegin = true, loseBegin = true, loseFinish = true;
const api = { server: 'https://migration.invalid', HttpError, downloadBytes: async () => new Uint8Array([1, 2]), request: async (path, method = 'GET', body) => {
  calls.push(structuredClone({ path, method, body }));
  if (path.startsWith('/crypto/history-migration-receipts/')) {
    const url = new URL(path, api.server);
    const value = receipts.get(url.pathname.split('/').pop());
    if (receiptMode === 'offline') throw new Error('offline');
    if (value && receiptMode !== 'none') return { receipt: {
      fingerprint: receiptMode === 'mismatch' ? '0'.repeat(64) : value, migration: structuredClone(status)
    } };
    return { receipt: null, ...(url.searchParams.has('legacyConversationId') && receiptMode !== 'old-server'
      ? { unsignedControlFence: 'device-proof-v1' } : {}) };
  }
  if (path === '/crypto/recovery') return { version: 'current' };
  if (body?.deviceId && body.deviceId !== currentDevice) throw new HttpError(403);
  if (path.startsWith('/crypto/history-migrations?conversationId=')) return { migration: structuredClone(status) };
  if (method === 'POST' && path === '/crypto/history-migrations') { assert.equal(body.proof, 'history.begin'); if (rejectBegin) throw new HttpError(409); if (failBeforeBegin) throw Error('lost before begin commit'); receipts.set(body.requestId, fingerprint('begin', body)); if (loseBegin) { loseBegin = false; throw Error('lost begin acknowledgement'); } return structuredClone(status); }
  if (path.endsWith('/items?after=0&limit=32')) return { items: item.converted ? [] : [structuredClone(item)], next: '1', hasMore: false };
  if (path.endsWith('/items?after=0&limit=32&includeConverted=true')) return { items: [structuredClone(item), structuredClone(retained)], next: '2', hasMore: false };
  if (method === 'PUT') { if (revokedUpload) throw new HttpError(403); item.converted = true; item.record = { ...original, sealed: body.sealed, historyMigration: { id: status.id, actorId: 'human', sourceDigest: item.sourceDigest }, audienceRevision: '7' }; status.remaining = 0; const parts = path.split('/'); receipts.set(body.requestId, fingerprint(`${parts[3]}:${parts[5]}:${parts[6]}`, body)); if (lose) { lose = false; throw Error('lost response'); } return { ...item }; }
  if (path.endsWith('/finish')) { assert.equal(body.proof, `history.finish:${status.id}`); status.state = 'complete'; status.verificationDigest = body.verificationDigest; receipts.set(body.requestId, fingerprint(`finish:${status.id}`, body)); if (loseFinish) { loseFinish = false; throw Error('lost finish acknowledgement'); } return structuredClone(status); }
  return structuredClone(status);
} };
const engine = { signDeviceCommand: async (account, action, body) => ({ ...body, deviceId: currentDevice, proof: action }), cryptoDevice: () => currentDevice, recoveryConfirmed: () => true, matrixRoom: id => `!${id}:llmn`, matrixUser: id => `@${id}:llmn`, recoveryReader: async () => { const reader = { close() { reader.closed = true; }, unseal: async sealed => { checked.push(sealed); return { type: 'com.llmn.content.v1', content: { text: recoveredText } }; } }; readers.push(reader); return reader; }, seal: async (room, users, type, content, offline, required) => { encrypted.push(structuredClone({ content, required })); return { cipher: `frozen-${encrypted.length}` }; }, signEnvelope: async envelope => { signed.push(envelope); return envelope; } };
const migrationExports = {};
vm.runInNewContext(ts.transpileModule(fs.readFileSync(require.resolve('../src/cloud/crypto/migration.ts'), 'utf8'), { compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 } }).outputText, { exports: migrationExports, indexedDB, crypto: require('node:crypto').webcrypto, TextEncoder, Uint8Array, btoa, require: name => name === '../api' ? api : name === './engine' ? engine : name === './content' ? { projectMessage: async (record, conversation, unseal) => { const event = await unseal(record.sealed); return { ...record, text: event.content.text, ...(record.id === 'retained' ? { attachments: [{ index: 0, kind: 'file', mime: 'text/plain', byteLength: 2, mediaEncryptionInfo: 'sdk-info' }] } : {}) }; } } : name === './canonical' ? canonical : name === './attachments' ? { decodeAttachment: async () => new Uint8Array([7, 8]) } : {} });
(async () => {
  const conversation = { id: 'room', kind: 'direct', members: [{ account: { id: 'human' } }, { account: { id: 'old-author' } }] };
  await assert.rejects(migrationExports.beginMigration('human', conversation, { conversationRevision: '7', sourceDigest: status.sourceDigest }, status.backupDigest, status.recoveryDigest, {}), /lost before begin/);
  const beginPending = (await migrationExports.migrationSession('human', 'room')).pending;
  await assert.rejects(migrationExports.confirmMigrationResult('human', 'room'), /尚未找到/);
  assert.deepEqual((await migrationExports.migrationSession('human', 'room')).pending, beginPending, 'no receipt for a new request is unknown, never evidence that it may be discarded');
  failBeforeBegin = false;
  await assert.rejects(migrationExports.retryMigration('human', 'room'), /lost begin acknowledgement/);
  currentDevice = 'begin-restored-device';
  await assert.rejects(migrationExports.retryMigration('human', 'room'), e => e.status === 403);
  await migrationExports.confirmMigrationResult('human', 'room');
  assert.equal((await migrationExports.migrationSession('human', 'room')).pending, undefined, 'a matching begin fingerprint resolves the effect after original device revocation');
  assert.ok(readers[0].closed);
  await assert.rejects(migrationExports.convertMigrationPage('human', conversation), /lost response/);
  const pending = (await migrationExports.migrationSession('human', 'room')).pending;
  assert.equal(JSON.stringify(pending).includes('old plaintext'), false, 'pending command contains frozen ciphertext, never source plaintext');
  assert.equal(signed[0].sender, '@human:llmn');
  assert.equal(signed[0].eventId, '$old.3.migration.migration');
  assert.equal(encrypted[0].content.senderId, 'old-author', 'historical author is preserved separately from the migration signature');
  assert.equal(JSON.stringify(encrypted[0].required), '["human"]', 'a stopped old AI need not have a fake device');
  currentDevice = 'recovered-device'; revokedUpload = true;
  await assert.rejects(migrationExports.retryMigration('human', 'room'), e => e.status === 403);
  await migrationExports.refreshMigration('human', 'room');
  assert.deepEqual((await migrationExports.migrationSession('human', 'room')).pending, pending, 'a 403 or status refresh alone cannot resolve an unknown PUT');
  assert.equal(item.converted, true); assert.deepEqual(item.record.sealed, pending.body.sealed);
  await assert.rejects(migrationExports.verifyAndFinishMigration('human', conversation, {}, status.recoveryDigest), /原转换请求/);
  receiptMode = 'mismatch';
  await assert.rejects(migrationExports.confirmMigrationResult('human', 'room'), /不一致/);
  assert.deepEqual((await migrationExports.migrationSession('human', 'room')).pending, pending, 'different fingerprint never clears the original request');
  receiptMode = 'valid';
  await migrationExports.confirmMigrationResult('human', 'room');
  assert.equal((await migrationExports.migrationSession('human', 'room')).pending, undefined);
  assert.equal(currentDevice, 'recovered-device');
  revokedUpload = false;
  assert.equal(encrypted.length, 1);
  const writes = calls.filter(c => c.method === 'PUT');
  assert.deepEqual(writes[0], writes[1], 'unknown upload retries the same request and cipher');
  recoveredText = 'tampered text with otherwise valid authenticated envelope';
  await assert.rejects(migrationExports.verifyAndFinishMigration('human', conversation, {}, status.recoveryDigest), /与预览原内容不一致/);
  assert.equal((await migrationExports.migrationSession('human', 'room')).pending, undefined, 'a decryptable but altered migration never queues finish');
  recoveredText = 'old plaintext';
  await assert.rejects(migrationExports.verifyAndFinishMigration('human', conversation, {}, status.recoveryDigest), /lost finish acknowledgement/);
  currentDevice = 'finish-restored-device';
  await assert.rejects(migrationExports.retryMigration('human', 'room'), e => e.status === 403);
  await migrationExports.confirmMigrationResult('human', 'room');
  assert.equal(readers.length, 3); assert.ok(readers[1].closed); assert.ok(readers[2].closed);
  assert.equal(checked.length, 3, 'whole converted manifest is decrypted through the independent reader');
  assert.match(status.verificationDigest, /^[a-f0-9]{64}$/);
  assert.equal((await migrationExports.migrationSession('human', 'room')).status.state, 'complete');
  rejectBegin = true;
  await assert.rejects(migrationExports.beginMigration('human', conversation, { conversationRevision: '7', sourceDigest: status.sourceDigest }, status.backupDigest, status.recoveryDigest, {}), e => e.status === 409);
  const rejected = await migrationExports.migrationSession('human', 'room');
  assert.equal(rejected.pending, undefined, 'first definitive rejection cannot remain an unresolvable send lock');
  assert.equal(rejected.rejected.length, 1, 'the exact rejected intent is preserved rather than overwritten');
  // Freeze the exact shape written to IndexedDB by the published client, without
  // letting current code sign it first. Recovery must never dispatch these commands.
  async function pendingFromAlpha(account, command) {
    const db = await new Promise((resolve, reject) => {
      const open = indexedDB.open('llmn-history-migrations', 1);
      open.onsuccess = () => resolve(open.result); open.onerror = () => reject(open.error);
    });
    try {
      const tx = db.transaction('sessions', 'readwrite');
      tx.objectStore('sessions').put({ version: 3, pending: command }, JSON.stringify([api.server, account, 'room']));
      await new Promise((resolve, reject) => { tx.oncomplete = resolve; tx.onerror = () => reject(tx.error); });
    } finally { db.close(); }
  }
  for (const operation of ['begin', 'finish:migration']) {
    const path = '/crypto/history-migrations' + (operation === 'begin' ? '' : '/migration/finish');
    const command = { path, method: 'POST', body: { deviceId: 'old', requestId: require('node:crypto').randomUUID(),
      ...(operation === 'begin' ? { conversationId: 'room', recoveryDigest: status.recoveryDigest } : { verificationDigest: status.verificationDigest }) } };
    const account = 'legacy-' + operation;
    await pendingFromAlpha(account, command);
    const before = calls.length;
    await assert.rejects(migrationExports.retryMigration(account, 'room'), /旧版/);
    assert.equal(calls.length, before, 'legacy requests never dispatch');
    for (const mode of ['offline', 'old-server']) {
      receiptMode = mode;
      await assert.rejects(migrationExports.reconcileLegacyControl(account, 'room'), /offline|尚未找到/);
      assert.deepEqual((await migrationExports.migrationSession(account, 'room')).pending, command);
    }
    receipts.set(command.body.requestId, fingerprint(operation, command.body));
    receiptMode = 'mismatch';
    await assert.rejects(migrationExports.reconcileLegacyControl(account, 'room'), /不一致/);
    assert.deepEqual((await migrationExports.migrationSession(account, 'room')).pending, command);
    receiptMode = 'valid';
    const committed = await migrationExports.reconcileLegacyControl(account, 'room');
    assert.equal(committed.pending, undefined);
    assert.equal(committed.archivedControls, undefined);
    assert.equal(committed.status.id, status.id);

    receipts.delete(command.body.requestId);
    await pendingFromAlpha(account, command);
    await assert.rejects(migrationExports.confirmMigrationResult(account, 'room'), /尚未找到/);
    const archived = await migrationExports.reconcileLegacyControl(account, 'room');
    assert.equal(archived.pending, undefined);
    assert.deepEqual((await migrationExports.migrationSession(account, 'room')).archivedControls, [command]);
    assert.ok(calls.slice(before).every(call => call.method === 'GET'), 'confirming or archiving a frozen control is read-only');
  }
  assert.equal(migrationExports.legacyControl({ method: 'POST', path: '/crypto/history-migrations', body: { proof: 'saved' } }), false);
  assert.equal(migrationExports.legacyControl({ method: 'PUT', path: '/crypto/history-migrations/migration/items/message/old', body: {} }), false);

  // Each reader gets its own generation. Missing key/index allows the other reader;
  // an invalid signature, source mismatch or a stale supplement never does.
  const originalFile = { format: 'llmn-recovery-v1', version: 'old' };
  const extra = { file: { format: 'llmn-recovery-v2', version: 'current' }, digest: 'd'.repeat(64) };
  const missing = Symbol('missing session');
  const attempts = [], opened = [];
  let failSignature = false;
  status.state = 'active';
  engine.recoveryReader = async file => {
    const old = file === originalFile;
    const reader = { sealed: old, lacksSession: failure => failure === missing, close() { reader.closed = true; },
      async unseal(blob) {
        attempts.push(file.version);
        if (failSignature) throw Error('signature mismatch');
        if ((blob.cipher === 'already-sealed') === old) throw missing;
        return { type: 'com.llmn.content.v1', content: { text: recoveredText } };
      } };
    opened.push(reader);
    return reader;
  };
  const finish = supplement => migrationExports.verifyAndFinishMigration('multi', conversation, originalFile, status.recoveryDigest, supplement);
  await assert.rejects(finish({ ...extra, file: { ...extra.file, version: 'stale' } }), /不是当前恢复版本/);
  await assert.rejects(finish({ ...extra, file: originalFile }), /不同的当前 v2/);
  await assert.rejects(finish({ ...extra, digest: status.recoveryDigest }), /不同的当前 v2/);
  assert.ok(opened.every(reader => reader.closed));
  failSignature = true;
  await assert.rejects(finish(extra), /signature mismatch/);
  assert.deepEqual(attempts, ['old']);
  assert.ok(opened.every(reader => reader.closed));
  failSignature = false;
  attempts.length = 0;
  const completed = await finish(extra);
  assert.equal(completed.status.state, 'complete');
  assert.deepEqual(attempts, ['old', 'old', 'current']);
  assert.ok(opened.every(reader => reader.closed));
  const lastFinish = calls.at(-1).body;
  assert.deepEqual(lastFinish.recoveryDigests, [status.recoveryDigest, extra.digest]);
  assert.equal(lastFinish.supplementalRecoveryVersion, extra.file.version);
  const React = require('react'), unauthorizedLoads = [];
  function panelHarness({ blocked = false, pending = false, complete = false, role = 'owner', kind = 'group', cachedSlots } = {}) {
    const slots = cachedSlots ?? [[], { conversationId: 'room', messageCount: 1, contactIntentCount: 0, attachmentCount: 0, sourceDigest: 'source' }, pending ? { pending: pending === 'alpha' ? { method: 'POST', path: '/crypto/history-migrations', body: {} } : { path: 'saved', body: {} }, ...(complete ? { status: { state: 'complete' } } : {}) } : {}, 'b'.repeat(64), {}, 'c'.repeat(64), true];
    let index = 0; const result = {};
    const hooks = { ...React, useState(initial) { const i = index++; if (!(i in slots)) slots[i] = typeof initial === 'function' ? initial() : initial; return [slots[i], value => { slots[i] = value; }]; }, useRef: () => ({ current: 0 }), useEffect(fn) { if (kind === 'group' && role === 'member') fn(); } };
    vm.runInNewContext(ts.transpileModule(fs.readFileSync(require.resolve('../src/cloud/crypto/HistoryMigrationPanel.tsx'), 'utf8'), { compilerOptions: { jsx: ts.JsxEmit.ReactJSX, module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 } }).outputText, { exports: result, TextDecoder, Uint8Array, require: name => name === 'react' ? hooks : name === '../api' ? { HttpError } : name === './engine' ? { cryptoDevice: () => 'device' } : name === './migration' ? { legacyControl: migrationExports.legacyControl, digest: async () => 'e'.repeat(64), migrationSession: async () => { unauthorizedLoads.push('local'); return {}; }, refreshMigration: async () => { unauthorizedLoads.push('network'); return {}; } } : require(name) });
    return result.HistoryMigrationPanel({ account: 'human', conversation: { id: 'room', kind, readOnly: true, members: [{ account: { id: 'human' }, role }] }, blocked, onComplete: async () => {} });
  }
  const all = node => !node || typeof node !== 'object' ? [] : [node, ...React.Children.toArray(node.props?.children).flatMap(all)];
  const label = node => typeof node === 'string' ? node : React.Children.toArray(node?.props?.children).map(label).join('');
  const buttons = options => all(panelHarness(options)).filter(n => n.type === 'button');
  assert.equal(buttons({ blocked: true }).find(n => label(n) === '确认备份并开始转换').props.disabled, true, 'unconfirmed or unsynced local sends block migration');
  const memberTree = panelHarness({ role: 'member' });
  assert.equal(all(memberTree).some(n => n.type === 'p' && label(n) === '群历史转换须由当前群主或管理员操作。'), true);
  assert.equal(all(memberTree).some(n => n.type === 'button' || n.type === 'input'), false, 'ordinary group members see only the permission explanation');
  assert.equal(buttons({ role: 'member', pending: true }).length, 0, 'cached pending commands do not expose unauthorized confirmation actions');
  await new Promise(resolve => setImmediate(resolve));
  assert.deepEqual(unauthorizedLoads, [], 'opening group details as a member reads neither cached migration session nor privileged remote status');
  assert.equal(buttons({ kind: 'direct', role: 'member' }).find(n => label(n) === '确认备份并开始转换').props.disabled, false, 'read-only direct archives permit authorized conversion while staying read-only');
  const alphaActions = buttons({ pending: 'alpha' }).map(label);
  assert.ok(alphaActions.includes('核对并归档旧版请求'));
  assert.equal(alphaActions.includes('手动重试原转换请求'), false);
  assert.ok(buttons({ pending: true }).some(n => label(n) === '手动重试原转换请求'));
  assert.ok(buttons({ pending: true }).some(n => label(n) === '核对原转换结果'));
  assert.ok(buttons({ pending: true, complete: true }).some(n => label(n) === '核对原转换结果'), 'finish result remains confirmable even if authoritative status is already complete');
  assert.equal(buttons({ pending: true }).some(n => label(n) === '确认备份并开始转换'), false);
  const pickerSlots = [[], undefined, { status: { state: 'active', remaining: 0 } }, '', originalFile, status.recoveryDigest, false, false, '', extra];
  const picker = () => all(panelHarness({ cachedSlots: pickerSlots })).filter(n => n.type === 'input' && n.props.type === 'file');
  const fileInput = (name, json) => ({ value: name, files: [ { name, arrayBuffer: async () => new TextEncoder().encode(json) } ] });
  const change = async (slot, input) => {
    const node = picker()[slot];
    node.props.onChange({ currentTarget: input });
    await new Promise(resolve => setImmediate(resolve));
  };
  const wrong = fileInput('broken.json', '{no');
  await change(1, wrong);
  assert.equal(wrong.value, '');
  assert.equal(pickerSlots[9], undefined, 'failed supplemental replacement discards the previously selected supplement');
  const valid = fileInput('new.json', JSON.stringify(extra.file));
  await change(1, valid);
  assert.equal(valid.value, 'new.json');
  assert.equal(pickerSlots[9].file.version, 'current');
  const tree = all(panelHarness({ cachedSlots: pickerSlots }));
  const control = tree.filter(n => n.type === 'input' && n.props.type === 'file')[1];
  (control.props.ref ?? control.ref).current = valid;
  tree.find(n => n.type === 'button' && label(n) === '移除补充文件').props.onClick();
  assert.equal(valid.value, '');
  assert.equal(pickerSlots[9], undefined);
  const invalid = fileInput('not-a-recovery.json', '{}');
  await change(0, invalid);
  assert.equal(pickerSlots[4], undefined, 'failed original selection cannot reuse the old file');
  assert.equal(pickerSlots[5], '');
  assert.equal(invalid.value, '');
  assert.ok(buttons({ cachedSlots: pickerSlots }).find(n => label(n) === '从恢复文件重新读取核对全部清单并完成').props.disabled);
  console.log('PASS: historical author and migration signer remain separate; persisted ciphertext retries exactly and completion reads the full manifest with an independent reader');
})().catch(error => { console.error(error); process.exitCode = 1; });
