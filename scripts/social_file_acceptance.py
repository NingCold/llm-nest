"""Generic attachments: exact bytes, scoped downloads and metadata-only model input."""
import base64
import io
import json
import urllib.error
import uuid
import zipfile
from urllib.parse import unquote
from social_image_acceptance import png_bytes


def verify_files(api, contact, direct, group, history, wait, requests, stop, start, request_file):
    private_content = b'fixture-file-content-must-not-reach-model\x00\xff\x01'
    attachment = {'name': '说明 中文.txt', 'mime': 'text/plain', 'data': base64.b64encode(private_content).decode()}
    actor = contact('文件验收')
    cid = direct(actor)

    def send(cid, text='', files=None, request_id=None, **extra):
        return api(f'/conversations/{cid}/messages', 'POST', dict(
            requestId=request_id or str(uuid.uuid4()), text=text,
            files=files if files is not None else [attachment], **extra))['message']

    def settled(cid):
        return wait(lambda: (plans := api(f'/conversations/{cid}/plans')) and all(p['status'] not in ('queued', 'running') for p in plans))

    def denied(path, method='GET', body=None, codes=(400, 403, 409)):
        try:
            with request_file(path, method, body):
                pass
        except urllib.error.HTTPError as error:
            assert error.code in codes, (error.code, error.read())
            return
        raise AssertionError('request unexpectedly succeeded: ' + path)

    def path(message, account=None, index=None):
        prefix = f'/ai-accounts/{account}/view' if account else ''
        index = message['files'][0]['index'] if index is None else index
        return f'{prefix}/messages/{message["id"]}/files/{index}/download?revision={message["revision"]}'

    def download(url):
        with request_file(url) as response:
            assert response.headers['Content-Type'] == 'application/octet-stream'
            assert response.headers['Cache-Control'] == 'no-store'
            assert response.headers['X-Content-Type-Options'] == 'nosniff'
            disposition = response.headers['Content-Disposition']
            assert disposition.startswith('attachment;')
            return unquote(disposition.split("filename*=UTF-8''")[1]), response.read()

    def metadata_only(request):
        wire = json.dumps(request, ensure_ascii=False)
        assert attachment['name'] in wire and '文件内容尚未读取' in wire
        assert attachment['data'] not in wire and 'fixture-file-content-must-not-reach-model' not in wire
        for message in request['messages']:
            if isinstance(message['content'], list):
                assert all(part.get('type') == 'text' for part in message['content']), 'generic bytes must not become provider file/image blocks'

    retry_id = str(uuid.uuid4())
    sent = send(cid, request_id=retry_id)
    assert sent['text'] == '' and sent['images'] == []
    assert sent['files'] == [{'index': 0, 'name': attachment['name'], 'mime': 'text/plain', 'byteLength': len(private_content)}]
    assert attachment['data'] not in json.dumps(sent)
    assert send(cid, request_id=retry_id)['id'] == sent['id']
    denied(f'/conversations/{cid}/messages', 'POST', {'requestId': retry_id, 'text': '', 'files': [dict(attachment, name='changed.txt')]})
    for account in [None, actor['id']]:
        assert download(path(sent, account)) == (attachment['name'], private_content)
    outsider = contact('文件无权限', model=None)
    denied(path(sent, outsider['id']))
    denied(path(sent, index=999))
    settled(cid)
    metadata_only(requests[-1])
    stop(); start()
    assert next(m for m in history(cid) if m['id'] == sent['id'])['files'] == sent['files']
    assert download(path(sent))[1] == private_content

    revised = api(f'/messages/{sent["id"]}/revise', 'POST', {'requestId': str(uuid.uuid4()), 'expectedRevision': sent['revision'], 'text': '补充说明'})
    assert revised['files'][0]['index'] == 1
    denied(path(sent))
    assert download(path(revised))[1] == private_content
    cleared = api(f'/messages/{sent["id"]}/revise', 'POST', {'requestId': str(uuid.uuid4()), 'expectedRevision': revised['revision'], 'text': ''})
    assert download(path(cleared))[1] == private_content
    withdrawn = api(f'/messages/{sent["id"]}/withdraw', 'POST', {'requestId': str(uuid.uuid4()), 'expectedRevision': cleared['revision']})
    assert withdrawn['files'] == []
    denied(path(cleared))

    # File content is never rendered inline, including HTML and image MIME values.
    gid = group('文件共同群', [actor, outsider])
    source = send(gid, '保存文件', files=[attachment, {'name': '页面.html', 'mime': 'text/html', 'data': base64.b64encode(b'<script>throw 1</script>').decode()}, {'name': '空白.txt', 'mime': 'text/plain', 'data': ''}])
    assert download(path(source, index=source['files'][1]['index'])) == ('页面.html', b'<script>throw 1</script>')
    assert download(path(source, index=source['files'][2]['index'])) == ('空白.txt', b'')
    state = api('/init')
    assert download(path(source))[1] == private_content
    assert api('/init') == state, 'download must not alter unread state, events or runs'
    selection = {'conversationId': gid, 'sources': [{'kind': 'message', 'conversation_id': gid, 'message_id': source['id'], 'revision': source['revision']}]}
    follow = send(cid, '继续讨论群文件', files=[], reference=selection)
    settled(cid)
    metadata_only(requests[-1])
    current = api(f'/conversations/{gid}')
    api(f'/conversations/{gid}/members', 'POST', {'requestId': str(uuid.uuid4()), 'expectedRevision': current['revision'], 'actorId': actor['id'], 'action': 'remove'})
    denied(path(source, actor['id']))
    assert download(path(source))[1] == private_content
    assert api(f'/messages/{follow["id"]}')['contextReference']['stale']
    for fmt in ['markdown', 'json']:
        exported = api(f'/conversations/{gid}/export?format={fmt}')['content']
        expected_name = attachment['name'].replace('.', '\\.') if fmt == 'markdown' else attachment['name']
        assert expected_name in exported and attachment['data'] not in exported
        with request_file(f'/conversations/{gid}/export?format={fmt}&includeImages=true') as response:
            with zipfile.ZipFile(io.BytesIO(response.read())) as archive:
                assert archive.namelist() == ['chat.md' if fmt == 'markdown' else 'chat.json'], 'this batch keeps generic export metadata-only'

    image = {'mime': 'image/png', 'data': base64.b64encode(png_bytes()).decode()}
    mixed = send(cid, '图片与文件一起发送', images=[image])
    assert mixed['images'][0]['index'] == 1 and mixed['files'][0]['index'] == 2
    assert download(path(mixed))[1] == private_content
    image_path = f'/messages/{mixed["id"]}/images/1?revision={mixed["revision"]}'
    assert api(image_path) == image
    denied(path(mixed, index=1))
    settled(cid)
    wire = json.dumps(requests[-1], ensure_ascii=False)
    assert attachment['name'] in wire and '文件内容尚未读取' in wire and attachment['data'] not in wire
    assert any(part.get('image_url', {}).get('url') == 'data:image/png;base64,' + image['data']
               for message in requests[-1]['messages'] if isinstance(message['content'], list) for part in message['content'])

    invalid = [[dict(attachment, name='../escape.txt')], [dict(attachment, name='C:\\escape.txt')],
               [dict(attachment, name='bad\nname.txt')], [dict(attachment, mime='text/plain\r\ninjected: x')],
               [dict(attachment, data='%%%')], [attachment] * 5,
               [dict(attachment, data=base64.b64encode(b'x' * (10 * 1024 * 1024 + 1)).decode())]]
    for files in invalid:
        denied(f'/conversations/{gid}/messages', 'POST', {'requestId': str(uuid.uuid4()), 'text': '', 'files': files})
    # Above the old 12 MiB HTTP ceiling and image-sized storage envelope.
    large_bytes = bytes(range(256)) * (6 * 1024 * 1024 // 256)
    large = {'name': '原样数据.bin', 'mime': 'application/octet-stream', 'data': base64.b64encode(large_bytes).decode()}
    saved = send(gid, files=[large, large])
    assert len(saved['files']) == 2
    for file in saved['files']:
        assert download(path(saved, index=file['index']))[1] == large_bytes
    denied(f'/conversations/{gid}/messages', 'POST', {'requestId': str(uuid.uuid4()), 'text': '', 'files': [large] * 4})
    print('PASS: generic file-only/mixed sends, exact downloads/headers, restart, idempotency, revisions/withdrawal, membership, empty/HTML files, metadata-only AI and group references, export placeholders, invalid payloads and 12 MiB file pair')
