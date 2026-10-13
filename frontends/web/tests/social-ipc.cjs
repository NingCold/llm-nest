// Desktop transport contracts and asynchronous lifecycle, without a real provider.
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const assert = require('node:assert/strict');
const { webcrypto } = require('node:crypto');
const ts = require('typescript');
const root = path.resolve(__dirname, '../src/social');
const compile = name => ts.transpileModule(fs.readFileSync(path.join(root, name + '.ts'), 'utf8'), { compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.CommonJS } }).outputText;
const defer = () => { let resolve; const promise = new Promise(r => { resolve = r; }); return { promise, resolve }; };
const flush = () => new Promise(resolve => setImmediate(resolve));

function harness(start = Promise.resolve()) {
  const calls = [], channels = [];
  let failure = null;
  const transport = {};
  vm.runInNewContext(compile('transport'), { exports: transport, Error });
  const exports = {};
  vm.runInNewContext(compile('tauri'), {
    exports, crypto: webcrypto, Error,
    require: name => name === './transport' ? transport : {
      Channel: class { constructor(callback) { this.onmessage = callback; channels.push(this); } },
      invoke: async (name, args) => {
        calls.push({ name, args });
        if (failure) throw failure;
        if (name === 'social_subscribe') return start;
        return { ok: true };
      },
    },
  });
  return { ...exports, calls, channels, transport, fail: error => { failure = error; } };
}
const change = cursor => ({ kind: 'change', event: { cursor, change: { kind: 'actor_changed', actorId: 'ai', revision: '1' } } });
const deliver = (h, sequence, event) => h.channels.at(-1).onmessage({ kind: 'event', sequence, event });

(async () => {
  const h = harness();
  const inputs = { requestId: 'command', expectedRevision: '2', text: 'hello', actorId: 'ai', paused: true, action: 'pause' };
  const cases = [
    ['updateConversationList', ['chat', inputs], 'social_update_conversation_list', { id: 'chat', input: inputs }],
    ['searchMessages', [inputs], 'social_search_messages', { input: inputs }],
    ['messageContext', ['id'], 'social_message_context', { id: 'id' }],
    ['updateContactSettings', ['chat', inputs], 'social_update_contact_settings', { id: 'chat', input: inputs }],
    ['createContactIntent', ['chat', inputs], 'social_create_contact_intent', { id: 'chat', input: inputs }],
    ['cancelContactIntent', ['chat', 'intent', inputs], 'social_cancel_contact_intent', { id: 'chat', intent: 'intent', input: inputs }],
    ['updateParticipation', ['chat', inputs], 'social_update_participation', { id: 'chat', input: inputs }],
    ['referencePreview', [inputs], 'social_reference_preview', { input: inputs }],
    ['summaries', ['chat'], 'social_summaries', { id: 'chat', before: null }],
    ['summaries', ['chat', '90'], 'social_summaries', { id: 'chat', before: '90' }],
    ['configureSummaries', ['chat', inputs], 'social_configure_summaries', { id: 'chat', input: inputs }],
    ['requestSummary', ['chat', inputs], 'social_request_summary', { id: 'chat', input: inputs }],
    ['memories', ['chat', 'ai'], 'social_memories', { id: 'chat', actor: 'ai' }],
    ['createMemory', ['chat', 'ai', inputs], 'social_create_memory', { id: 'chat', actor: 'ai', input: inputs }],
    ['updateMemory', ['id', inputs], 'social_update_memory', { id: 'id', input: inputs }],
    ['forgetMemory', ['id', inputs], 'social_forget_memory', { id: 'id', input: inputs }],
    ['message', ['id'], 'social_message', { id: 'id' }],
    ['revise', ['id', inputs], 'social_revise_message', { id: 'id', input: inputs }],
    ['withdraw', ['id', inputs], 'social_withdraw_message', { id: 'id', input: inputs }],
    ['control', ['id', inputs], 'social_control_conversation', { id: 'id', input: inputs }],
    ['pauseMember', ['id', inputs], 'social_pause_member', { id: 'id', input: inputs }],
    ['createGroup', [inputs], 'social_create_group', { input: inputs }],
    ['changeMembership', ['id', inputs], 'social_change_membership', { id: 'id', input: inputs }],
    ['plans', ['id'], 'social_plans', { id: 'id' }],
    ['init', [], 'social_init', undefined],
    ['account', ['id'], 'social_account', { id:'id' }],
    ['updateAccount', ['id', inputs], 'social_update_account', { id:'id', input:inputs }],
    ['setContact', ['id', inputs], 'social_set_contact', { id:'id', input:inputs }],
    ['managedAccounts', [], 'social_managed_accounts', undefined],
    ['managedAccount', ['id'], 'social_managed_account', { id:'id' }],
    ['setAdministrator', ['id', inputs], 'social_set_administrator', { id:'id', input:inputs }],
    ['accountView', ['id'], 'social_account_view', { id:'id' }],
    ['accountViewMessages', ['id','chat'], 'social_account_view_messages', { id:'id', conversation:'chat', before:null }],
    ['createAi', [inputs], 'social_create_ai', { input: inputs }],
    ['updateAi', ['id', {...inputs,binding:{providerId:'test',modelId:'model',reasoningEffort:null,temperature:0,maxTokens:4096}}], 'social_update_ai', { id: 'id', input: {...inputs,binding:{providerId:'test',modelId:'model',reasoningEffort:null,temperature:0,maxTokens:4096}} }],
    ['controlAccount', ['id', { ...inputs, action: 'archive' }], 'social_control_account', { id: 'id', input: { ...inputs, action: 'archive' } }],
    ['openDirect', ['id', 'command'], 'social_open_direct', { id: 'id', input: { requestId: 'command' } }],
    ['messages', ['id'], 'social_messages', { id: 'id', before: null }],
    ['send', ['id', inputs], 'social_send', { id: 'id', input: inputs }],
    ['cancel', ['id'], 'social_cancel_run', { id: 'id' }],
    ['runDetails', ['id'], 'social_run_details', { id: 'id' }],
  ];
  for (const [method, args, name, expected] of cases) {
    await h.socialClient[method](...args);
    assert.equal(h.calls.at(-1).name, name);
    assert.equal(JSON.stringify(h.calls.at(-1).args), JSON.stringify(expected));
  }
  h.fail({ code: 'revision_conflict', message: 'revision changed', retryable: true });
  await assert.rejects(h.socialClient.message('id'), error => error.code === 'revision_conflict' && error.message === 'revision changed');

  const start = defer(), applied = defer(), ordered = [], controller = new AbortController();
  const stream = harness(start.promise);
  const running = stream.subscribe('9007199254740993', controller.signal, async event => {
    ordered.push(event.cursor); await applied.promise;
  }, event => ordered.push(event.text), async () => ordered.push('resync'), () => ordered.push('ready'));
  assert.equal(stream.calls[0].args.after, '9007199254740993');
  deliver(stream, 1, change('9007199254740994'));
  await flush();
  assert.deepEqual(ordered, ['9007199254740994'], 'listener exists before subscribe acknowledgement');
  assert.equal(stream.calls.filter(call => call.name === 'social_ack_subscription').length, 0, 'no ack until state applies');
  start.resolve(); await flush();
  applied.resolve(); await flush();
  assert.equal(stream.calls.at(-1).args.sequence, 1);
  deliver(stream, 2, { kind: 'generation', event: { type: 'generation.delta', text: '你好🙂', deltaSeq: 1 } });
  deliver(stream, 3, { kind: 'resync' }); await flush();
  assert.deepEqual(ordered, ['9007199254740994', 'ready', '你好🙂', 'resync']);
  assert.equal(stream.calls.at(-1).args.sequence, 3);
  controller.abort(); await running;
  assert.equal(stream.calls.at(-1).name, 'social_unsubscribe');
  assert.equal(stream.calls.at(-1).args.id, stream.calls[0].args.id);
  assert.ok(!stream.calls.some(call => call.name === 'social_cancel_run'));

  const pendingStart = defer(), pending = harness(pendingStart.promise), abort = new AbortController();
  const opening = pending.subscribe('7', abort.signal, async () => {}, () => {}, async () => {}, () => assert.fail('aborted subscription cannot become ready'));
  abort.abort(); pendingStart.resolve(); await opening;
  assert.equal(pending.calls.at(-1).name, 'social_unsubscribe', 'abort during initialization cannot orphan a native subscription');

  const failed = harness(), failedAbort = new AbortController();
  const failing = failed.subscribe('7', failedAbort.signal, async () => { throw new Error('state failed'); }, () => {}, async () => {}, () => {});
  const rejected = assert.rejects(failing, /state failed/);
  deliver(failed, 1, change('8')); await rejected;
  assert.ok(!failed.calls.some(call => call.name === 'social_ack_subscription'), 'failed application is replayable');
  assert.equal(failed.calls.at(-1).name, 'social_unsubscribe');

  const closed = harness();
  const ending = closed.subscribe('8', new AbortController().signal, async () => {}, () => {}, async () => {}, () => {});
  const ended = assert.rejects(ending, /消息订阅已断开/);
  closed.channels[0].onmessage({ kind: 'closed' }); await ended;

  const stalled = harness(), delayedState = defer();
  const stalledStream = stalled.subscribe('8', new AbortController().signal, async () => delayedState.promise, () => {}, async () => {}, () => {});
  const stalledError = assert.rejects(stalledStream, /确认超时/);
  deliver(stalled, 1, change('9')); await flush();
  stalled.channels[0].onmessage({ kind: 'error', error: { code: 'unavailable', message: '确认超时' } });
  await stalledError;
  delayedState.resolve(); await flush();
  assert.ok(!stalled.calls.some(call => call.name === 'social_ack_subscription'), 'terminal error interrupts stalled application without a late ack');

  const gap = harness();
  const missing = gap.subscribe('8', new AbortController().signal, async () => assert.fail('gap cannot apply'), () => {}, async () => {}, () => {});
  const mismatch = assert.rejects(missing, /序号不连续/);
  deliver(gap, 2, change('9')); await mismatch;

  for (const desktop of [false, true]) {
    const exports = {}, http = { socialClient: {}, subscribe() {} }, tauri = { socialClient: {}, subscribe() {} };
    vm.runInNewContext(compile('client'), { exports, require: name => name === '@/lib/desktop' ? { IS_DESKTOP: desktop } : name === './http' ? http : name === './tauri' ? tauri : h.transport });
    assert.equal(exports.socialClient, desktop ? tauri.socialClient : http.socialClient);
    assert.equal(exports.subscribe, desktop ? tauri.subscribe : http.subscribe);
  }
  console.log('PASS: social IPC command parity, structured errors, early events, acknowledged delivery ordering, resync, startup abort, failed-state replay, channel closure and desktop transport selection');
})().catch(error => { console.error(error); process.exitCode = 1; });
