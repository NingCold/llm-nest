"""Image messages: real storage, permissions and wire requests; mock provider only."""
import base64
import io
import json
import struct
import urllib.error
import uuid
import zlib
import zipfile
from urllib.parse import unquote

def png_bytes(width=4, height=3, compression=6):
    def chunk(kind, data):
        return struct.pack('>I', len(data)) + kind + data + struct.pack('>I', zlib.crc32(kind + data))
    pixels = b''.join(b'\x00' + bytes([48, 108, 220]) * width for _ in range(height))
    return b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', struct.pack('>IIBBBBB', width, height, 8, 2, 0, 0, 0)) + chunk(b'IDAT', zlib.compress(pixels, compression)) + chunk(b'IEND', b'')

def verify_images(api, contact, direct, group, history, wait, requests, stop, start, request_file):
    image = {'mime':'image/png', 'data':base64.b64encode(png_bytes()).decode()}
    actor = contact('图片验收')
    cid = direct(actor)
    def send(cid, text='', images=None, **extra):
        return api(f'/conversations/{cid}/messages', 'POST', dict(requestId=str(uuid.uuid4()), text=text, images=images if images is not None else [image], **extra))
    def denied(path, method='GET', body=None, codes=(400,403,409)):
        try: api(path, method, body)
        except urllib.error.HTTPError as error:
            assert error.code in codes, (error.code, error.read())
            return
        raise AssertionError('request unexpectedly succeeded: ' + path)
    def image_path(m, account=None):
        prefix = f'/ai-accounts/{account}/view' if account else ''
        return f'{prefix}/messages/{m["id"]}/images/{m["images"][0]["index"]}?revision={m["revision"]}'
    def download_path(m, account=None):
        return image_path(m, account).replace('?revision=', '/download?revision=')
    def download(path, media_type):
        with request_file(path) as response:
            assert response.headers['Cache-Control'] == 'no-store'
            assert response.headers['Content-Type'] == media_type
            assert response.headers['X-Content-Type-Options'] == 'nosniff'
            name = unquote(response.headers['Content-Disposition'].split("filename*=UTF-8''")[1])
            return name, response.read(), response.headers
    def settled(cid):
        return wait(lambda: (p := api(f'/conversations/{cid}/plans')) and all(x['status'] not in ('queued','running') for x in p))

    sent = send(cid)['message']
    assert sent['text'] == '' and sent['images'] == [{'index':0, 'mime':'image/png', 'byteLength':len(png_bytes())}]
    assert image['data'] not in json.dumps(sent)
    assert api(image_path(sent)) == image
    assert api(image_path(sent,actor['id'])) == image
    for scope in [None, actor['id']]:
        name, data, _ = download(download_path(sent,scope), 'image/png')
        assert name == f'{sent["id"]}-0.png' and data == png_bytes()
    other = contact('无权查看图片',model=None)
    denied(image_path(sent,other['id']))
    denied(download_path(sent,other['id']))
    settled(cid)
    wire = next(r for r in reversed(requests) if any(isinstance(m['content'],list) for m in r['messages']))
    assert any(p.get('image_url',{}).get('url') == 'data:image/png;base64,'+image['data'] for m in wire['messages'] if isinstance(m['content'],list) for p in m['content'])
    stop(); start()
    assert api(image_path(sent)) == image
    assert next(m for m in history(cid) if m['id']==sent['id'])['images'] == sent['images']
    edit_request={'requestId':str(uuid.uuid4()),'expectedRevision':sent['revision'],'text':'补充说明'}
    edited = api(f'/messages/{sent["id"]}/revise','POST',edit_request)
    assert edited['images'][0]['index']==1 and api(image_path(edited))==image
    denied(image_path(sent))
    denied(download_path(sent))
    assert download(download_path(edited),'image/png')[1] == png_bytes()
    empty_caption = api(f'/messages/{edited["id"]}/revise','POST',{'requestId':str(uuid.uuid4()),'expectedRevision':edited['revision'],'text':''})
    assert api(image_path(empty_caption))==image
    withdrawn = api(f'/messages/{sent["id"]}/withdraw','POST',{'requestId':str(uuid.uuid4()),'expectedRevision':empty_caption['revision']})
    assert withdrawn['images']==[]
    assert api(f'/messages/{sent["id"]}/revise','POST',edit_request)['status']=='withdrawn', 'old caption retry returns current state after withdrawal'
    denied(image_path(empty_caption))
    denied(download_path(empty_caption))
    bad_images = [[dict(image,mime='image/jpeg')],[dict(image,data='%%%')],[image]*5,[{'mime':'image/png','data':base64.b64encode(png_bytes(4097,1)).decode()}]]
    for invalid in bad_images:
        denied(f'/conversations/{cid}/messages','POST',{'requestId':str(uuid.uuid4()),'text':'bad','images':invalid})
    # File size is enforced independently from the 12 MiB HTTP JSON envelope.
    denied(f'/conversations/{cid}/messages','POST',{'requestId':str(uuid.uuid4()),'text':'','images':[dict(image,data='A'*(6*1024*1024))]})
    request={'requestId':str(uuid.uuid4()),'text':'provider-error','images':[image]}
    first=api(f'/conversations/{cid}/messages','POST',request)
    assert api(f'/conversations/{cid}/messages','POST',request)['message']['id']==first['message']['id']
    settled(cid)
    failed=next(m for m in reversed(history(cid)) if m['senderId']==actor['id'])
    assert '支持图片' in api(f'/runs/{failed["runId"]}')['error']['message']
    assert api(image_path(first['message']))==image

    # Group images can be explicitly carried to the same long-running private chat.
    gid=group('图片共同群',[actor])
    source=send(gid,'一起看图')['message']
    selection={'conversationId':gid,'sources':[{'kind':'message','conversation_id':gid,'message_id':source['id'],'revision':source['revision']}]}
    follow=send(cid,'image-reference-followup',[],reference=selection)['message']
    settled(cid)
    last=requests[-1]
    reference=next(m for m in last['messages'] if isinstance(m['content'],list) and any('用户明确引用的共同群话题' in p.get('text','') for p in m['content']))
    assert any(p.get('image_url',{}).get('url')=='data:image/png;base64,'+image['data'] for p in reference['content'])
    current=api(f'/conversations/{gid}')
    api(f'/conversations/{gid}/members','POST',{'requestId':str(uuid.uuid4()),'expectedRevision':current['revision'],'actorId':actor['id'],'action':'remove'})
    denied(image_path(source,actor['id']))
    denied(download_path(source,actor['id']))
    assert api(image_path(source))==image, 'remaining group members still read the image'
    assert api(f'/messages/{follow["id"]}')['contextReference']['stale']
    exported=api(f'/conversations/{cid}/export?format=json')['content']
    assert image['data'] not in exported and 'byteLength' in exported
    before = api('/init')
    for fmt in ['markdown', 'json']:
        name, data, headers = download(f'/conversations/{cid}/export?format={fmt}&includeImages=true','application/zip')
        assert name.endswith('.zip') and int(headers['X-LLMN-Message-Count']) == len(history(cid))
        with zipfile.ZipFile(io.BytesIO(data)) as archive:
            picture = f'images/{first["message"]["id"]}-1.png'
            transcript = 'chat.md' if fmt == 'markdown' else 'chat.json'
            assert set(archive.namelist()) == {picture,transcript}, archive.namelist()
            assert archive.read(picture) == png_bytes()
            text = archive.read(transcript).decode()
            assert picture in text and 'fixture-private-reasoning' not in text and image['data'] not in text
            assert f'images/{source["id"]}' not in text and f'images/{sent["id"]}' not in text
            if fmt == 'json':
                document = json.loads(text)
                assert document['version'] == 2
                assert next(m for m in document['messages'] if m['id']==first['message']['id'])['parts'][1]['filePath']==picture
            else: assert f'![图片]({picture})' in text
    assert api('/init') == before, 'binary export and image reads must be read-only'
    empty_group = group('空窗口导出',[other])
    with zipfile.ZipFile(io.BytesIO(download(f'/conversations/{empty_group}/export?format=json&includeImages=true','application/zip')[1])) as archive:
        assert archive.namelist()==['chat.json'] and json.loads(archive.read('chat.json'))['messages']==[]
    denied(f'/conversations/{cid}/export?format=json&includeImages=invalid',codes=(400,))
    # A valid request above Axum's default 2 MiB and above the old JSON-array
    # storage limit must succeed. Three such images cross the binary total cap.
    large={'mime':'image/png','data':base64.b64encode(png_bytes(1024,950,0)).decode()}
    large_group=group('大图存储验收',[other])
    large_message=send(large_group,'large image pair',[large,large])['message']
    assert len(large_message['images'])==2 and api(image_path(large_message))==large
    with zipfile.ZipFile(io.BytesIO(download(f'/conversations/{large_group}/export?format=json&includeImages=true','application/zip')[1])) as archive:
        pictures=[p for p in archive.namelist() if p.startswith('images/')]
        assert len(pictures)==2
        for path in pictures: assert archive.read(path)==base64.b64decode(large['data'])
    denied(f'/conversations/{large_group}/messages','POST',{'requestId':str(uuid.uuid4()),'text':'too large','images':[large]*3})
    print('PASS: image-only/mixed persistence and restart, revision/withdrawal, authorization, native model input, invalid files, provider rejection, scoped original downloads and read-only Markdown/JSON ZIP with exact image bytes (including 5.6 MiB pair)')
