"""M2 group-topic continuation through real HTTP and the isolated provider."""
import json
import urllib.error
import uuid


def verify_references(api, contact, direct, group, send, history, wait_for, requests, stop, start):
    def command(**values):
        return dict(requestId=str(uuid.uuid4()), **values)

    def denied(operation):
        try:
            operation()
            raise AssertionError('invalid reference was accepted')
        except urllib.error.HTTPError as error:
            assert error.code in (400, 403, 409)
            assert json.loads(error.read())['code'] in ('invalid_input', 'not_authorized', 'revision_conflict')

    actor = contact('接续验收 AI')
    outsider = contact('非群成员 AI')
    cid = group('共同话题验收', [actor])
    private = direct(actor)
    original = send(cid, '群中明确选中的计划：周五讨论设计。')['message']
    send(cid, '未选中的群内容：这个秘密点不应自动进入私聊。')
    selection = dict(conversationId=cid, sources=[dict(kind='message', conversation_id=cid, message_id=original['id'], revision=original['revision'])])
    preview_input = dict(actorId=actor['id'], selection=selection)
    preview = api('/references/preview', 'POST', preview_input)
    assert preview['messages'] == [original] and preview['title'] == '共同话题验收'
    assert history(private) == [], 'preview/opening the unique chat must not send'
    assert direct(actor) == private
    denied(lambda: api('/references/preview', 'POST', dict(actorId=outsider['id'], selection=selection)))

    def post(text, reference=selection, reply=None):
        return command(text=text, replyTo=reply, mentions=[], reference=reference)

    def send_private(data):
        return api(f'/conversations/{private}/messages', 'POST', data)

    def response(text):
        return wait_for(lambda: next((m for m in history(private) if m['senderId'] == actor['id'] and m['status'] == 'committed' and m['text'].startswith('回应：' + text)), None))

    def request_for(text):
        return next(r for r in reversed(requests) if any(m['content'].startswith('[我 ·') and m['content'].endswith('\n' + text) for m in r['messages']))

    first = post('接续计划第一问')
    receipt = send_private(first)
    assert send_private(first)['message']['id'] == receipt['message']['id']
    response('接续计划第一问')
    wire = json.dumps(request_for('接续计划第一问'), ensure_ascii=False)
    assert original['text'] in wire and '共同话题验收' in wire
    assert '未选中的群内容' not in wire and 'persona-非群成员' not in wire
    assert receipt['message']['contextReference']['target']['actor_id'] == actor['id']
    assert original['text'] not in json.dumps(receipt, ensure_ascii=False), 'private message stores references, not copied source bodies'
    send_private(post('同一话题继续追问', None))
    response('同一话题继续追问')
    assert original['text'] in json.dumps(request_for('同一话题继续追问'), ensure_ascii=False)
    assert len(history(cid)) == 2, 'private replies never go back to the source group'

    slow = send_private(post('slow-reference-revision'))['message']
    wait_for(lambda: any(m['status'] == 'generating' and m['text'] for m in history(private)))
    revised = api(f"/messages/{original['id']}/revise", 'POST', command(text='修订为周六讨论', expectedRevision=original['revision']))
    wait_for(lambda: api(f"/messages/{slow['id']}")['contextReference']['stale'])
    wait_for(lambda: all(m['status'] != 'generating' for m in history(private)))
    assert any(m['status'] == 'interrupted' and m['text'].startswith('回应：slow-reference-revision') for m in history(private))
    denied(lambda: api('/references/preview', 'POST', preview_input))
    denied(lambda: send_private(post('不应保存旧引用')))
    assert not any(m['text'] == '不应保存旧引用' for m in history(private))
    assert send_private(first)['message']['contextReference']['stale'], 'lost ack retry returns current state even after invalidation'

    selection['sources'][0]['revision'] = revised['revision']
    fresh = send_private(post('slow-reference-leaving'))['message']
    wait_for(lambda: any(m['status'] == 'generating' and m['text'] for m in history(private)))
    current = api(f'/conversations/{cid}')
    api(f'/conversations/{cid}/members', 'POST', command(actorId=actor['id'], action='remove', expectedRevision=current['revision']))
    assert api(f"/messages/{fresh['id']}")['contextReference']['stale']
    denied(lambda: api('/references/preview', 'POST', preview_input))
    wait_for(lambda: all(m['status'] != 'generating' for m in history(private)))
    current = api(f'/conversations/{cid}')
    api(f'/conversations/{cid}/members', 'POST', command(actorId=actor['id'], action='add', expectedRevision=current['revision']))
    assert api(f"/messages/{fresh['id']}")['contextReference']['stale']
    saved = send_private(post('接续恢复后的新引用'))['message']
    response('接续恢复后的新引用')
    stop(); start()
    assert api(f"/messages/{saved['id']}") == saved
    assert direct(actor) == private

    # A selected related summary brings only that segment and remains traced to all originals.
    originals = [send(cid, f'接续分段原文 {i}')['message'] for i in range(28)]
    api(f'/conversations/{cid}/summary-settings', 'POST', command(enabled=True, binding=dict(providerId='test', modelId='chat', reasoningEffort=None, maxTokens=512), expectedRevision='0'))
    summary = wait_for(lambda: next((s for s in api(f'/conversations/{cid}/summaries')['segments'] if s['status'] == 'ready'), None))
    selected_original = next(m for m in originals if any(s['message_id'] == m['id'] for s in summary['sources']))
    with_summary = dict(conversationId=cid, sources=[dict(kind='message', conversation_id=cid, message_id=selected_original['id'], revision=selected_original['revision']), dict(kind='summary', summary_id=summary['id'], revision=summary['revision'])])
    sent = send_private(post('接续群摘要', with_summary))['message']
    response('接续群摘要')
    assert summary['text'] in json.dumps(request_for('接续群摘要'), ensure_ascii=False)
    covered = next(s for s in summary['sources'] if s['message_id'] != selected_original['id'])
    api(f"/messages/{covered['message_id']}/withdraw", 'POST', command(expectedRevision=covered['revision']))
    assert api(f"/messages/{sent['id']}")['contextReference']['stale']
    denied(lambda: api('/references/preview', 'POST', dict(actorId=actor['id'], selection=with_summary)))
    send(cid, '用于浏览器接续验证的新群话题。')
    print('PASS: group-to-private references, actual bounded provider context, current-member permissions, source/summary invalidation, lost ack idempotence and restart', flush=True)
