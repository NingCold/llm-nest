"""Offline full-chain acceptance. Build web-server first; no API keys/network needed.
python scripts/acceptance.py [--serve] [--dist PATH]
Uses fresh temp config/storage, a loopback mock OpenAI provider, the real server,
real tool subprocesses and disk records. --serve keeps the fixture up for GUI QA.
"""
import argparse
import concurrent.futures
import http.server
import json
import os
from pathlib import Path
import socket
import subprocess
import tempfile
import threading
import time
import urllib.request
import urllib.error

ROOT = Path(__file__).resolve().parents[1]
REQUESTS = []

class Provider(http.server.BaseHTTPRequestHandler):
    def log_message(self, *_): pass
    def do_POST(self):
        req = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
        REQUESTS.append(req)
        messages = req['messages']
        user = next((m.get('content', '') for m in reversed(messages) if m['role'] == 'user'), '')
        if isinstance(user, list): user = ' '.join(p.get('text', '') for p in user)
        if user == 'error':
            self.send_response(429); self.end_headers()
            self.wfile.write(b'{"error":{"message":"acceptance rate limit"}}'); return
        self.send_response(200)
        self.send_header('Content-Type', 'text/event-stream'); self.end_headers()
        def emit(data):
            data = dict(id='chatcmpl-fixture', object='chat.completion.chunk', created=0, model='chat', **data)
            self.wfile.write(('data: '+json.dumps(data, ensure_ascii=False)+'\n\n').encode())
            self.wfile.flush()
        def chunk(delta, finish=None): emit({'choices':[{'index':0,'delta':delta,'finish_reason':finish}]})
        try:
            if user == 'tool' and messages[-1]['role'] != 'tool':
                chunk({'tool_calls':[{'index':0,'id':'call-acceptance','type':'function','function':{'name':'add','arguments':'{"a":2,"b":3}'}}]})
                chunk({}, 'tool_calls')
            else:
                chunk({'reasoning_content':'验收思考🙂'})
                chunk({'content':'结果：5' if user == 'tool' else '你好，验收🙂'})
                if user in ('slow', 'crash', 'disconnect'):
                    for _ in range(100):
                        time.sleep(0.1); chunk({'content':'…'})
                if user == 'eof': return
                chunk({}, 'stop')
            emit({'choices':[], 'usage':{'prompt_tokens':10,'completion_tokens':5,'total_tokens':15}})
            self.wfile.write(b'data: [DONE]\n\n'); self.wfile.flush()
        except (BrokenPipeError, ConnectionResetError, ConnectionAbortedError): pass

def free_port():
    with socket.socket() as s:
        s.bind(('127.0.0.1', 0)); return s.getsockname()[1]

def main():
    parser = argparse.ArgumentParser(); parser.add_argument('--serve', action='store_true'); parser.add_argument('--dist', type=Path)
    args = parser.parse_args()
    work = Path(tempfile.mkdtemp(prefix='llmn-acceptance-'))
    provider = http.server.ThreadingHTTPServer(('127.0.0.1', 0), Provider)
    threading.Thread(target=provider.serve_forever, daemon=True).start()
    port = free_port(); base = f'http://127.0.0.1:{port}'
    config = work/'llmn.toml'
    config.write_text(f'# acceptance fixture\n[providers.test]\nprotocol="openai"\nbase_url="http://127.0.0.1:{provider.server_port}/v1"\napi_key="fixture-only"\n[providers.test.models.chat]\nmodel="chat"\n', encoding='utf-8')
    env = os.environ.copy(); env.update(LLMN_CONFIG=str(config), LLMN_DATA_DIR=str(work/'sessions'), LLMN_PORT=str(port))
    if args.dist: env['LLMN_WEB_DIST'] = str(args.dist.resolve())
    exe = ROOT/'target/debug'/('web-server.exe' if os.name == 'nt' else 'web-server')
    log = (work/'server.log').open('w', encoding='utf-8')
    proc = None
    def request(path, method='GET', data=None):
        body = None if data is None else json.dumps(data).encode()
        return urllib.request.urlopen(urllib.request.Request(base+'/api'+path, data=body, method=method, headers={'X-LLMN-Client':'1','Content-Type':'application/json'}), timeout=20)
    def api(path, method='GET', data=None):
        with request(path, method, data) as r:
            body = r.read(); return json.loads(body) if body else None
    def start():
        nonlocal proc
        proc = subprocess.Popen([str(exe)], cwd=ROOT, env=env, stdout=log, stderr=log, creationflags=subprocess.CREATE_NO_WINDOW if os.name == 'nt' else 0)
        for _ in range(100):
            if proc.poll() is not None: raise RuntimeError((work/'server.log').read_text())
            try: api('/init'); return
            except (OSError, urllib.error.URLError): time.sleep(0.1)
        raise AssertionError('server did not start')
    def stop():
        if proc and proc.poll() is None: proc.kill(); proc.wait(timeout=10)
    def new(): return api('/sessions','POST')['id']
    def history(sid): return api(f'/sessions/{sid}/messages')
    def payload(sid, text, **kw): return dict(sessionId=sid, messageId='wire-fixture', input=text, model={'provider':'test','model':'chat'}, temperature=0.3, maxTokens=1234, **kw)
    def chat(sid, text, **kw):
        with request(f'/sessions/{sid}/chat','POST',payload(sid,text,**kw)) as r:
            return [json.loads(line[6:]) for line in r if line.startswith(b'data: ')]
    def wait_for(fn):
        deadline = time.monotonic()+8
        while time.monotonic()<deadline:
            if fn(): return
            time.sleep(.05)
        raise AssertionError('condition timed out')
    try:
        start()
        init = api('/init'); assert init['providers'][0]['id']=='test'
        settings = dict(init['config'], temperature=0.3, maxTokens=1234)
        api('/config','PUT',settings)
        before = config.read_text()
        for patch in ({'temperature':3},{'maxTokens':0},{'currentModel':{'provider':'missing','model':'bad'}}):
            try: api('/config','PUT',dict(settings, **patch)); raise AssertionError('invalid settings accepted')
            except urllib.error.HTTPError as e: assert 400 <= e.code < 500
        assert config.read_text()==before
        sid = new(); events = chat(sid,'hello'); assert events[-1]['type']=='finished',events
        msgs = history(sid); assert msgs[-1]['content']=='你好，验收🙂'; assert msgs[-1]['reasoning']=='验收思考🙂'
        assert msgs[-1]['usage']['total_tokens']==15
        assert REQUESTS[-1]['temperature']==0.3 and REQUESTS[-1]['max_tokens']==1234
        ids = [m['id'] for m in msgs]; assert len(set(ids))==len(ids)
        api(f'/sessions/{sid}/messages/{ids[-1]}','PATCH',{'feedback':'up','revision':msgs[-1]['revision']})
        msgs = history(sid); assert msgs[-1]['feedback']=='up'
        events=chat(sid,'edited',edit={'userId':msgs[0]['id'],'expectedMessageCount':len(msgs),'expectedRevision':msgs[0]['revision']})
        assert events[-1]['type']=='finished'; assert len(history(sid))==2
        stale=chat(sid,'stale',edit={'userId':msgs[0]['id'],'expectedMessageCount':len(msgs),'expectedRevision':msgs[0]['revision']})
        assert stale[-1]['type']=='error'; assert history(sid)[0]['content']=='edited'
        tool_sid=new(); events=chat(tool_sid,'tool'); assert events[-1]['type']=='finished', events
        tool_msgs=history(tool_sid); assert any(t['kind']=='result' and not t.get('isError') for m in tool_msgs for t in m.get('tools',[])),tool_msgs
        assert tool_msgs[-1]['content']=='结果：5'
        for text in ('error','eof'):
            failed=new(); events=chat(failed,text); assert events[-1]['type']=='error',events
            assert history(failed)[-1]['status']=='error'
            assert chat(failed,'retry')[-1]['type']=='finished'
        slow=new()
        with concurrent.futures.ThreadPoolExecutor() as pool:
            future=pool.submit(chat,slow,'slow')
            wait_for(lambda: any(r['messages'][-1].get('content')=='slow' for r in REQUESTS))
            try: chat(slow,'duplicate'); raise AssertionError('concurrent chat accepted')
            except urllib.error.HTTPError: pass
            api('/cancel','POST',{'sessionId':slow})
            assert future.result()[-1]['type']=='cancelled'
        assert history(slow)[-1]['status']=='cancelled'
        dropped=new()
        stream=request(f'/sessions/{dropped}/chat','POST',payload(dropped,'disconnect'))
        while True:
            line=stream.readline()
            if line.startswith(b'data: ') and json.loads(line[6:])['type']=='delta': break
        stream.close()
        wait_for(lambda: history(dropped)[-1]['status']=='cancelled')
        crash=new(); stream=request(f'/sessions/{crash}/chat','POST',payload(crash,'crash'))
        wait_for(lambda: bool(((json.loads((work/'sessions'/f'{crash}.json').read_text(encoding='utf-8')).get('run') or {}).get('partial') or {}).get('reasoning')))
        stop(); stream.close(); start()
        restored=history(crash); assert restored[-1]['status'] in ('error','cancelled'),restored
        assert restored[-1]['content'].startswith('你好，验收🙂') and restored[-1]['reasoning']=='验收思考🙂'
        saved=api('/init')['config']; assert saved==settings,(saved,settings)
        snapshot=restored; stop(); start(); assert history(crash)==snapshot
        # Validate OS single writer lock without disturbing the running server.
        other=subprocess.run([str(exe)],cwd=ROOT,env=env,capture_output=True,timeout=15,creationflags=subprocess.CREATE_NO_WINDOW if os.name=='nt' else 0)
        assert other.returncode!=0 and b'lock' in other.stderr.lower(),other.stderr
        api(f'/sessions/{sid}','PATCH',{'title':'验收重命名'})
        assert any(s['title']=='验收重命名' for s in api('/sessions'))
        api(f'/sessions/{sid}','DELETE'); assert all(s['id']!=sid for s in api('/sessions'))
        # Verify provider changes persist without removing GUI settings.
        api('/providers','POST',{'id':'second','protocol':'openai','baseUrl':f'http://127.0.0.1:{provider.server_port}/v1','apiKey':'fixture-only','models':[{'id':'chat'}]})
        api('/providers/second','DELETE'); assert api('/init')['config']==settings
        print('PASS: settings/restart/validation, CRUD, Unicode+reasoning+usage, edit+stale guard, feedback, isolated tools, provider errors+EOF+retry, duplicate run, cancel+disconnect, crash recovery+idempotency, OS lock, provider edits',flush=True)
        print(f'Fixture: {work}\nGUI: {base}',flush=True)
        if args.serve:
            print('Serving fixture until interrupted.',flush=True)
            while not (work/'STOP').exists(): time.sleep(0.25)
    finally:
        stop(); provider.shutdown(); log.close()

if __name__=='__main__': main()
