/* Real official WASM SDK against an isolated cloud server and PostgreSQL.
 * Uses disposable invited accounts and a local mock model. Private artifacts stay
 * in target; stdout reports assertions, never recovery material or credentials.
 * Requires CLOUD_TEST_DATABASE_URL, built cloud-server/llmn-agent/interop, and psql on PATH.
 */
const { spawn, spawnSync } = require('node:child_process');
const { createRequire } = require('node:module');
const { randomUUID } = require('node:crypto');
const fs = require('node:fs');
const path = require('node:path');
const assert = require('node:assert/strict');
const { createServer } = require('node:http');
const { connect } = require('node:net');
const root = path.resolve(__dirname, '..');
const webRequire = createRequire(path.join(root, 'client/web/package.json'));
const sdk = webRequire('@matrix-org/matrix-sdk-crypto-wasm');
const { ed25519 } = webRequire('@noble/curves/ed25519.js');
const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));
const directory = path.join(root, 'target', `crypto-acceptance-${randomUUID()}`);
fs.mkdirSync(directory, { recursive: true });
const origin = 'http://127.0.0.1:18789';
const executable = path.join(root, 'target/debug', process.platform === 'win32' ? 'cloud-server.exe' : 'cloud-server');
const database = `crypto_acceptance_${randomUUID().replaceAll('-', '')}`;
const base = new URL(process.env.CLOUD_TEST_DATABASE_URL);
const sqlEnv = { ...process.env, PGHOST: base.hostname, PGPORT: base.port || '5432', PGUSER: decodeURIComponent(base.username), PGPASSWORD: decodeURIComponent(base.password), PGDATABASE: base.pathname.slice(1) };
function sql(statement, db = database) {
  const p = spawnSync('psql', ['-X', '-v', 'ON_ERROR_STOP=1', '-At', '-c', statement], { env: { ...sqlEnv, PGDATABASE: db }, encoding: 'utf8', windowsHide: true });
  if (p.status !== 0) throw new Error(`test SQL failed: ${p.stderr}`);
  return p.stdout.trim();
}
sql(`CREATE DATABASE "${database}"`, sqlEnv.PGDATABASE);
base.pathname = `/${database}`;
const env = { ...process.env, CLOUD_DATABASE_URL: base.toString(), CLOUD_PUBLIC_URL: origin, CLOUD_BIND: '127.0.0.1:18789', CLOUD_ATTACHMENTS_DIR: path.join(directory, 'attachments'), CLOUD_MAIL_OUTBOX: path.join(directory, 'mail'), CLOUD_WEB_DIR: path.join(root, 'client/web/dist') };
const log = fs.openSync(path.join(directory, 'server.log'), 'w');
let server = spawn(executable, [], { env, windowsHide: true, stdio: ['ignore', log, log] });
const children = [];
let modelServer;
let phase = 'startup';
async function http(session, route, method = 'GET', body, expected = 200) {
  const response = await fetch(origin + '/api' + route, { method, headers: { Cookie: session.cookie || '', Origin: origin, 'X-LLMN-Client': '1', ...(body === undefined ? {} : { 'Content-Type': 'application/json' }) }, body: body === undefined ? undefined : JSON.stringify(body) });
  const c = response.headers.get('set-cookie');
  if (c) session.cookie = c.split(';')[0];
  const text = await response.text();
  assert.equal(response.status, expected, `${method} ${route}: ${text.slice(0, 200)}`);
  return text ? JSON.parse(text) : null;
}
const userId = id => `@${id}:llmn`;
const roomId = id => `!${id}:llmn`;
async function signup(name) {
  const p = spawnSync(executable, ['invite'], { env, encoding: 'utf8', windowsHide: true });
  assert.equal(p.status, 0, 'invite command failed');
  const session = { cookie: '' }, email = `${randomUUID()}@example.invalid`;
  await http(session, '/auth/register', 'POST', { invite: p.stdout.trim(), email, password: randomUUID(), name });
  const mail = fs.readdirSync(env.CLOUD_MAIL_OUTBOX).map(f => JSON.parse(fs.readFileSync(path.join(env.CLOUD_MAIL_OUTBOX, f), 'utf8'))).find(m => m.to === email);
  const token = mail.body.match(/token=([a-f0-9]+)/)[1];
  const verified = await http(session, '/auth/verify', 'POST', { token });
  session.account = verified.account.id;
  return session;
}
async function device(session) {
  const state = { ...session, device: randomUUID(), cursor: '0' };
  state.machine = await sdk.OlmMachine.initialize(new sdk.UserId(userId(state.account)), new sdk.DeviceId(state.device));
  await http(state, '/crypto/devices', 'POST', { deviceId: state.device, name: 'isolated acceptance' });
  await state.machine.updateTrackedUsers([new sdk.UserId(userId(state.account))]);
  await flush(state);
  return state;
}
async function outgoing(s, r) {
  const body = JSON.parse(r.body);
  if (r.type === sdk.RequestType.ToDevice) Object.assign(body, { event_type: r.event_type, txn_id: r.txn_id });
  if (r.type === sdk.RequestType.KeysBackup) body.version = r.version;
  const kind = ['keysUpload', 'keysQuery', 'keysClaim', 'toDevice', 'signatures', 'roomMessage', 'keysBackup'][r.type];
  const response = await http(s, '/crypto/requests', 'POST', { deviceId: s.device, requestId: r.id || randomUUID(), kind, body });
  if (r.id) await s.machine.markRequestAsSent(r.id, r.type, JSON.stringify(response));
}
async function flush(s) { for (const r of await s.machine.outgoingRequests()) await outgoing(s, r); }
async function query(s, accounts) {
  await s.machine.updateTrackedUsers(accounts.map(a => new sdk.UserId(userId(a))));
  await outgoing(s, new sdk.KeysQueryRequest(randomUUID(), JSON.stringify({ device_keys: Object.fromEntries(accounts.map(a => [userId(a), []])) })));
}
async function sync(s) {
  await flush(s);
  const value = await http(s, `/crypto/sync?deviceId=${s.device}&after=${s.cursor}`);
  const processed = await s.machine.receiveSyncChanges(JSON.stringify(value.to_device.events), new sdk.DeviceLists(value.device_lists.changed.map(id => new sdk.UserId(id)), value.device_lists.left.map(id => new sdk.UserId(id))), new Map(Object.entries(value.device_one_time_keys_count)), new Set(value.device_unused_fallback_key_types), new sdk.DecryptionSettings(sdk.TrustRequirement.CrossSigned));
  s.cursor = value.next_batch;
  await flush(s);
  return processed;
}
async function bootstrap(s) {
  const b = await s.machine.bootstrapCrossSigning(false);
  if (b.uploadKeysRequest) await outgoing(s, b.uploadKeysRequest);
  const body = JSON.parse(b.uploadSigningKeysRequest.body);
  s.master = Object.values(body.master_key.keys)[0];
  await http(s, '/crypto/requests', 'POST', { deviceId: s.device, requestId: randomUUID(), kind: 'signingKeys', body });
  await outgoing(s, b.uploadSignaturesRequest);
  await query(s, [s.account]);
}
async function trust(a, b) {
  await query(a, [b.account]);
  const identity = await a.machine.getIdentity(new sdk.UserId(userId(b.account)));
  assert.equal(Object.values(JSON.parse(identity.masterKey).keys)[0], b.master, 'independent fixture master mismatch');
  await outgoing(a, await identity.verify());
  await query(a, [b.account]);
  const d = await a.machine.getDevice(new sdk.UserId(userId(b.account)), new sdk.DeviceId(b.device));
  assert.equal(d.isVerified(), true);
  assert.equal(d.isCrossSignedByOwner(), true);
}
async function share(a, b, cid) {
  await query(a, [a.account, b.account]);
  const members = [a, b].map(s => new sdk.UserId(userId(s.account)));
  const claim = await a.machine.getMissingSessions(members);
  if (claim) await outgoing(a, claim);
  const settings = new sdk.EncryptionSettings();
  settings.algorithm = sdk.EncryptionAlgorithm.MegolmV1AesSha2;
  settings.sharingStrategy = sdk.CollectStrategy.onlyTrustedDevices();
  const room = new sdk.RoomId(cid.startsWith('!') ? cid : roomId(cid));
  await a.machine.invalidateGroupSession(room);
  for (const r of await a.machine.shareRoomKey(room, [a, b].map(s => new sdk.UserId(userId(s.account))), settings)) await outgoing(a, r);
  if (b.machine) await sync(b);
}
async function decrypt(s, envelope) {
  const identity = await s.machine.getIdentity(new sdk.UserId(envelope.sender));
  assert(identity?.isVerified(), 'sender master must be independently verified');
  const master = Object.values(JSON.parse(identity.masterKey).keys)[0];
  const signed = canonical(Object.fromEntries(Object.entries(envelope).filter(([key]) => key !== 'signatures')));
  const signatures = envelope.signatures[envelope.sender];
  const verify = (key, signature) => ed25519.verify(Buffer.from(signature, 'base64'), Buffer.from(signed), Buffer.from(key, 'base64'), { zip215: false });
  assert(verify(master, signatures[`ed25519:${master}`]), 'master signature');
  assert(verify(envelope.deviceKeys.ed25519, signatures[`ed25519:${envelope.deviceId}`]), 'device signature');
  assert.equal(envelope.deviceKeys.curve25519, envelope.content.sender_key);
  assert.equal(envelope.deviceId, envelope.content.device_id);
  const event = { type: 'm.room.encrypted', sender: envelope.sender, event_id: envelope.eventId, origin_server_ts: 0, content: envelope.content };
  const result = await s.machine.decryptRoomEvent(JSON.stringify(event), new sdk.RoomId(envelope.roomId), new sdk.DecryptionSettings(sdk.TrustRequirement.Untrusted));
  return JSON.parse(result.event).content;
}
function canonical(value) {
  if (value === null || typeof value !== 'object') return JSON.stringify(value);
  if (Array.isArray(value)) return `[${value.map(canonical).join(',')}]`;
  return `{${Object.keys(value).sort().map(key => `${JSON.stringify(key)}:${canonical(value[key])}`).join(',')}}`;
}
async function sign(s, envelope) {
  envelope.deviceKeys = { ed25519: s.machine.identityKeys.ed25519.toBase64(), curve25519: s.machine.identityKeys.curve25519.toBase64() };
  envelope.signatures = JSON.parse((await s.machine.sign(canonical(envelope))).asJSON());
}
async function waitFor(read, accept, description) {
  for (let i = 0; i < 100; i++) {
    const value = await read();
    if (accept(value)) return value;
    await sleep(250);
  }
  throw new Error(`Timed out: ${description}`);
}
async function agentFlow(human) {
  phase = 'Agent enrollment and recovery setup';
  const agent = path.join(root, 'target/debug', process.platform === 'win32' ? 'llmn-agent.exe' : 'llmn-agent');
  const agentEnv = { ...env, LLMN_E2EE_TEST_KEY: 'mock-provider-only' };
  const modelInputs = [];
  modelServer = createServer(async (req, res) => {
    const chunks = []; for await (const chunk of req) chunks.push(chunk);
    const input = JSON.parse(Buffer.concat(chunks));
    assert.equal(req.url, '/v1/chat/completions'); assert.equal(input.model, 'fixture');
    modelInputs.push(input);
    res.writeHead(200, { 'Content-Type': 'text/event-stream' });
    const emit = data => res.write(`data: ${JSON.stringify({ id: 'fixture', ...data })}\n\n`);
    const isSummary = input.messages[0].content.includes('你负责生成窗口分段摘要');
    emit({ choices: [{ index: 0, delta: { content: isSummary ? '加密窗口摘要' : '节点密文' }, finish_reason: null }] });
    await sleep(700);
    emit({ choices: [{ index: 0, delta: { content: `回复-${modelInputs.length}` }, finish_reason: null }] });
    emit({ choices: [{ index: 0, delta: {}, finish_reason: 'stop' }], usage: { prompt_tokens: 20, completion_tokens: 8, total_tokens: 28 } });
    res.end('data: [DONE]\n\n');
  });
  await new Promise(resolve => modelServer.listen(0, '127.0.0.1', resolve));
  const modelPort = modelServer.address().port;
  const step = (verb, dir, ...args) => {
    const result = spawnSync(agent, [verb, dir, ...args], { env: agentEnv, windowsHide: true, encoding: 'utf8', timeout: 90000 });
    assert.equal(result.status, 0, `Agent ${verb} failed: ${result.stderr}`);
    return result.stdout.trim();
  };
  const ai = await http(human, '/ai', 'POST', { requestId: randomUUID(), name: '加密节点验收', bio: 'isolated mock provider' });
  const aiId = ai.account.id;
  async function pairNode(name, bindingVersion) {
    const dir = path.join(directory, name);
    step('init', dir);
    fs.writeFileSync(path.join(dir, 'node.toml'), `server = "${origin}"
node_id = "${randomUUID()}"
node_name = "加密模拟节点"
daily_calls = 24
daily_tokens = 1048576
[profile]
revision = "1"
persona = "本机验收角色"
max_tokens = 256
[profile.selection]
provider = "fixture"
model = "fixture"
[providers.fixture]
protocol = "openai"
base_url = "http://127.0.0.1:${modelPort}/v1"
api_key = { env = "LLMN_E2EE_TEST_KEY" }
[providers.fixture.models.fixture]
model = "fixture"
context_window = 8192
max_tokens = 512
`);
    const pairing = spawn(agent, ['pair-json', dir], { env: agentEnv, windowsHide: true, stdio: ['ignore', 'pipe', 'pipe'] });
    children.push(pairing);
    let output = ''; pairing.stdout.on('data', chunk => output += chunk);
    const event = await waitFor(async () => output.split('\n').filter(Boolean).map(line => JSON.parse(line)).find(item => item.type === 'pairing'), Boolean, 'node pairing code');
    await http(human, `/ai/${aiId}/node`, 'POST', { requestId: randomUUID(), pairingCode: event.userCode, expectedBindingVersion: bindingVersion });
    await waitFor(async () => pairing.exitCode, code => code !== null, 'paired node exit');
    assert.equal(pairing.exitCode, 0);
    return dir;
  }
  function startNode(dir) {
    const fd = fs.openSync(path.join(dir, 'test.log'), 'a');
    const child = spawn(agent, ['run-managed', dir], { env: agentEnv, windowsHide: true, stdio: ['ignore', fd, fd] });
    fs.closeSync(fd); children.push(child); return child;
  }
  async function stopNode(child) { child.kill(); await waitFor(async () => child.exitCode ?? child.signalCode, code => code !== null, 'node exit'); }
  const nodeDir = await pairNode('node-first', ai.bindingVersion);
  let rpcDirectory = nodeDir;
  const identity = JSON.parse(step('crypto-init', nodeDir, aiId));
  const recoveryPath = path.join(directory, 'ai-recovery.private.json');
  step('crypto-export-recovery', nodeDir, aiId, recoveryPath);
  step('crypto-trust', nodeDir, aiId, human.account, human.device, human.machine.identityKeys.ed25519.toBase64(), human.machine.identityKeys.curve25519.toBase64(), human.master);
  const peer = { account: aiId, device: identity.deviceId, master: Object.values(identity.master.keys)[0] };
  await trust(human, peer);
  step('crypto-authorize-admin', nodeDir, aiId, human.account, human.device);
  let running = startNode(nodeDir);
  const managed = () => http(human, '/ai');
  await waitFor(managed, rows => rows.find(row => row.account.id === aiId)?.node?.online, 'encrypted node online');
  const cid = (await http(human, '/chat/direct', 'POST', { accountId: aiId })).id;
  const messagePath = `/chat/conversations/${cid}/messages`;
  await share(human, peer, cid);
  const direct = (await http(human, '/chat/init')).conversations.find(c => c.id === cid);
  const encryptedDirect = await http(human, `/chat/conversations/${cid}/encryption`, 'POST', { deviceId: human.device, requestId: randomUUID(), expectedConversationRevision: direct.revision });
  async function send(text) {
    await share(human, peer, cid);
    const id = randomUUID();
    const inner = { version: 1, server: origin, kind: 'message', conversationId: cid, conversationRevision: encryptedDirect.revision, messageId: id, senderId: human.account, revision: '1', text, replyTo: null, mentions: [], runId: null, sequence: null };
    const sealed = { version: 1, roomId: roomId(cid), sender: userId(human.account), deviceId: human.device, eventId: `$${id}.1.final`, content: JSON.parse(await human.machine.encryptRoomEvent(new sdk.RoomId(roomId(cid)), 'com.llmn.content.v1', JSON.stringify(inner))) };
    await sign(human, sealed);
    const command = { requestId: randomUUID(), deviceId: human.device, messageId: id, text: '', sealed, expectedConversationRevision: encryptedDirect.revision };
    await http(human, messagePath, 'POST', command);
    await http(human, messagePath, 'POST', command);
    const page = await waitFor(() => http(human, messagePath), page => {
      const response = page.messages.find(message => message.id === id)?.aiResponses?.find(response => response.aiId === aiId);
      assert(!response || ['queued', 'running', 'completed'].includes(response.status), `AI task failed: ${JSON.stringify(response)}`);
      return response?.status === 'completed';
    }, 'sealed AI reply');
    await sync(human);
    const reply = page.messages.filter(message => message.senderId === aiId).at(-1);
    assert.equal(reply.text, '');
    assert.match((await decrypt(human, reply.sealed)).text, /^节点密文回复-/);
    assert.equal(sql(`SELECT count(*) FROM cloud_messages WHERE id='${id}'`), '1');
    return id;
  }
  if (process.argv.includes('--social-only')) {
    phase = 'encrypted social Agent and offline attachments';
    await socialScenarios(nodeDir);
    await stopNode(running);
    return modelInputs.length;
  }
  phase = 'real Agent encrypted generation';
  const firstMessage = await send('真实节点加密输入');
  assert.equal(modelInputs.length, 1); assert(JSON.stringify(modelInputs[0]).includes('真实节点加密输入'));
  assert(!sql(`SELECT string_agg(body||COALESCE(sealed::text,''),'') FROM cloud_messages WHERE conversation='${cid}'`).includes('节点密文回复'));
  phase = 'AI ciphertext resources';
  const resourceList = (kind, conversationId) => http(human, `/crypto/resources?aiId=${aiId}&kind=${kind}${conversationId ? `&conversationId=${conversationId}` : ''}`);
  async function resource(kind, conversationId, id, expectedRevision, data, sources = [], tombstone = null) {
    const audience = await http(human, `/crypto/resource-participants?aiId=${aiId}&kind=${kind}${conversationId ? `&conversationId=${conversationId}` : ''}`);
    const scope = `${aiId}.${kind}.${conversationId ?? 'global'}`, room = `!${scope}:llmn`, revision = String(Number(expectedRevision) + 1);
    let sealed = null;
    if (!tombstone) {
      await share(human, peer, room);
      const inner = { version: 1, server: origin, kind: 'resource', conversationId, messageId: id, senderId: human.account, revision, text: '', replyTo: null, mentions: [], runId: null, sequence: null, scope, id, expectedRevision, expectedAudienceRevision: audience.revision, data };
      sealed = { version: 1, roomId: room, sender: userId(human.account), deviceId: human.device, eventId: `$${id}.${revision}.final`, content: JSON.parse(await human.machine.encryptRoomEvent(new sdk.RoomId(room), 'com.llmn.content.v1', JSON.stringify(inner))) };
      await sign(human, sealed);
    }
    return http(human, `/crypto/resources/${id}`, 'PUT', { requestId: randomUUID(), deviceId: human.device, aiId, conversationId, kind, expectedRevision, expectedAudienceRevision: audience.revision, sealed, sources, tombstone });
  }
  await sync(human);
  const profile = (await resourceList('profile', null)).items[0]; assert(profile);
  const profileData = (await decrypt(human, profile.sealed)).data;
  await stopNode(running);
  const changedProfile = await resource('profile', null, profile.id, profile.revision, { ...profileData, revision: String(Number(profile.revision) + 1), persona: '密文更新角色' });
  assert.notEqual(changedProfile.applied?.revision, changedProfile.revision, 'offline node cannot have applied a new profile');
  const memoryId = randomUUID(), now = new Date().toISOString();
  const memory = await resource('memory', cid, memoryId, '0', { id: memoryId, revision: '1', kind: 'fact', text: '密文记忆：喜欢蓝色', status: 'confirmed', source: { messageId: firstMessage, revision: '1' }, createdAt: now, updatedAt: now }, [{ messageId: firstMessage, revision: '1' }]);
  assert.equal(memory.applied, null);
  running = startNode(nodeDir);
  await waitFor(() => resourceList('memory', cid), r => r.items[0]?.applied?.status === 'applied', 'node memory application');
  await send('读取新角色与记忆');
  assert.equal(modelInputs.length, 2);
  assert(JSON.stringify(modelInputs[1]).includes('密文更新角色')); assert(JSON.stringify(modelInputs[1]).includes('喜欢蓝色'));
  await resource('memory', cid, memoryId, '1', null, memory.sources, 'forgotten');
  await waitFor(() => resourceList('memory', cid), r => r.items[0]?.tombstone === 'forgotten' && r.items[0]?.applied?.revision === '2', 'node forgetting');
  phase = 'offline sealed native IPC';
  const bound = (await managed()).find(row => row.account.id === aiId);
  const grant = await http(human, `/chat/conversations/${cid}/direct`, 'POST', { nodeId: bound.node.id, bindingVersion: bound.bindingVersion });
  function rpc(operation) {
    const endpoint = JSON.parse(fs.readFileSync(path.join(rpcDirectory, 'direct-endpoint.json'), 'utf8'));
    return new Promise((resolve, reject) => {
      const socket = connect(endpoint.name), chunks = [];
      socket.setTimeout(15000, () => socket.destroy(new Error('IPC timeout')));
      socket.once('error', reject);
      socket.once('connect', () => {
        const body = Buffer.from(JSON.stringify({ token: endpoint.token, request: { ownerId: human.account, operation } }));
        const header = Buffer.alloc(4); header.writeUInt32BE(body.length);
        socket.write(Buffer.concat([header, body]));
      });
      socket.on('data', chunk => {
        chunks.push(chunk); const bytes = Buffer.concat(chunks);
        if (bytes.length >= 4 && bytes.length >= 4 + bytes.readUInt32BE()) {
          socket.end(); const result = JSON.parse(bytes.subarray(4, 4 + bytes.readUInt32BE()).toString());
          if (result.type === 'error') reject(new Error(`IPC: ${result.error.code} ${result.error.message}`)); else resolve(result.type === 'view' ? result.view : result);
        }
      });
    });
  }
  await rpc({ type: 'prepare', aiId, grant: grant.token });
  await stopNode(server);
  await human.machine.invalidateGroupSession(new sdk.RoomId(roomId(cid)));
  const settings = new sdk.EncryptionSettings(); settings.algorithm = sdk.EncryptionAlgorithm.MegolmV1AesSha2; settings.sharingStrategy = sdk.CollectStrategy.onlyTrustedDevices();
  assert(!(await human.machine.getMissingSessions([human.account, aiId].map(id => new sdk.UserId(userId(id))))), 'Olm sessions must be prepared before disconnection');
  const localRequests = await human.machine.shareRoomKey(new sdk.RoomId(roomId(cid)), [human.account, aiId].map(id => new sdk.UserId(userId(id))), settings);
  const keyMessages = localRequests.map(r => ({ ...JSON.parse(r.body), event_type: r.event_type, txn_id: r.txn_id }));
  const localId = randomUUID(), localInner = { version: 1, server: origin, kind: 'message', conversationId: cid, conversationRevision: encryptedDirect.revision, messageId: localId, senderId: human.account, revision: '1', text: '断开云端后的密文输入', replyTo: null, mentions: [], runId: null, sequence: null };
  const localSealed = { version: 1, roomId: roomId(cid), sender: userId(human.account), deviceId: human.device, eventId: `$${localId}.1.final`, content: JSON.parse(await human.machine.encryptRoomEvent(new sdk.RoomId(roomId(cid)), 'com.llmn.content.v1', JSON.stringify(localInner))) }; await sign(human, localSealed);
  const localCommand = { type: 'send', conversationId: cid, requestId: randomUUID(), messageId: localId, text: '', sealed: localSealed, keyMessages, expectedConversationRevision: encryptedDirect.revision };
  await rpc(localCommand); await rpc(localCommand);
  const local = await waitFor(() => rpc({ type: 'view', conversationId: cid }), view => !view.running && view.messages.some(m => m.local && m.message.senderId === aiId && m.status === 'committed'), 'offline encrypted response');
  const incoming = (local.keyMessages ?? []).flatMap(body => { const content = body.messages[userId(human.account)]?.[human.device]; return content ? [{ sender: userId(aiId), type: body.event_type, content }] : []; });
  assert(incoming.length, 'AI must deliver new Olm room keys through local IPC');
  await human.machine.receiveSyncChanges(JSON.stringify(incoming), new sdk.DeviceLists([], []), new Map(), new Set(), new sdk.DecryptionSettings(sdk.TrustRequirement.CrossSigned));
  const localAnswer = local.messages.filter(m => m.local && m.message.senderId === aiId).at(-1);
  assert.equal(localAnswer.message.text, ''); assert.match((await decrypt(human, localAnswer.message.sealed)).text, /^节点密文回复-/);
  assert.equal(modelInputs.length, 3);
  server = spawn(executable, [], { env, windowsHide: true, stdio: ['ignore', log, log] });
  await waitFor(async () => { try { return (await fetch(origin + '/health')).ok; } catch { return false; } }, Boolean, 'cloud restart');
  for (const request of localRequests) await outgoing(human, request);
  await rpc({ type: 'sync', conversationId: cid });
  await waitFor(() => http(human, messagePath), page => page.messages.some(m => m.id === localAnswer.message.id), 'original offline result synchronization');
  assert.equal(modelInputs.length, 3, 'synchronization must not invoke the model again');
  assert.equal(sql(`SELECT count(*) FROM cloud_messages WHERE id='${localId}'`), '1');
  assert.equal(sql(`SELECT count(*) FROM cloud_node_tasks WHERE input_id='${localId}'`), '0');
  phase = 'encrypted window summary';
  for (let i = 0; i < 11; i++) await send(`摘要原文-${i}`);
  const settingsId = randomUUID();
  await resource('summarySettings', cid, settingsId, '0', { revision: '1', enabled: true, binding: { providerId: 'fixture', modelId: 'fixture', reasoningEffort: null, maxTokens: 256 } });
  const summary = (await waitFor(() => resourceList('summary', cid), page => page.items.some(item => item.sealed), 'encrypted summary publication')).items.find(item => item.sealed);
  await sync(human);
  const summaryData = (await decrypt(human, summary.sealed)).data;
  assert.match(summaryData.item.text, /^加密窗口摘要/);
  assert.equal(modelInputs.length, 15, 'fourteen chats and one summary should run once each');
  assert(JSON.stringify(modelInputs[14]).includes('真实节点加密输入'), 'summary provider must receive decrypted original messages');
  assert(!sql(`SELECT sealed::text FROM cloud_crypto_resources WHERE item_id='${summary.id}'`).includes('加密窗口摘要'));
  await resource('summarySettings', cid, settingsId, '1', { revision: '2', enabled: false, binding: { providerId: 'fixture', modelId: 'fixture', reasoningEffort: null, maxTokens: 256 } });
  phase = 'normal encrypted node handover';
  const before = (await managed()).find(row => row.account.id === aiId);
  await http(human, `/ai/${aiId}/node/handover`, 'POST', { requestId: randomUUID(), expectedBindingVersion: before.bindingVersion });
  await waitFor(managed, rows => rows.find(row => row.account.id === aiId)?.node?.handoverState === 'ready', 'node handover ready');
  await stopNode(running);
  const replacement = await pairNode('node-replacement', before.bindingVersion);
  // A smaller model window makes the restored summary necessary; when all
  // originals fit, the existing consciousness policy correctly prefers them.
  const replacementConfig = path.join(replacement, 'node.toml');
  fs.writeFileSync(replacementConfig, fs.readFileSync(replacementConfig, 'utf8').replace('context_window = 8192', 'context_window = 2048'));
  step('crypto-recover', replacement, aiId, recoveryPath);
  const recoveredIdentity = JSON.parse(step('crypto-status', replacement, aiId));
  assert.notEqual(recoveredIdentity.deviceId, identity.deviceId);
  peer.device = recoveredIdentity.deviceId;
  assert.equal(Object.values(recoveredIdentity.master.keys)[0], peer.master);
  await trust(human, peer);
  running = startNode(replacement);
  await waitFor(managed, rows => rows.find(row => row.account.id === aiId)?.node?.online, 'recovered node online');
  await send('换节点后继续');
  assert.equal(modelInputs.length, 16, 'handover must not repeat an earlier model or summary call');
  assert(JSON.stringify(modelInputs[15]).includes('密文更新角色'));
  assert(!JSON.stringify(modelInputs[15]).includes('喜欢蓝色'), 'forgotten memory must not return after recovery');
  assert(JSON.stringify(modelInputs[15]).includes('加密窗口摘要'), 'new node must recover the encrypted window summary');
  const source = summary.sources.find(source => source.messageId === firstMessage); assert(source);
  await http(human, `/chat/messages/${firstMessage}/withdraw`, 'POST', { requestId: randomUUID(), expectedRevision: source.revision });
  const invalidated = (await waitFor(() => resourceList('summary', cid), page => page.items.find(item => item.id === summary.id)?.tombstone === 'stale' && page.items.find(item => item.id === summary.id)?.applied?.revision === '2', 'summary source invalidation')).items.find(item => item.id === summary.id);
  assert.equal(invalidated.sealed, null);
  await send('撤回摘要来源后继续');
  assert.equal(modelInputs.length, 17);
  assert(!JSON.stringify(modelInputs[16]).includes('加密窗口摘要'), 'stale summary must not return to model context');
  // The tiny window above tests summary selection. Restore the ordinary fixture
  // capacity before testing required image/reference material in later scenarios.
  await stopNode(running);
  fs.writeFileSync(replacementConfig, fs.readFileSync(replacementConfig, 'utf8').replace('context_window = 2048', 'context_window = 8192'));
  running = startNode(replacement);
  await waitFor(managed, rows => rows.find(row => row.account.id === aiId)?.node?.online, 'social fixture node online');
  phase = 'encrypted social Agent and offline attachments';
  await socialScenarios(replacement);
  await stopNode(running);
  console.log('PASS: actual Agent pairing, signed streaming replies, ciphertext role/memory saved while node offline, application, forgetting, offline encrypted IPC with local Olm delivery, original-result synchronization, encrypted summary generation, normal handover, fresh node recovery, summary source invalidation, no repeated model calls.');
  return modelInputs.length;
  async function socialScenarios(nodeDirectory) {
    rpcDirectory = nodeDirectory;
    await require('./crypto_social_acceptance.cjs').agent({ human, peer, aiId, cid, http, sql, sdk, decrypt, sync, waitFor, modelInputs, rpc, origin, userId, roomId, sign, outgoing, query,
      stopCloud: () => stopNode(server),
      startCloud: async () => { server = spawn(executable, [], { env, windowsHide: true, stdio: ['ignore', log, log] }); await waitFor(async () => { try { return (await fetch(origin + '/health')).ok; } catch { return false; } }, Boolean, 'cloud restart'); },
      localShare: async () => {
        const room = new sdk.RoomId(roomId(cid));
        await human.machine.invalidateGroupSession(room);
        const settings = new sdk.EncryptionSettings(); settings.algorithm = sdk.EncryptionAlgorithm.MegolmV1AesSha2; settings.sharingStrategy = sdk.CollectStrategy.onlyTrustedDevices();
        return human.machine.shareRoomKey(room, [human.account, aiId].map(id => new sdk.UserId(userId(id))), settings);
      },
    });
  }
}
async function main() {
  await sdk.initAsync();
  for (let i = 0; i < 100; i++) {
    try { if ((await fetch(origin + '/health')).ok) break; } catch {}
    if (i === 99) throw new Error('isolated server did not start');
    await sleep(100);
  }
  const first = await signup('加密甲'), second = await signup('加密乙');
  phase = 'devices';
  const a = await device(first), b = await device(second);
  const conversation = await http(a, '/chat/direct', 'POST', { accountId: b.account });
  const cid = conversation.id;
  await bootstrap(a); await bootstrap(b);
  await trust(a, b); await trust(b, a);
  phase = 'Olm room-key sharing';
  await share(a, b, cid);
  const enabled = await http(a, `/chat/conversations/${cid}/encryption`, 'POST', { deviceId: a.device, requestId: randomUUID(), expectedConversationRevision: conversation.revision });
  phase = 'private message';
  const messageId = randomUUID(), marker = `只有端点可读-${randomUUID()}`;
  const inner = { version: 1, server: origin, kind: 'message', conversationId: cid, conversationRevision: enabled.revision, messageId, senderId: a.account, revision: '1', text: marker, replyTo: null, mentions: [], runId: null, sequence: null };
  const sealed = { version: 1, roomId: roomId(cid), sender: userId(a.account), deviceId: a.device, eventId: `$${messageId}.1.final`, content: JSON.parse(await a.machine.encryptRoomEvent(new sdk.RoomId(roomId(cid)), 'com.llmn.content.v1', JSON.stringify(inner))) };
  await sign(a, sealed);
  const command = { deviceId: a.device, requestId: randomUUID(), messageId, text: '', sealed, expectedConversationRevision: enabled.revision };
  const sent = await http(a, `/chat/conversations/${cid}/messages`, 'POST', command);
  assert.equal(sent.id, messageId);
  assert.equal((await http(a, `/chat/conversations/${cid}/messages`, 'POST', command)).id, messageId);
  const page = await http(b, `/chat/conversations/${cid}/messages`);
  assert.equal(page.messages.at(-1).text, '');
  assert.equal((await decrypt(b, page.messages.at(-1).sealed)).text, marker);
  for (const change of [e => e.roomId = roomId(randomUUID()), e => e.eventId += '-changed', e => e.deviceKeys.ed25519 = b.machine.identityKeys.ed25519.toBase64(), e => delete e.signatures]) {
    const tampered = structuredClone(sealed); change(tampered);
    await assert.rejects(() => decrypt(b, tampered));
  }
  const stored = sql(`SELECT body||COALESCE(sealed::text,'') FROM cloud_messages WHERE id='${messageId}'`);
  assert(!stored.includes(marker));
  await http(a, `/chat/conversations/${cid}/messages`, 'POST', { requestId: randomUUID(), text: marker }, 409);
  const untrusted = await device(second);
  await assert.rejects(() => decrypt(untrusted, sealed));
  phase = 'backup';
  const backupKey = sdk.BackupDecryptionKey.createRandomKey(), version = randomUUID();
  const auth_data = { public_key: backupKey.megolmV1PublicKey.publicKeyBase64 };
  auth_data.signatures = JSON.parse((await b.machine.sign(JSON.stringify(auth_data))).asJSON());
  await http(b, '/crypto/backup', 'PUT', { deviceId: b.device, expectedVersion: null, version, algorithm: 'm.megolm_backup.v1.curve25519-aes-sha2', auth_data });
  await b.machine.saveBackupDecryptionKey(backupKey, version);
  await b.machine.enableBackupV1(auth_data.public_key, version);
  const backup = await b.machine.backupRoomKeys(); assert(backup); await outgoing(b, backup);
  const bundle = await b.machine.exportSecretsBundle(), encoded = sdk.Attachment.encrypt(Buffer.from(JSON.stringify(bundle.to_json())));
  const recoveryVersion = randomUUID(), media = encoded.mediaEncryptionInfo;
  await http(b, '/crypto/recovery', 'PUT', { deviceId: b.device, expectedVersion: null, version: recoveryVersion, ciphertext: Buffer.from(encoded.encryptedData).toString('base64') });
  const recovered = await http(untrusted, '/crypto/recovery');
  phase = 'recovery';
  const plain = sdk.Attachment.decrypt(new sdk.EncryptedAttachment(Buffer.from(recovered.ciphertext, 'base64'), media));
  await untrusted.machine.importSecretsBundle(sdk.SecretsBundle.from_json(JSON.parse(Buffer.from(plain).toString())));
  const keys = await http(untrusted, '/crypto/backup/keys'), restored = new Map();
  for (const [room, data] of Object.entries(keys.rooms)) {
    const sessions = new Map();
    for (const [id, record] of Object.entries(data.sessions)) sessions.set(id, JSON.parse(backupKey.decryptV1(record.session_data.ephemeral, record.session_data.mac, record.session_data.ciphertext)));
    restored.set(new sdk.RoomId(room), sessions);
  }
  await untrusted.machine.importBackedUpRoomKeys(restored, () => {}, version);
  await query(untrusted, [a.account, b.account]);
  const knownSender = await untrusted.machine.getDevice(new sdk.UserId(userId(a.account)), new sdk.DeviceId(a.device));
  assert(knownSender, 'restored SDK must have the original sender device');
  assert(knownSender.isVerified(), 'restored SDK must verify the original sender cross-signing chain');
  assert.equal((await decrypt(untrusted, sealed)).text, marker);
  phase = 'WASM and Rust interop';
  await a.machine.saveBackupDecryptionKey(sdk.BackupDecryptionKey.createRandomKey(), 'interop');
  const rustRecovery = sdk.Attachment.encrypt(Buffer.from(JSON.stringify((await a.machine.exportSecretsBundle()).to_json())));
  const inputPath = path.join(directory, 'interop-input.private.json'), outputPath = path.join(directory, 'interop-output.private.json');
  fs.writeFileSync(inputPath, JSON.stringify({ account: a.account, mediaEncryptionInfo: rustRecovery.mediaEncryptionInfo, recoveryCiphertext: Array.from(rustRecovery.encryptedData), roomKeys: JSON.parse(await a.machine.exportRoomKeys(() => true)), sealed, expected: inner }));
  const interop = spawnSync(path.join(root, 'target/debug/examples', process.platform === 'win32' ? 'interop.exe' : 'interop'), [inputPath, outputPath], { windowsHide: true, encoding: 'utf8' });
  assert.equal(interop.status, 0, `Rust SDK interop failed: ${interop.stderr}`);
  const rust = JSON.parse(fs.readFileSync(outputPath, 'utf8'));
  await a.machine.importExportedRoomKeys(JSON.stringify(rust.roomKeys), () => {});
  assert.deepEqual(await decrypt(a, rust.sealed), inner);
  phase = 'encrypted groups and historical keys';
  await require('./crypto_social_acceptance.cjs').human({ a, b, http, sdk, signup, device, bootstrap, trust, sync, decrypt, sql, roomId, userId, origin, sign, outgoing, query });
  const modelCalls = await agentFlow(a);
  await http(b, `/crypto/devices/${untrusted.device}/revoke`, 'POST', {});
  await http(untrusted, `/crypto/sync?deviceId=${untrusted.device}&after=0`, 'GET', undefined, 403);
  for (const s of [a, b, untrusted]) s.machine.close();
  console.log('PASS: official SDK cross-signing, Olm distribution, signed private ciphertext, tamper rejection, idempotent retry, no plaintext in PostgreSQL, login alone cannot decrypt, recovery to a new device, WASM/Rust bidirectional interoperability, revoked device rejected.');
  fs.writeFileSync(path.join(directory, 'result.json'), JSON.stringify({ passed: true, mode: process.argv.includes('--social-only') ? 'social' : 'full', modelCalls, database, date: new Date().toISOString() }));
}
main().catch(error => { console.error(`FAIL ${phase}:`, error?.stack || error?.description || error?.message || String(error)); process.exitCode = 1; }).finally(() => { children.forEach(child => child.kill()); modelServer?.close(); server.kill(); fs.closeSync(log); });
