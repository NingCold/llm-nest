"""Contact blocking through the real HTTP host and mock provider."""
from datetime import datetime, timedelta, timezone
import time
import urllib.error
import uuid


def verify_blocks(api, contact, direct, group, complete, send, history, wait_for, stop, start):
    a, b = contact('Block fixture'), contact('Unblocked fixture')
    cid, gid = direct(a), group('Shared block fixture', [a, b])

    def entry():
        return next(e for e in api('/init')['addressBook'] if e['accountId'] == a['id'])

    def block(value):
        return api(f'/contacts/{a["id"]}/block', 'POST', {
            'requestId': str(uuid.uuid4()), 'blocked': value, 'expectedRevision': entry()['revision']})

    def record(c):
        return api(f'/conversations/{c}')

    def intent(c, actor):
        due = datetime.now(timezone.utc) + timedelta(minutes=5)
        return api(f'/conversations/{c}/contact-intents', 'POST', {
            'requestId': str(uuid.uuid4()), 'actorId': actor['id'], 'text': 'block fixture future',
            'dueAt': due.isoformat(), 'expiresAt': (due + timedelta(minutes=5)).isoformat()})

    def forbidden(action):
        try:
            action()
            raise AssertionError('blocked operation accepted')
        except urllib.error.HTTPError as e:
            assert e.code == 403

    answer = complete(cid, 'block-history')
    for c in [cid, gid]:
        settings = dict(record(c)['contactSchedule']['settings'], enabled=True)
        api(f'/conversations/{c}/contact-settings', 'POST', {'requestId': str(uuid.uuid4()), 'settings': settings})
        intent(c, a)
    other = intent(gid, b)
    send(cid, 'slow-block-running')
    wait_for(lambda: any(m['status'] == 'generating' and m['text'] for m in history(cid)))
    send(cid, 'block-queued')
    cursor = api('/init')['cursor']
    block(True)
    wait_for(lambda: not record(cid)['activeRun'])
    assert not record(cid)['pendingPlans']
    assert record(cid)['directBlocked'] and not record(gid)['directBlocked']
    assert next(m for m in history(cid) if m['id'] == answer['id']) == answer
    assert any(m['status'] == 'interrupted' for m in history(cid))
    assert api(f'/accounts/{a["id"]}')['status'] == 'active'
    assert any(e['change']['kind'] == 'contact_changed' for e in api('/events?after=' + cursor))
    forbidden(lambda: send(cid, 'must not save'))
    for c in [cid, gid]:
        forbidden(lambda: intent(c, a))
        for i in record(c)['contactSchedule']['intents']:
            assert i['status'] == ('pending' if i['id'] == other['id'] else 'cancelled')
    send(gid, 'group-block-mention', mentions=[a['id']])
    wait_for(lambda: any(m['senderId'] == a['id'] and m['status'] == 'committed' for m in history(gid)))
    for listed in [False, True]:
        api(f'/contacts/{a["id"]}', 'POST', {'requestId': str(uuid.uuid4()), 'listed': listed, 'expectedRevision': entry()['revision']})
        assert entry()['blocked']
    stop(); start()
    assert entry()['blocked'] and record(cid)['directBlocked']
    assert direct(a) == cid
    before = len(history(cid))
    block(False)
    time.sleep(1)
    assert len(history(cid)) == before and not record(cid)['pendingPlans']
    assert all(i['status'] == 'cancelled' for i in record(cid)['contactSchedule']['intents'])
    complete(cid, 'block-new-message')
    print('PASS: HTTP block/unblock, running and queued cancellation, shared group mentions, scoped appointments, list independence, events and restart without replay', flush=True)
