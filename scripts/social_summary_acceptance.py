"""M2 segmented summaries against the real HTTP service and isolated mock provider."""
import json
import time
import urllib.error
import uuid


def verify_summaries(api, contact, group, send, history, wait_for, stop, start, requests, provider):
    def command(**values):
        return dict(requestId=str(uuid.uuid4()), **values)

    actor = contact('摘要验收 AI')
    cid = group('窗口摘要验收', [actor])
    path = f'/conversations/{cid}/summaries'
    settings_path = f'/conversations/{cid}/summary-settings'
    state = lambda: api(path)
    first = lambda: state()['segments'][0]
    binding = dict(providerId='test', modelId='chat', reasoningEffort=None, maxTokens=1024)
    assert state() == {'settings': {'id': cid, 'enabled': False, 'binding': None, 'revision': '0'}, 'segments': [], 'hasMore': False}
    originals = []
    for index in range(28):
        text = f'早期讨论 {index}：项目采用蓝色界面。' if index < 16 else f'近期记录 {index} ' + '进展细节 ' * 170
        originals.append(send(cid, text)['message'])
    assert state()['segments'] == [], 'summary generation requires an explicit binding and enable'
    before_count = len(requests)
    enable = command(enabled=True, binding=binding, expectedRevision='0')
    configured = api(settings_path, 'POST', enable)
    assert api(settings_path, 'POST', enable) == configured
    summary = wait_for(lambda: next((s for s in state()['segments'] if s['status'] == 'ready'), None))
    assert len(requests) > before_count
    assert summary['fromSequence'] == '1' and int(summary['throughSequence']) <= 16
    assert len(history(cid)) == 28 and all(m['status'] == 'committed' for m in history(cid)), 'summary is not a chat message'
    source_request = next(r for r in reversed(requests) if any(m['content'].startswith('[窗口分段摘要原文]') for m in r['messages']))
    assert source_request['max_tokens'] == 1024 and 'temperature' not in source_request
    serialized = json.dumps(source_request, ensure_ascii=False)
    assert 'persona-' not in serialized and 'fixture-private-reasoning' not in serialized
    assert '近期记录 27' not in serialized
    # A long window uses earlier summaries along with the newest original messages.
    trigger = send(cid, '继续讨论项目', mentions=[actor['id']])['message']
    wait_for(lambda: any(m['senderId'] == actor['id'] and m['status'] == 'committed' for m in history(cid)))
    reply_request = next(r for r in reversed(requests) if any(m['content'].endswith('\n继续讨论项目') for m in r['messages']))
    assert any('[本窗口分段摘要' in m['content'] for m in reply_request['messages'])
    assert not any(m['content'].endswith(originals[0]['text']) for m in reply_request['messages'])
    assert any(m['content'].endswith(trigger['text']) for m in reply_request['messages'])
    # Revising a source removes the generated text immediately and emits a persistent reference.
    cursor = api('/init')['cursor']
    revised = api(f"/messages/{originals[0]['id']}/revise", 'POST', command(text='修订：改用灰白界面', expectedRevision='1'))
    stale = next(s for s in state()['segments'] if s['id'] == summary['id'])
    assert stale['status'] == 'stale' and stale['text'] == ''
    events = api('/events?after=' + cursor)
    assert any(e['change']['kind'] == 'summaries_changed' for e in events)

    def current():
        return next(s for s in state()['segments'] if s['id'] == summary['id'])

    def retry():
        s = current()
        data = command(retry={'id': s['id'], 'expectedRevision': s['revision']})
        queued = api(path, 'POST', data)
        assert api(path, 'POST', data)['id'] == queued['id'], 'retry receipt is idempotent'
        return queued

    for mode in ['eof', 'empty', 'tool', 'error']:
        provider.summary_mode = mode
        retry()
        failed = wait_for(lambda: current() if current()['status'] == 'failed' else None)
        assert failed['text'] == '' and failed['error']
    provider.summary_mode = 'slow'
    retry()
    wait_for(lambda: current()['status'] == 'running')
    api(f"/messages/{revised['id']}/withdraw", 'POST', command(expectedRevision=revised['revision']))
    wait_for(lambda: current()['status'] == 'stale')
    time.sleep(.7)
    assert current()['text'] == '', 'late provider output cannot revive withdrawn material'
    # Changing configuration stops a running background request without changing the conversation.
    retry()
    wait_for(lambda: current()['status'] == 'running')
    config = state()['settings']
    disabled = api(settings_path, 'POST', command(enabled=False, binding=binding, expectedRevision=config['revision']))
    wait_for(lambda: current()['status'] == 'interrupted')
    api(settings_path, 'POST', command(enabled=True, binding=binding, expectedRevision=disabled['revision']))
    retry()
    wait_for(lambda: current()['status'] == 'running')
    count = len(requests)
    stop(); provider.summary_mode = 'ok'; start()
    assert current()['status'] == 'interrupted'
    time.sleep(.3)
    assert len(requests) == count, 'restart does not replay unfinished summary calls'
    retry()
    ready = wait_for(lambda: current() if current()['status'] == 'ready' else None)
    assert all(s['message_id'] != revised['id'] for s in ready['sources'])
    assert next(m for m in history(cid) if m['id'] == revised['id'])['status'] == 'withdrawn'
    try:
        api(settings_path, 'POST', command(enabled=True, binding=dict(binding, modelId='missing'), expectedRevision=state()['settings']['revision']))
        raise AssertionError('unknown summary model accepted')
    except urllib.error.HTTPError as error:
        assert error.code in (400, 422)
    print('PASS: M2 summaries opt-in/explicit budget, background segments, original history, real context use, versioned invalidation, independent EOF/empty/tool/provider failures, idempotent retry, withdrawal race, settings cancellation and crash/no replay', flush=True)
