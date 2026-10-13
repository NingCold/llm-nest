"""Single-window export through the real HTTP adapter, using isolated mock data."""
import json
import urllib.error
import uuid


def verify_exports(api, contact, direct, group, send, history, wait_for):
    actor = contact('导出验收🙂', None)
    cid = direct(actor)
    empty = api(f'/conversations/{cid}/export?format=json')
    assert empty['messageCount'] == 0 and json.loads(empty['content'])['messages'] == []
    first = send(cid, '旧正文 secret')['message']
    withdrawn = send(cid, '撤回正文 secret')['message']
    api(f'/messages/{first["id"]}/revise', 'POST', {'requestId': str(uuid.uuid4()), 'expectedRevision': first['revision'], 'text': '中文🙂\n第二行\n```'})
    api(f'/messages/{withdrawn["id"]}/withdraw', 'POST', {'requestId': str(uuid.uuid4()), 'expectedRevision': withdrawn['revision']})
    for i in range(205):
        send(cid, f'导出消息 {i}')
    page = api(f'/conversations/{cid}/messages')
    assert page['hasMore'] and len(page['messages']) == 200
    # This actor has no model, but its final coalesced input plan still needs to
    # settle before comparing a read-only export with the surrounding snapshot.
    wait_for(lambda: all(p['status'] not in ('queued','running') for p in api(f'/conversations/{cid}/plans')))
    before = api('/init')
    json_file = api(f'/conversations/{cid}/export?format=json')
    markdown = api(f'/conversations/{cid}/export?format=markdown')
    assert api('/init') == before, 'export must not mutate read positions, conversation state or event cursors'
    document = json.loads(json_file['content'])
    assert document['format'] == 'llm-nest.conversation' and document['version'] == 1
    assert document['conversationId'] == cid and document['title'] == actor['name']
    assert json_file['fileName'].endswith('.json') and markdown['fileName'].endswith('.md')
    assert json_file['messageCount'] == markdown['messageCount'] == 207
    messages = document['messages']
    assert [m['sequence'] for m in messages] == [str(i) for i in range(1, 208)]
    assert messages[0]['id'] == first['id'] and messages[0]['revision'] == '2'
    assert messages[0]['parts'][0]['text'] == '中文🙂\n第二行\n```'
    assert messages[1]['status'] == 'withdrawn' and messages[1]['parts'] == []
    for secret in ['旧正文 secret', '撤回正文 secret', 'persona-导出验收', 'fixture-private-reasoning']:
        assert secret not in json_file['content'] and secret not in markdown['content']
    assert '已撤回' in markdown['content'] and '导出消息 204' in markdown['content']
    for suffix in ['?format=pdf', '', '?format=json&format=markdown']:
        try:
            api(f'/conversations/{cid}/export{suffix}')
            raise AssertionError('invalid format accepted')
        except urllib.error.HTTPError as error:
            assert error.code == 400 and json.load(error)['code'] == 'invalid_input'
    group_actor = contact('Group export')
    gid = group('导出群', [group_actor])
    send(gid, '只在群里', mentions=[group_actor['id']])
    answer = wait_for(lambda: next((m for m in history(gid) if m['senderId'] == group_actor['id'] and m['status'] == 'committed'), None))
    exported_group = json.loads(api(f'/conversations/{gid}/export?format=json')['content'])
    assert exported_group['kind'] == 'group' and exported_group['messages'][-1]['id'] == answer['id']
    assert 'fixture-private-reasoning' not in json.dumps(exported_group)
    assert '只在群里' not in api(f'/conversations/{cid}/export?format=markdown')['content']
    print('PASS: HTTP full-window Markdown/JSON, 207 messages, empty/group windows, current revisions, withdrawal, Unicode, isolation and read-only export', flush=True)
