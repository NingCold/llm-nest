"""Explicit-memory checks using social_acceptance's isolated HTTP/mock-provider fixture."""
import json
import urllib.error
import uuid


def verify_memory(api, contact, direct, group, complete, send, history, wait_for, stop, start, calls_for):
    command = lambda: str(uuid.uuid4())
    a, b = contact('Memory A'), contact('Memory B')
    private = direct(a)
    shared = group('Memory group', [a, b])
    path = f'/conversations/{private}/memories/{a["id"]}'
    content = {'requestId': command(), 'kind': 'preference', 'text': 'MEMORY_ONLY_short_paragraphs', 'source': None}
    saved = api(path, 'POST', content)
    assert api(path, 'POST', content) == saved and len(api(path)) == 1
    complete(private, 'memory-read')
    assert 'MEMORY_ONLY_short_paragraphs' in json.dumps(calls_for('memory-read')[-1]['messages'])
    send(shared, 'memory-isolation', mentions=[a['id'], b['id']])
    wait_for(lambda: len([m for m in history(shared) if m['runId'] and m['status'] == 'committed']) == 2)
    assert all('MEMORY_ONLY_' not in json.dumps(r['messages']) for r in calls_for('memory-isolation'))
    stop(); start()
    managed = api(f'/ai-accounts/{a["id"]}')
    api(f'/ai-accounts/{a["id"]}', 'PATCH', {'requestId': command(), 'name': a['name'], 'bio': a['bio'], 'persona': managed['persona'], 'expectedRevision': managed['account']['revision'], 'binding': {**managed['binding'], 'modelId': 'alt'}})
    complete(private, 'memory-after-restart-model')
    request = calls_for('memory-after-restart-model')[-1]
    assert request['model'] == 'alt-wire' and 'MEMORY_ONLY_short_paragraphs' in json.dumps(request['messages'])

    changed = api(f'/memories/{saved["id"]}', 'PATCH', {**content, 'requestId': command(), 'expectedRevision': saved['revision'], 'text': 'MEMORY_ONLY_corrected'})
    complete(private, 'memory-corrected')
    request = json.dumps(calls_for('memory-corrected')[-1]['messages'])
    assert 'MEMORY_ONLY_corrected' in request and 'MEMORY_ONLY_short_paragraphs' not in request
    send(private, 'slow-memory-forget')
    running = wait_for(lambda: next((m for m in history(private) if m['status'] == 'generating'), None))
    forgotten = api(f'/memories/{saved["id"]}/forget', 'POST', {'requestId': command(), 'expectedRevision': changed['revision']})
    assert forgotten['status'] == 'forgotten' and not forgotten['text'] and api(path) == []
    wait_for(lambda: api(f'/messages/{running["id"]}')['status'] == 'interrupted')
    assert api(f'/runs/{running["runId"]}')['error']['code'] == 'context_changed'
    stop(); start()
    complete(private, 'memory-after-forget')
    assert 'MEMORY_ONLY_' not in json.dumps(calls_for('memory-after-forget')[-1]['messages'])

    source = send(private, 'memory-source')['message']
    wait_for(lambda: any(m['runId'] and m['status'] == 'committed' and m['text'] == '回应：memory-source' for m in history(private)))
    saved = api(path, 'POST', {'requestId': command(), 'kind': 'fact', 'text': 'DERIVED_MEMORY', 'source': {'messageId': source['id'], 'revision': source['revision']}})
    revised = api(f'/messages/{source["id"]}/revise', 'POST', {'requestId': command(), 'expectedRevision': source['revision'], 'text': 'revised-source'})
    stale = api(path)[0]
    assert stale['status'] == 'stale'
    complete(private, 'memory-after-source-revision')
    assert 'DERIVED_MEMORY' not in json.dumps(calls_for('memory-after-source-revision')[-1]['messages'])
    try:
        api(f'/memories/{saved["id"]}', 'PATCH', {'requestId': command(), 'expectedRevision': stale['revision'], 'kind': 'fact', 'text': 'DERIVED_MEMORY', 'source': saved['source']})
        raise AssertionError('stale source was silently reconfirmed')
    except urllib.error.HTTPError as error:
        assert error.code == 409
    confirmed = api(f'/memories/{saved["id"]}', 'PATCH', {'requestId': command(), 'expectedRevision': stale['revision'], 'kind': 'fact', 'text': 'DERIVED_MEMORY', 'source': {'messageId': source['id'], 'revision': revised['revision']}})
    api(f'/messages/{source["id"]}/withdraw', 'POST', {'requestId': command(), 'expectedRevision': revised['revision']})
    assert api(path)[0]['status'] == 'stale'
    # A departed AI's existing memories remain manageable, without granting it access.
    group_path = f'/conversations/{shared}/memories/{a["id"]}'
    group_memory = api(group_path, 'POST', {'requestId': command(), 'kind': 'fact', 'text': 'GROUP_ONLY', 'source': None})
    current = api(f'/conversations/{shared}')
    api(f'/conversations/{shared}/members', 'POST', {'requestId': command(), 'expectedRevision': current['revision'], 'actorId': a['id'], 'action': 'remove'})
    current = api(f'/conversations/{shared}')
    assert a['id'] in current['memoryOwnerIds'] and a['id'] not in current['memberIds']
    api(f'/memories/{group_memory["id"]}/forget', 'POST', {'requestId': command(), 'expectedRevision': group_memory['revision']})
    assert not api(group_path)
    events = api('/events?after=0')
    # Event pages contain references, never memory bodies.
    assert 'MEMORY_ONLY_' not in json.dumps(events)
    print('PASS: explicit memories via real HTTP/provider requests, scope isolation, restart/model change, correction/forget cancellation, source revisions/withdrawal, departed-owner management', flush=True)
