"""Read positions, personal list state and full history search through real HTTP."""
import urllib.error
import urllib.parse
import uuid


def verify_message_list(api, contact, direct, group, complete, send, stop, start):
    actor = contact('Message list fixture')
    cid = direct(actor)

    def state():
        return api(f'/conversations/{cid}')['listState']

    def change(action):
        return api(f'/conversations/{cid}/list-state', 'POST', {
            'requestId': str(uuid.uuid4()), 'expectedRevision': state()['revision'], 'action': action})

    assert state()['unreadCount'] == 0
    answer = complete(cid, '正文验收 🪶 needle')
    assert state()['unreadCount'] == 1
    page = api(f'/conversations/{cid}/messages')
    change({'kind': 'read', 'through': page['readThrough']})
    assert state()['unreadCount'] == 0
    change({'kind': 'pin', 'pinned': True})
    change({'kind': 'unread'})
    stop(); start()
    assert state()['pinned'] and state()['markedUnread']
    cursor = api('/init')['cursor']
    change({'kind': 'read', 'through': state()['latestDelivery']})
    events = api('/events?after=' + cursor)
    assert any(e['change']['kind'] == 'conversation_list_changed' for e in events)
    search = lambda text, **kw: api('/messages/search?' + urllib.parse.urlencode(dict(query=text, **kw)))
    assert len(search('正文验收', conversationId=cid)['messages']) == 2
    assert not search('fixture-private-reasoning', conversationId=cid)['messages']
    context = api(f'/messages/{answer["id"]}/context')
    assert context['messages'][-1]['id'] == answer['id']
    saved = send(cid, 'original %_ message')['message']
    edited = api(f'/messages/{saved["id"]}/revise', 'POST', {
        'requestId': str(uuid.uuid4()), 'expectedRevision': saved['revision'], 'text': 'updated source'})
    assert all(m['id'] != saved['id'] for m in search('%_', conversationId=cid)['messages'])
    api(f'/messages/{saved["id"]}/withdraw', 'POST', {
        'requestId': str(uuid.uuid4()), 'expectedRevision': edited['revision']})
    assert all(m['id'] != saved['id'] for m in search('updated source', conversationId=cid)['messages'])
    for path in ['/messages/search?query=', '/messages/search?query=x&before=-1']:
        try:
            api(path)
            raise AssertionError('invalid search accepted')
        except urllib.error.HTTPError as error:
            assert error.code in (400, 422)
    print('PASS: HTTP read/unread/pin, restart, list events, full-text search, source context, edit/withdraw freshness and invalid cursors', flush=True)
