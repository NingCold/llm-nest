/* Disposable real-CLI materials conversion, invoked by crypto_acceptance.cjs. */
const assert = require('node:assert/strict');
const { randomUUID } = require('node:crypto');
const fs = require('node:fs');
const path = require('node:path');
const { spawnSync } = require('node:child_process');
function nodeSnapshot(dir, ai, cid) {
  const code = `import sqlite3,json,sys
c=sqlite3.connect('file:'+sys.argv[1]+'?mode=ro',uri=True)
a,cid=sys.argv[2:]
out={}
for table,key in [('conscious_memory','memories'),('conscious_summaries','summaries')]:
 out[key]=[json.loads(r[0]) for r in c.execute('SELECT json FROM '+table+' WHERE ai=? AND cid=?',(a,cid))]
r=c.execute('SELECT json FROM conscious_settings WHERE ai=? AND cid=?',(a,cid)).fetchone()
out['settings']=json.loads(r[0]) if r else None
r=c.execute('SELECT json FROM profiles WHERE ai=?',(a,)).fetchone()
out['profile']=json.loads(r[0]) if r else None
out['resources']=[dict(id=r[0],revision=r[1],status=r[2]) for r in c.execute('SELECT r.id,r.revision,r.status FROM crypto_resources r JOIN crypto_resource_scopes s ON s.id=r.id WHERE s.ai=?',(a,))] if c.execute(\"SELECT count(*) FROM sqlite_master WHERE name='crypto_resource_scopes'\").fetchone()[0] else []
out['tasks']=c.execute('SELECT count(*) FROM tasks WHERE ai=?',(a,)).fetchone()[0]
print(json.dumps(out))`;
  const result = spawnSync('python', ['-c', code, path.join(dir, 'node.sqlite'), ai, cid], { encoding: 'utf8', windowsHide: true });
  assert.equal(result.status, 0, 'read-only node material inspection failed');
  return JSON.parse(result.stdout);
}
exports.seed = async function seed(c) {
  const { human, aiId, nodeDir, step, http, directory } = c;
  const group = await http(human, '/chat/groups', 'POST', { requestId: randomUUID(), name: '节点旧资料迁移验收', memberIds: [aiId] });
  const source = await http(human, `/chat/conversations/${group.id}/messages`, 'POST', { requestId: randomUUID(), text: '旧群资料来源，未点名 AI', replyTo: null, mentions: [] });
  const now = new Date().toISOString();
  const memoryId = randomUUID(), forgottenId = randomUUID(), summaryId = randomUUID();
  const sources = [{ messageId: source.id, revision: source.revision }];
  const binding = { providerId: 'fixture', modelId: 'fixture', reasoningEffort: null, maxTokens: 256 };
  const memory = (id, status, text) => ({ id, revision: '1', kind: 'fact', status, text, source: sources[0], createdAt: now, updatedAt: now });
  const summary = { id: summaryId, revision: '1', status: 'ready', text: '旧节点有效窗口摘要', fromPosition: source.position, throughPosition: source.position, sources, error: null, createdAt: now, updatedAt: now };
  const bundle = { version: 1, packageId: randomUUID(), origin: 'node', aiId, profile: { persona: '旧节点保留角色', providerId: 'fixture', modelId: 'fixture', reasoningEffort: null, temperature: null, maxTokens: 256, autoDailyTokens: null }, windows: [{ conversationId: group.id, memories: [memory(memoryId, 'confirmed', '旧节点有效记忆'), memory(forgottenId, 'forgotten', '遗忘正文不得恢复')], settings: { revision: '1', enabled: false, binding }, summaries: [{ item: summary, binding, requestedBy: human.account }] }] };
  const file = path.join(directory, 'legacy-node-materials.private.json');
  fs.writeFileSync(file, JSON.stringify(bundle));
  const preview = JSON.parse(step('preview-import-ai', nodeDir, aiId, file));
  assert.equal(preview.type, 'importPreview');
  const imported = JSON.parse(step('import-ai', nodeDir, aiId, file, preview.expectedProfileRevision));
  assert.equal(imported.type, 'imported');
  const snapshot = nodeSnapshot(nodeDir, aiId, group.id);
  assert.equal(snapshot.profile.persona, bundle.profile.persona);
  assert.equal(snapshot.memories.find(m => m.id === forgottenId).text, '');
  assert.equal(snapshot.summaries.find(s => s.item.id === summaryId).item.status, 'ready');
  assert.equal(snapshot.tasks, 0, 'import must not fabricate model executions');
  return { group, source, memoryId, forgottenId, summaryId, persona: bundle.profile.persona, memoryText: '旧节点有效记忆', summaryText: summary.text };
};
exports.run = async function run(c) {
  const { human, aiId, nodeDir, step, http, sql, modelInputs, waitFor, startNode, stopNode, pairNode, recoveryPath, peer, trust, share, identity, fixture } = c;
  const baseline = modelInputs.length;
  let running = c.running;
  await stopNode(running);
  const localEdit = { ...nodeSnapshot(nodeDir, aiId, fixture.group.id).profile, revision: randomUUID(), persona: '本机未同步角色编辑保留直至确认' };
  const editPath = path.join(nodeDir, 'local-profile-edit.private.json');
  fs.writeFileSync(editPath, JSON.stringify(localEdit));
  step('profile', nodeDir, aiId, editPath);
  const preview = JSON.parse(step('crypto-migration-preview', nodeDir, aiId));
  assert.equal(preview.profileAction, 'applyCloudAuthoritative');
  assert.equal(nodeSnapshot(nodeDir, aiId, fixture.group.id).profile.persona, localEdit.persona, 'preview must preserve the user local profile edit');
  assert.equal(preview.type, 'materialMigrationPreview');
  assert(preview.items.some(i => i.id === fixture.memoryId));
  assert(preview.items.some(i => i.id === fixture.forgottenId && i.tombstone === 'forgotten'));
  assert(preview.items.some(i => i.id === fixture.summaryId));
  assert(preview.items.some(i => i.kind === 'summarySettings'));
  const converted = JSON.parse(step('crypto-migrate', nodeDir, aiId, preview.digest));
  assert.equal(converted.type, 'materialMigrationComplete');
  const beforeRetry = sql(`SELECT item_id||':'||revision||':'||coalesce(tombstone,'') FROM cloud_crypto_resources WHERE ai_id='${aiId}' ORDER BY item_id`);
  const replay = JSON.parse(step('crypto-migrate', nodeDir, aiId, preview.digest));
  assert.equal(replay.alreadyComplete, true);
  assert.equal(sql(`SELECT item_id||':'||revision||':'||coalesce(tombstone,'') FROM cloud_crypto_resources WHERE ai_id='${aiId}' ORDER BY item_id`), beforeRetry);
  const items = [];
  for (const kind of ['memory', 'summary', 'summarySettings']) {
    const list = await http(human, `/crypto/resources?aiId=${aiId}&conversationId=${fixture.group.id}&kind=${kind}`);
    for (const item of list.items) { assert.equal(item.applied?.status, 'applied'); assert.equal(item.applied?.revision, item.revision); items.push(item); }
  }
  assert.equal(items.find(i => i.id === fixture.forgottenId).tombstone, 'forgotten');
  assert.equal(items.find(i => i.id === fixture.forgottenId).sealed, null);
  assert(items.find(i => i.id === fixture.memoryId).sealed);
  assert(items.find(i => i.id === fixture.summaryId).sealed);
  const stored = sql(`SELECT coalesce(sealed::text,'') FROM cloud_crypto_resources WHERE ai_id='${aiId}'`);
  for (const text of [fixture.persona, fixture.memoryText, fixture.summaryText, '遗忘正文不得恢复']) assert(!stored.includes(text));
  assert.equal(modelInputs.length, baseline, 'material conversion/retry must not open any model request');
  running = startNode(nodeDir);
  const managed = () => http(human, '/ai');
  await waitFor(managed, rows => rows.find(row => row.account.id === aiId)?.node?.online, 'material migration node reconnect');
  await share(human, peer, fixture.group.id);
  await http(human, `/chat/conversations/${fixture.group.id}/encryption`, 'POST', { deviceId: human.device, requestId: randomUUID(), expectedConversationRevision: fixture.group.revision });
  const current = (await managed()).find(row => row.account.id === aiId);
  await http(human, `/ai/${aiId}/node/handover`, 'POST', { requestId: randomUUID(), expectedBindingVersion: current.bindingVersion });
  await waitFor(managed, rows => rows.find(row => row.account.id === aiId)?.node?.handoverState === 'ready', 'converted materials handover readiness');
  await stopNode(running);
  const replacement = await pairNode('node-material-migration-recovery', current.bindingVersion);
  step('crypto-recover', replacement, aiId, recoveryPath);
  const restored = JSON.parse(step('crypto-status', replacement, aiId));
  assert.notEqual(restored.deviceId, identity.deviceId);
  assert.equal(Object.values(restored.master.keys)[0], peer.master);
  peer.device = restored.deviceId;
  await trust(human, peer);
  running = startNode(replacement);
  await waitFor(managed, rows => rows.find(row => row.account.id === aiId)?.node?.online, 'material recovery node reconnect');
  const recovered = await waitFor(() => Promise.resolve(nodeSnapshot(replacement, aiId, fixture.group.id)), s => s.memories.some(m => m.id === fixture.memoryId) && s.summaries.some(s => s.item.id === fixture.summaryId), 'recovered old node materials');
  assert.equal(recovered.profile.persona, fixture.persona);
  assert.equal(recovered.memories.find(m => m.id === fixture.memoryId).text, fixture.memoryText);
  assert(!recovered.memories.some(m => m.id === fixture.forgottenId && m.text), 'forgotten material must not resurrect');
  assert.equal(recovered.resources.find(r => r.id === fixture.forgottenId)?.status, 'forgotten', 'fresh node must retain the authoritative tombstone');
  assert.equal(recovered.summaries.find(s => s.item.id === fixture.summaryId).item.text, fixture.summaryText);
  assert.equal(recovered.settings.enabled, false);
  assert(recovered.settings.binding);
  assert.equal(recovered.tasks, 0);
  assert.equal(modelInputs.length, baseline, 'handover/recovery must not re-run original calls');
  console.log('PASS: real CLI legacy materials import, explicit ciphertext migration, applied receipts, tombstones, idempotent conversion, normal handover and fresh-node recovery without model calls.');
  return { running, nodeDir: replacement, fixture };
};
