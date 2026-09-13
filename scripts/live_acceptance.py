"""Explicit opt-in live provider acceptance; sends synthetic prompts and consumes API tokens.
python scripts/live_acceptance.py --config config/config.example.toml
Reads only providers.chatecnu; credentials stay in environment, never in artifacts.
"""
import argparse
import json
import os
from pathlib import Path
import socket
import subprocess
import tempfile
import time
import tomllib
import urllib.request
import uuid

ROOT = Path(__file__).resolve().parents[1]

def toml_value(value):
    if isinstance(value, dict):
        return '{ ' + ', '.join(f'{json.dumps(k)} = {toml_value(v)}' for k,v in value.items()) + ' }'
    if isinstance(value, list): return '[' + ', '.join(map(toml_value, value)) + ']'
    return json.dumps(value, ensure_ascii=False)

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--config', type=Path, default=ROOT/'config/config.example.toml')
    parser.add_argument('--cases', default='basic,reasoning,tool,cancel')
    parser.add_argument('--serve', action='store_true')
    parser.add_argument('--dist', type=Path)
    args = parser.parse_args()
    provider = tomllib.loads(args.config.read_text(encoding='utf-8'))['providers']['chatecnu']
    credential = provider.get('api_key', {'env':'CHATECNU_API_KEY'})
    env_name = credential.get('env') if isinstance(credential,dict) else None
    secret = os.environ.get(env_name,'') if env_name else credential
    if not secret and env_name and os.name=='nt':
        import winreg
        try:
            with winreg.OpenKey(winreg.HKEY_CURRENT_USER,'Environment') as key:
                secret = winreg.QueryValueEx(key,env_name)[0]
        except FileNotFoundError: pass
    if not secret: raise RuntimeError('Referenced API key is unavailable; no live requests sent.')
    provider['api_key'] = {'env':'LLMN_LIVE_ACCEPTANCE_KEY'}
    provider['default_model'] = 'ecnu-max'
    # Bound live test requests without changing user configuration.
    provider['timeout_ms'] = 120000
    work = Path(tempfile.mkdtemp(prefix='llmn-live-ecnu-'))
    config = work/'llmn.toml'
    config.write_text('[providers.chatecnu]\n'+'\n'.join(f'{k} = {toml_value(v)}' for k,v in provider.items())+'\n',encoding='utf-8')
    env = os.environ.copy()
    with socket.socket() as sock:
        sock.bind(('127.0.0.1',0)); port=sock.getsockname()[1]
    base=f'http://127.0.0.1:{port}'
    env.update(LLMN_CONFIG=str(config),LLMN_DATA_DIR=str(work/'sessions'),LLMN_PORT=str(port),LLMN_LIVE_ACCEPTANCE_KEY=secret)
    if args.dist: env['LLMN_WEB_DIST']=str(args.dist.resolve())
    exe=ROOT/'target/debug'/('web-server.exe' if os.name=='nt' else 'web-server')
    proc=None
    report=[]
    log=(work/'server.log').open('w',encoding='utf-8')
    def clean(value): return str(value).replace(secret,'<redacted>')
    def request(path,method='GET',data=None):
        return urllib.request.urlopen(urllib.request.Request(base+'/api'+path,method=method,data=None if data is None else json.dumps(data).encode(),headers={'X-LLMN-Client':'1','Content-Type':'application/json','Origin':base}),timeout=135)
    def api(path,method='GET',data=None):
        with request(path,method,data) as response:
            data=response.read();return json.loads(data) if data else None
    def start():
        nonlocal proc
        proc=subprocess.Popen([str(exe)],cwd=ROOT,env=env,stdout=log,stderr=log,creationflags=subprocess.CREATE_NO_WINDOW if os.name=='nt' else 0)
        for _ in range(100):
            if proc.poll() is not None: raise RuntimeError('Server startup failed; inspect sanitized fixture log.')
            try: api('/init');return
            except OSError: time.sleep(.1)
        raise RuntimeError('Server startup timed out')
    def stop():
        if proc and proc.poll() is None: proc.terminate();proc.wait(timeout=10)
    def messages(sid): return api(f'/sessions/{sid}/messages')
    def chat(sid,prompt,effort,max_tokens=1024,cancel_after_delta=False):
        params={'sessionId':sid,'messageId':str(uuid.uuid4()),'input':prompt,'model':{'provider':'chatecnu','model':'ecnu-max','reasoning_effort':effort},'temperature':0.2,'maxTokens':max_tokens}
        events=[]; started=time.monotonic();cancelled=False
        with request(f'/sessions/{sid}/chat','POST',params) as response:
            for line in response:
                if not line.startswith(b'data: '):continue
                event=json.loads(line[6:]);events.append(event)
                if cancel_after_delta and event['type'] in ('delta','reasoning_delta') and not cancelled:
                    api('/cancel','POST',{'sessionId':sid});cancelled=True
        history=messages(sid)
        last=history[-1] if history else {}
        result={'terminal':events[-1]['type'] if events else None,'error':next((e['error'] for e in events if e['type']=='error'),None),'seconds':round(time.monotonic()-started,2),'event_counts':{t:sum(e['type']==t for e in events) for t in sorted({e['type'] for e in events})},'answer':last.get('content',''),'reasoning_chars':len(last.get('reasoning','')),'status':last.get('status'),'usage':last.get('usage'),'timings':last.get('timings'),'tools':[t for m in history for t in m.get('tools',[])]}
        return result
    try:
        start()
        init=api('/init')
        selected=next(p for p in init['providers'] if p['id']=='chatecnu')
        model=next(m for m in selected['models'] if m['id']=='ecnu-max')
        print(json.dumps({'fixture':str(work),'provider':'chatecnu','model':model},ensure_ascii=False),flush=True)
        cases={
            'basic':('请只回答“连接成功🙂”，不要调用任何工具。','off',128),
            'reasoning':('请推算 37 × 49 的结果，只在最终答案写出结果；不要调用工具。','low',1024),
            'reasoning_max':('请推算 23 × 47 的结果，只在最终答案写出结果；不要调用工具。','max',1024),
            'tool':('这是工具集成测试。请务必调用 add 工具计算 123 + 456，不要心算。收到工具结果后，用一句中文报告结果。','high',1536),
            'cancel':('请写一篇至少两千字的虚构太空冒险故事，从第一章开始，直接写正文，不要调用工具。','off',1024),
        }
        for name in filter(None,args.cases.split(',')):
            sid=api('/sessions','POST')['id']
            prompt,effort,cap=cases[name]
            result=chat(sid,prompt,effort,cap,name=='cancel')
            result.update(case=name,session_id=sid,effort=effort)
            result['passed']=result['terminal']==('cancelled' if name=='cancel' else 'finished')
            if name=='basic':result['passed'] &= '连接成功' in result['answer']
            if name=='reasoning':result['passed'] &= '1813' in result['answer'] and result['reasoning_chars']>0
            if name=='reasoning_max':result['passed'] &= '1081' in result['answer'] and result['reasoning_chars']>0
            if name=='tool':result['passed'] &= '579' in result['answer'] and any(t['kind']=='result' and t['name']=='add' and not t.get('isError') for t in result['tools'])
            report.append(result)
            print(clean(json.dumps(result,ensure_ascii=False)),flush=True)
            if name=='cancel' and result['passed']:
                follow=chat(sid,'请只回答“恢复成功”，不要调用工具。','off',128)
                follow.update(case='after_cancel',session_id=sid,passed=follow['terminal']=='finished' and '恢复成功' in follow['answer'])
                report.append(follow);print(clean(json.dumps(follow,ensure_ascii=False)),flush=True)
        before={s['id']:messages(s['id']) for s in api('/sessions')}
        stop();start()
        assert before=={s['id']:messages(s['id']) for s in api('/sessions')},'Restart history mismatch'
        print('PASS: live history, reasoning, tools and statuses survive process restart unchanged.',flush=True)
        if args.serve:
            print(f'GUI: {base} (create STOP inside fixture directory to finish)',flush=True)
            while not (work/'STOP').exists(): time.sleep(0.25)
    finally:
        stop();log.close()
        (work/'report.json').write_text(clean(json.dumps(report,ensure_ascii=False,indent=2)),encoding='utf-8')
        log_path=work/'server.log';log_path.write_text(clean(log_path.read_text(encoding='utf-8')),encoding='utf-8')
    if any(not row['passed'] for row in report): raise SystemExit(1)

if __name__=='__main__':main()
