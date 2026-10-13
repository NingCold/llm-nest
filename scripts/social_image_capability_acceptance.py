"""Image capability metadata and actual dispatch, using the local mock provider."""
import base64
import json
import urllib.error
import uuid

from social_image_acceptance import png_bytes


def verify_image_capabilities(api, settings, contact, direct, group, history, wait, requests, stop, start):
    image = {'mime': 'image/png', 'data': base64.b64encode(png_bytes()).decode()}

    def update(value, model='chat'):
        return settings('/models/image-input', 'POST', {'provider': 'test', 'model': model, 'imageInput': value})

    def capability(model='chat'):
        return next(m['imageInput'] for p in settings('/settings')['providers'] if p['id'] == 'test' for m in p['models'] if m['id'] == model)

    def send(cid, text, **extra):
        return api(f'/conversations/{cid}/messages', 'POST', dict(requestId=str(uuid.uuid4()), text=text, **extra))['message']

    def settled(cid):
        return wait(lambda: (plans := api(f'/conversations/{cid}/plans')) and all(p['status'] not in ('queued', 'running') for p in plans))

    assert capability() == {'support': 'unknown', 'source': 'unknown'}
    a = contact('能力甲')
    b = contact('能力乙', model='alt')
    cid = direct(a)
    sent = send(cid, 'cap-unknown', images=[image]); settled(cid)
    assert len(requests) == 1, 'unknown support must allow dispatch'
    assert api('/init')['imageInputs'][a['id']] == 'unknown'
    update(False)
    assert capability() == {'support': 'unsupported', 'source': 'config'}
    sent = send(cid, 'cap-history-only-followup'); settled(cid)
    assert len(requests) == 1, 'historical images must also be checked'
    failed = next(m for m in reversed(history(cid)) if m['senderId'] == a['id'])
    assert api(f'/runs/{failed["runId"]}')['error']['code'] == 'image_input_unsupported'
    assert any(m['id'] == sent['id'] and m['status'] == 'committed' for m in history(cid))
    state = api('/init')
    assert state['imageInputs'][a['id']] == 'unsupported'
    assert all('binding' not in account and 'persona' not in account for account in state['accounts'])
    assert all(isinstance(value, str) for value in state['imageInputs'].values())
    # A purely textual window remains usable with a text-only model.
    plain = direct(contact('文字甲'))
    send(plain, 'cap-text-only'); settled(plain); assert len(requests) == 2
    # Group members run independently; the unsupported member does not block others.
    update(True, 'alt')
    gid = group('图片能力验收群', [a, b])
    source = send(gid, 'cap-mixed-group', images=[image], mentions=[a['id'], b['id']]); settled(gid)
    assert len(requests) == 3 and requests[-1]['model'] == 'alt-wire'
    plans = api(f'/conversations/{gid}/plans')
    assert next(p for p in plans if p['actorId'] == a['id'])['status'] == 'failed'
    assert next(p for p in plans if p['actorId'] == b['id'])['status'] == 'succeeded'
    # Cross-window image references are checked even when the current input has no image.
    selection = {'conversationId': gid, 'sources': [{'kind': 'message', 'conversation_id': gid, 'message_id': source['id'], 'revision': source['revision']}]}
    send(cid, 'cap-reference', reference=selection); settled(cid); assert len(requests) == 3
    stop(); start()
    assert capability()['support'] == 'unsupported'
    assert api('/init')['imageInputs'][a['id']] == 'unsupported'
    assert any(m['id'] == source['id'] and m['images'] for m in history(gid))
    update(None)
    assert capability() == {'support': 'unknown', 'source': 'unknown'}
    send(cid, 'cap-back-to-auto'); settled(cid); assert len(requests) == 4
    update(True)
    assert capability() == {'support': 'supported', 'source': 'config'}
    send(cid, 'cap-supported'); settled(cid); assert len(requests) == 5
    for body in [dict(provider='test', model='missing', imageInput=True), dict(provider='test', model='chat', imageInput='yes')]:
        try:
            settings('/models/image-input', 'POST', body)
        except urllib.error.HTTPError as error:
            assert error.code in (400, 422)
        else:
            raise AssertionError('invalid override accepted')
    assert capability()['support'] == 'supported'
    print('PASS: image capability HTTP metadata, explicit overrides/auto/restart, persisted messages, unknown dispatch, zero unsupported requests, text-only windows, mixed groups and shared image references')
