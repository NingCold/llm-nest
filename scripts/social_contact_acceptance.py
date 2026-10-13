"""One-shot consent, clock scheduling and recovery through HTTP with a mock provider."""
from datetime import datetime, timedelta, timezone
import json
import time
import urllib.error
import uuid


def verify_contacts(api, contact, direct, group, send, history, wait_for, requests, stop, start):
    def command(**kw):
        return dict(requestId=str(uuid.uuid4()), **kw)

    def record(cid):
        return api(f'/conversations/{cid}')

    def settings(cid, **kw):
        value = dict(record(cid)['contactSchedule']['settings'], **kw)
        return api(f'/conversations/{cid}/contact-settings', 'POST', command(settings=value))

    def create(cid, ai, text, delay=.7, lifetime=20, body=None):
        due = datetime.now(timezone.utc) + timedelta(seconds=delay)
        data = body or command(actorId=ai['id'], text=text,
                              dueAt=due.astimezone(timezone(timedelta(hours=8))).isoformat(),
                              expiresAt=(due + timedelta(seconds=lifetime)).isoformat())
        return api(f'/conversations/{cid}/contact-intents', 'POST', data), data

    def intent(cid, id):
        return next(i for i in record(cid)['contactSchedule']['intents'] if i['id'] == id)

    def settled(cid, id, status):
        return wait_for(lambda: intent(cid, id) if intent(cid, id)['status'] == status else None)

    def calls(text):
        return [r for r in requests if any(m['role'] == 'user' and m['content'].startswith('[用户明确授权的一次性约定') and m['content'].endswith('\n' + text) for m in r['messages'])]

    a, b = contact('约定验收甲'), contact('约定验收乙')
    cid = group('一次性约定与免打扰', [a, b])
    try:
        create(cid, a, 'no-consent')
        raise AssertionError('implicit contact consent accepted')
    except urllib.error.HTTPError as e:
        assert e.code == 403
    settings(cid, enabled=True)
    p = record(cid)['participation']['policy']; p['limitedAuto'] = True
    api(f'/conversations/{cid}/participation', 'POST', command(policy=p))
    first, body = create(cid, a, 'm3-contact-once')
    assert not history(cid), 'creating an intent cannot create a fake human message'
    assert create(cid, a, '', body=body)[0]['id'] == first['id']
    settled(cid, first['id'], 'delivered')
    assert len(history(cid)) == 1 and history(cid)[0]['senderId'] == a['id']
    assert history(cid)[0]['replyTo'] is None
    assert len(calls('m3-contact-once')) == 1
    assert not any(m['content'].startswith('[我 ·') for m in calls('m3-contact-once')[0]['messages'])
    assert first['utcOffsetMinutes'] == 480
    assert record(cid)['participation']['reservedTokensToday'] == 8192
    time.sleep(.9)
    assert len(history(cid)) == 1, 'one-shot contact cannot start a group auto chain'

    until = (datetime.now(timezone.utc) + timedelta(seconds=2)).isoformat()
    settings(cid, quietUntil=until, quietAction='defer')
    deferred, _ = create(cid, a, 'm3-contact-deferred')
    time.sleep(.9)
    assert not calls('m3-contact-deferred')
    settled(cid, deferred['id'], 'delivered')
    assert len(calls('m3-contact-deferred')) == 1
    settings(cid, quietUntil=(datetime.now(timezone.utc) + timedelta(seconds=30)).isoformat(), quietAction='skip')
    skipped, _ = create(cid, a, 'm3-contact-skip')
    settled(cid, skipped['id'], 'skipped'); assert not calls('m3-contact-skip')
    settings(cid, quietAction='defer')
    expired, _ = create(cid, a, 'm3-contact-expire', lifetime=.5)
    settled(cid, expired['id'], 'expired'); assert not calls('m3-contact-expire')

    private = direct(a); settings(private, enabled=True)
    future, _ = create(private, a, 'm3-contact-future', delay=300)
    api(f'/conversations/{private}/control', 'POST', command(expectedRevision=record(private)['revision'], action='end'))
    assert intent(private, future['id'])['status'] == 'pending'
    settings(private, quietUntil=(datetime.now(timezone.utc) + timedelta(seconds=30)).isoformat())
    send(private, 'm3-normal-during-quiet')
    wait_for(lambda: any(m['text'] == '回应：m3-normal-during-quiet' and m['status'] == 'committed' for m in history(private)))
    settings(private, quietUntil=None)
    slow, _ = create(private, a, 'slow-m3-contact-expiring', lifetime=.7)
    settled(private, slow['id'], 'expired')
    assert len(calls('slow-m3-contact-expiring')) == 1
    assert any(m['status'] == 'interrupted' for m in history(private))

    slow, _ = create(private, a, 'slow-m3-contact-revoke')
    wait_for(lambda: intent(private, slow['id'])['status'] == 'executing')
    settings(private, enabled=False)
    assert intent(private, slow['id'])['status'] == 'cancelled'
    assert intent(private, future['id'])['status'] == 'cancelled'
    assert not record(private)['activeRun']
    settings(private, enabled=True)
    cancelled, data = create(private, a, 'm3-contact-cancel', delay=100)
    cancel_body = command(expectedRevision=cancelled['revision'])
    path = f'/conversations/{private}/contact-intents/{cancelled["id"]}/cancel'
    assert api(path, 'POST', cancel_body)['status'] == 'cancelled'
    assert api(path, 'POST', cancel_body)['status'] == 'cancelled'
    assert create(private, a, '', body=data)[0]['status'] == 'cancelled'

    recover = direct(b); settings(recover, enabled=True)
    crashed, _ = create(recover, b, 'slow-m3-contact-crash')
    wait_for(lambda: any(m['status'] == 'generating' and m['text'] for m in history(recover)))
    pending, _ = create(recover, b, 'm3-contact-restart-pending', delay=2)
    stop(); start()
    assert intent(recover, crashed['id'])['status'] == 'uncertain'
    settled(recover, pending['id'], 'delivered')
    assert len(calls('slow-m3-contact-crash')) == 1 and len(calls('m3-contact-restart-pending')) == 1
    assert record(recover)['participation']['autoTurnsToday'] == 2
    print('PASS: explicit one-shot contact, UTC/offset, no fake human message, one delivery, DND defer/skip, expiry, normal reply, cancellation/revocation, budget persistence and crash recovery', flush=True)
