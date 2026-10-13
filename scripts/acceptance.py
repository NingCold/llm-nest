"""Offline host and migration acceptance, using a temporary directory only.
Run after cargo build -p web-server. --serve keeps the fixture for browser QA.
"""
import argparse
import json
import os
from pathlib import Path
import socket
import subprocess
import tempfile
import time
import urllib.error
import urllib.request
import uuid

ROOT = Path(__file__).resolve().parents[1]

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--binary', type=Path)
    parser.add_argument('--dist', type=Path)
    parser.add_argument('--serve', action='store_true')
    args = parser.parse_args()
    work = Path(tempfile.mkdtemp(prefix='llmn-host-acceptance-'))
    data = work / 'data'; data.mkdir()
    config = work / 'llmn.toml'; config.write_text('# empty first run\n', encoding='utf-8')
    source_id, user_id, ai_id = (str(uuid.uuid4()) for _ in range(3))
    source = data / f'{source_id}.json'
    record = dict(version=1, id=source_id, title='历史聊天验收', model=None, metadata={'fixture':'only'},
                  created_at='2026-01-01T00:00:00Z', updated_at='2026-01-02T00:00:00Z',
                  messages=[dict(id=user_id, role='user', content='旧记录保留🙂'),
                            dict(id=ai_id, role='assistant', content='已经保留。')])
    original = json.dumps(record, ensure_ascii=False).encode()
    source.write_bytes(original)
    corrupt = data / f'{uuid.uuid4()}.json'; corrupt.write_text('{broken', encoding='utf-8')
    with socket.socket() as listener:
        listener.bind(('127.0.0.1', 0)); port = listener.getsockname()[1]
    base = f'http://127.0.0.1:{port}'
    env = dict(os.environ, LLMN_CONFIG=str(config), LLMN_DATA_DIR=str(data), LLMN_PORT=str(port))
    if args.dist: env['LLMN_WEB_DIST'] = str(args.dist.resolve())
    binary = (args.binary or ROOT / 'target/debug' / ('web-server.exe' if os.name == 'nt' else 'web-server')).resolve()
    log = (work/'server.log').open('w', encoding='utf-8')
    proc = None
    def api(path, method='GET', body=None, headers=None):
        request = urllib.request.Request(base+'/api'+path, method=method,
            data=None if body is None else json.dumps(body).encode(),
            headers=headers if headers is not None else {'X-LLMN-Client':'1','Content-Type':'application/json'})
        with urllib.request.urlopen(request, timeout=10) as response:
            value = response.read(); return json.loads(value) if value else None
    def rejected(path, status, **kwargs):
        try: api(path, **kwargs)
        except urllib.error.HTTPError as error:
            assert error.code == status, (path, error.code, error.read().decode()); return
        raise AssertionError(f'{path} unexpectedly accepted')
    def stop():
        if proc and proc.poll() is None: proc.kill(); proc.wait(timeout=10)
    def start():
        nonlocal proc
        proc = subprocess.Popen([str(binary)], cwd=ROOT, env=env, stdout=log, stderr=log,
            creationflags=subprocess.CREATE_NO_WINDOW if os.name=='nt' else 0)
        for _ in range(100):
            if proc.poll() is not None: raise AssertionError((work/'server.log').read_text(encoding='utf-8'))
            try: api('/social/init'); return
            except OSError: time.sleep(.1)
        raise AssertionError('startup timed out')
    try:
        start()
        assert api('/settings')['providers'] == []
        assert 'sessions' not in api('/settings') and 'config' not in api('/settings')
        assert api('/social/init')['conversations'] == []
        assert source.read_bytes() == original and corrupt.read_text(encoding='utf-8') == '{broken'
        rejected('/social/history-import', 400)
        corrupt.unlink()  # Test-owned malformed fixture only.
        for path, method in [('/init','GET'),('/config','PUT'),('/sessions','GET'),('/sessions','POST'),('/cancel','POST'),(f'/sessions/{source_id}/chat','POST')]:
            rejected(path,404,method=method,body={} if method in ('POST','PUT') else None)
        rejected('/settings',403,headers={})
        rejected('/settings',403,headers={'X-LLMN-Client':'1','Origin':'https://foreign.invalid'})
        rejected('/settings',403,headers={'X-LLMN-Client':'1','Host':'foreign.invalid'})
        print('PASS: empty configuration, no implicit JSON reads/writes, retired API 404 and local access guard')
        draft={'id':'fixture','protocol':'openai','baseUrl':'http://127.0.0.1:9/v1','apiKey':'fixture-only','models':[{'id':'chat'}]}
        assert api('/providers','POST',draft)[0]['id']=='fixture'
        before=config.read_bytes()
        rejected('/providers',400,method='POST',body={**draft,'id':'Invalid ID'})
        assert config.read_bytes()==before
        assert api('/providers/fixture','DELETE') == []
        preview=api('/social/history-import'); assert len(preview['sessions'])==1
        source.write_bytes(original+b'\n')
        rejected('/social/history-import',400,method='POST',body=preview)
        source.write_bytes(original)
        other = {**record, 'id':str(uuid.uuid4()), 'messages':[{'id':str(uuid.uuid4()),'role':'user','content':'Unselected fixture'}]}
        other_path = data/f'{other["id"]}.json'
        other_path.write_text(json.dumps(other),encoding='utf-8')
        result=api('/social/history-import','POST',preview)
        assert len(api('/social/init')['conversations'])==1
        other_path.unlink()
        cid=result['conversations'][0]; assert result['imported']==1
        assert api('/social/history-import','POST',preview)['already_imported']==1
        assert source.read_bytes()==original
        conversation=api(f'/social/conversations/{cid}')
        assert conversation['legacy']
        history=api(f'/social/conversations/{cid}/messages')['messages']
        assert [m['id'] for m in history]==[user_id,ai_id]
        rejected(f'/social/conversations/{cid}/messages',400,method='POST',body={'requestId':str(uuid.uuid4()),'text':'no write','mentions':[]})
        self_id=api('/social/init')['selfId']
        stop();start()
        assert api('/social/init')['selfId']==self_id
        assert any(c['id']==cid for c in api('/social/init')['conversations'])
        assert api('/settings')['providers']==[]
        assert source.read_bytes()==original
        print('PASS: provider transactions, selected/checksummed/idempotent import, stable IDs, readonly history and restart')
        result=subprocess.run([str(binary),'--llmn-tool-worker','add'],input=b'{"a":2,"b":3}',capture_output=True,env={},timeout=10,check=True,
            creationflags=subprocess.CREATE_NO_WINDOW if os.name=='nt' else 0)
        assert json.loads(result.stdout)=={'result':{'sum':5}}
        print('PASS: isolated compiled tool foundation (not exposed to social AI)')
        if args.serve:
            # Leave a fresh unimported record for testing the actual import controls.
            record.update(id=str(uuid.uuid4()), title='待导入的旅行计划', messages=[dict(id=str(uuid.uuid4()),role='user',content='周末去海边走走🙂')])
            (data/f'{record["id"]}.json').write_text(json.dumps(record,ensure_ascii=False),encoding='utf-8')
            print(f'QA_READY {base} STOP_FILE={work / "STOP"}',flush=True)
            while not (work/'STOP').exists(): time.sleep(.5)
    finally:
        stop();log.close()
    print(f'Evidence: {work}')

if __name__=='__main__': main()
