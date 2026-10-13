"""M3 participation through the real HTTP host and an isolated mock provider."""
import json
import time
import uuid
import urllib.error


def verify_participation(api, contact, direct, group, send, history, wait_for, requests, stop, start):
    def command(**values):
        return dict(requestId=str(uuid.uuid4()), **values)

    def record(cid):
        return api(f'/conversations/{cid}')

    def configure(cid, **changes):
        policy = dict(record(cid)['participation']['policy'], **changes)
        return api(f'/conversations/{cid}/participation', 'POST', command(policy=policy))

    def idle(cid):
        def settled():
            c = record(cid)
            return c if not c['activeRun'] and not c['pendingPlans'] else None
        return wait_for(settled)

    a, b = contact('节奏验收甲'), contact('节奏验收乙')
    private = direct(a)
    before = len(requests)
    first = send(private, 'm3-merge-first')['message']
    second = send(private, 'm3-merge-second')['message']
    assert first['id'] != second['id']
    pending = record(private)['pendingPlans']
    assert len(pending) == 1 and pending[0]['inputIds'] == [first['id'], second['id']]
    idle(private)
    assert len(requests) == before + 1
    assert len(history(private)) == 3
    assert all(text in json.dumps(requests[-1]) for text in ['m3-merge-first', 'm3-merge-second'])

    cid = group('接话节奏验收', [a, b])
    send(cid, 'm3-default-ignore')
    assert not record(cid)['pendingPlans']
    assert len(requests) == before + 1
    configure(cid, limitedAuto=True, maxAutoPerDay=5, dailyTokenBudget=40960)
    assert not record(cid)['pendingPlans'], 'enabling consent cannot replay old input'
    before = len(requests)
    send(cid, 'm3-auto-first')
    c = idle(cid)
    assert len(requests) == before + 3
    assert c['participation']['state']['reason'] == 'turn_limit'
    assert c['participation']['autoTurnsToday'] == 3
    assert c['participation']['reservedTokensToday'] == 24576
    send(cid, 'm3-auto-second')
    c = idle(cid)
    assert c['participation']['autoTurnsToday'] == 5
    assert c['participation']['state']['reason'] == 'daily_limit'
    before = len(requests)
    send(cid, 'm3-daily-blocked')
    assert not record(cid)['pendingPlans'] and len(requests) == before
    send(cid, 'm3-explicit-after-budget', mentions=[a['id']])
    idle(cid)
    assert len(requests) == before + 1
    assert record(cid)['participation']['autoTurnsToday'] == 5
    stop(); start()
    assert record(cid)['participation']['autoTurnsToday'] == 5
    before = len(requests)
    send(cid, 'm3-after-restart')
    assert not record(cid)['pendingPlans'] and len(requests) == before

    limited = group('自动接话额度验收', [a, b])
    configure(limited, limitedAuto=True, dailyTokenBudget=1024)
    before = len(requests)
    send(limited, 'm3-insufficient-reservation')
    idle(limited)
    assert len(requests) == before
    plan = api(f'/conversations/{limited}/plans')[0]
    assert plan['status'] == 'failed' and '额度不足' in plan['failure']
    assert len(history(limited)) == 1, 'budget rejection must roll back generating placeholder'
    assert record(limited)['participation']['reservedTokensToday'] == 0

    cancellable = group('自动接话中断验收', [a, b])
    configure(cancellable, limitedAuto=True)
    send(cancellable, 'slow-m3-auto')
    wait_for(lambda: any(m['status'] == 'generating' and m['text'] for m in history(cancellable)))
    configure(cancellable, limitedAuto=False)
    c = idle(cancellable)
    assert any(m['status'] == 'interrupted' for m in history(cancellable))
    assert c['participation']['autoTurnsToday'] == 1
    assert c['participation']['reservedTokensToday'] == 8192
    old = dict(c['participation']['policy'])
    configure(cancellable, maxAutoPerDay=20)
    try:
        api(f'/conversations/{cancellable}/participation', 'POST', command(policy=old))
        raise AssertionError('stale policy revision accepted')
    except urllib.error.HTTPError as error:
        assert error.code == 409
    time.sleep(.2)
    assert not record(cancellable)['pendingPlans']
    continuous = group('自动生成中继续输入验收', [a, b])
    configure(continuous, limitedAuto=True)
    send(continuous, 'slow-m3-continued-input')
    wait_for(lambda: any(m['status'] == 'generating' and m['text'] for m in history(continuous)))
    active = record(continuous)['activeRun']
    later = send(continuous, 'm3-later-human-input')['message']
    assert any(p['triggerId'] == later['id'] for p in record(continuous)['pendingPlans'])
    api(f'/runs/{active["id"]}/cancel', 'POST', {})
    idle(continuous)
    assert any(m['status'] == 'committed' and m['replyTo'] == later['id'] for m in history(continuous))
    # The fifth window waits for a global slot; cancelling releases it promptly.
    parallel = [direct(contact(f'并发节奏验收{i}')) for i in range(5)]
    before = len(requests)
    for i, space in enumerate(parallel):
        send(space, f'slow-m3-parallel-{i}')
    wait_for(lambda: sum(any(m['status'] == 'generating' and m['text'] for m in history(space)) for space in parallel) == 4)
    assert len(requests) == before + 4
    assert sum(record(space)['activeRun'] is not None for space in parallel) == 4
    running = next(record(space)['activeRun'] for space in parallel if record(space)['activeRun'])
    api(f'/runs/{running["id"]}/cancel', 'POST', {})
    wait_for(lambda: len(requests) == before + 5)
    for space in parallel:
        c = record(space)
        api(f'/conversations/{space}/control', 'POST', command(action='end', expectedRevision=c['revision']))
    for space in parallel:
        idle(space)
    print('PASS: M3 input coalescing, opt-in bounded group participation, persistent daily reservations, explicit priority, policy cancellation and restart without budget reset', flush=True)
