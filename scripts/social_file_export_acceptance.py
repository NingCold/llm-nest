"""File archives use one window snapshot and carry the selected original bytes."""
import base64
import io
import json
import re
import urllib.error
import uuid
import zipfile
from pathlib import PurePosixPath
from urllib.parse import unquote

from social_image_acceptance import png_bytes


def verify_file_exports(api, contact, direct, group, request_file):
    actor = contact('文件导出验收', model=None)
    cid = group('附件导出 中文', [actor])
    name = '说明 [最终](甲) #100%.txt'
    raw = b'file-export-fixture\x00\xff\x01'
    files = [
        {'name': name, 'mime': 'text/plain', 'data': base64.b64encode(raw).decode()},
        {'name': name, 'mime': 'text/plain', 'data': ''},
        {'name': '页面.html', 'mime': 'text/html', 'data': base64.b64encode(b'<script>fixture()</script>').decode()},
    ]
    image = {'mime': 'image/png', 'data': base64.b64encode(png_bytes()).decode()}

    def send(window, text, attachments, images=None, **extra):
        return api(f'/conversations/{window}/messages', 'POST', dict(
            requestId=str(uuid.uuid4()), text=text, files=attachments,
            images=images or [], **extra))['message']

    first = send(cid, '文字、图片、文件同一条消息', files, [image])
    second = send(cid, '另一条同名文件', [files[0]])
    first = api(f'/messages/{first["id"]}/revise', 'POST', {
        'requestId': str(uuid.uuid4()), 'expectedRevision': first['revision'], 'text': '最新修订'})
    expected_files = {}
    for message in [first, second]:
        for entry, source in zip(message['files'], files if message is first else files[:1]):
            expected_files[f'files/{message["id"]}-{entry["index"]}/{source["name"]}'] = base64.b64decode(source['data'])
    expected_images = {f'images/{first["id"]}-{first["images"][0]["index"]}.png': png_bytes()}
    before = api('/init')

    def read_export(window, fmt, include_images=False, include_files=False):
        suffix = f'?format={fmt}&includeImages={str(include_images).lower()}&includeFiles={str(include_files).lower()}'
        with request_file(f'/conversations/{window}/export{suffix}') as response:
            payload = response.read()
            if not include_images and not include_files:
                wrapper = json.loads(payload)
                return wrapper['content'], {}
            assert response.headers['Content-Type'] == 'application/zip'
            assert response.headers['Cache-Control'] == 'no-store'
            assert response.headers['X-Content-Type-Options'] == 'nosniff'
            assert response.headers['Content-Disposition'].startswith('attachment;')
            with zipfile.ZipFile(io.BytesIO(payload)) as archive:
                assert archive.testzip() is None
                paths = archive.namelist()
                assert len(paths) == len(set(paths)), 'duplicate attachment names must not overwrite'
                assert all(not PurePosixPath(p).is_absolute() and '..' not in PurePosixPath(p).parts and '\\' not in p for p in paths)
                document = archive.read('chat.md' if fmt == 'markdown' else 'chat.json').decode('utf-8')
                entries = {p: archive.read(p) for p in paths if p.startswith(('files/', 'images/'))}
                return document, entries

    for fmt in ['markdown', 'json']:
        for include_images, include_files in [(False, False), (True, False), (False, True), (True, True)]:
            document, entries = read_export(cid, fmt, include_images, include_files)
            expected = (expected_files if include_files else {}) | (expected_images if include_images else {})
            assert entries == expected, (fmt, include_images, include_files, entries.keys())
            assert '最新修订' in document and '文字、图片、文件同一条消息' not in document
            assert files[0]['data'] not in document and '<script>fixture()</script>' not in document
            if fmt == 'json':
                parsed = json.loads(document)
                assert parsed['version'] == (3 if include_files else 2 if include_images else 1)
                assert len(parsed['messages']) == 2
                paths = []
                for message in parsed['messages']:
                    for part in message['parts']:
                        assert 'data' not in part
                        if 'filePath' in part:
                            paths.append(part['filePath'])
                assert set(paths) == set(expected)
            else:
                paths = [unquote(path) for path in re.findall(r'\]\(([^)]+)\)', document)]
                assert set(paths) == set(expected)
                if include_files:
                    assert '%20%5B' in document and '%23' in document and '%25' in document
    assert api('/init') == before, 'export must not change list state, events or runs'

    # A continued group topic still exports just the direct window's own attachments.
    private = direct(actor)
    selection = {'conversationId': cid, 'sources': [{'kind': 'message', 'conversation_id': cid,
                 'message_id': first['id'], 'revision': first['revision']}]}
    continuation = send(private, '只导出本窗口', [], reference=selection)
    document, entries = read_export(private, 'json', True, True)
    assert entries == {} and files[0]['data'] not in document and name not in document
    assert continuation['id'] in document and first['id'] in document

    api(f'/messages/{second["id"]}/withdraw', 'POST', {
        'requestId': str(uuid.uuid4()), 'expectedRevision': second['revision']})
    document, entries = read_export(cid, 'json', False, True)
    assert set(entries) == {p for p in expected_files if not p.startswith(f'files/{second["id"]}-')}
    withdrawn = next(m for m in json.loads(document)['messages'] if m['id'] == second['id'])
    assert withdrawn['status'] == 'withdrawn' and withdrawn['parts'] == []

    empty = group('空窗口导出', [actor])
    document, entries = read_export(empty, 'json', False, True)
    assert entries == {} and json.loads(document)['messages'] == []
    # Current-member and post-removal permission checks live in the storage tests;
    # the local product only offers AI membership changes, not human group exit.
    try:
        read_export(str(uuid.uuid4()), 'json', True, True)
    except urllib.error.HTTPError as error:
        assert error.code in (403, 404), (error.code, error.read())
    else:
        raise AssertionError('unknown conversation must not produce an archive')
    print('PASS: file ZIP option combinations, current snapshot, exact bytes, repeated/Unicode names, empty files/windows, encoded links, no binary JSON, withdrawn/denied export and cross-window isolation')
