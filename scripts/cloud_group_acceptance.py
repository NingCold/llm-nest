"""Exercise cloud groups with two humans and two real Agent processes.

Uses an existing disposable cloud_acceptance.py account file and a local mock
model. Creates a test group and two AI accounts; never reads a real model key.
An optional SSH client checks the same group from another device. --serve-ui
keeps the mock nodes online until WORKDIR/stop is created for browser checks.
"""
import argparse
import base64
import json
import os
from pathlib import Path
import re
import subprocess
import threading
import time
from urllib.parse import quote
import uuid
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

from cloud_acceptance import Client, png, remote
from agent_acceptance import wait_for


def uid():
    return str(uuid.uuid4())


class Model(BaseHTTPRequestHandler):
    calls = []
    active = 0
    peak = 0
    lock = threading.Lock()
    hold = False
    release = threading.Event()

    def log_message(self, *args):
        pass

    def do_POST(self):
        payload = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
        assert self.path == '/v1/chat/completions' and payload['stream']
        actor = payload['model']
        assert actor in ('fixture-a', 'fixture-b')
        with Model.lock:
            Model.active += 1
            Model.peak = max(Model.peak, Model.active)
            Model.calls.append(payload)
        self.send_response(200)
        self.send_header('Content-Type', 'text/event-stream')
        self.send_header('Connection', 'close')
        self.end_headers()

        def emit(body):
            value = json.dumps(body, ensure_ascii=False) if isinstance(body, dict) else body
            self.wfile.write(('data: '+value+'\n\n').encode())
            self.wfile.flush()

        try:
            emit({'id': uid(), 'choices': [{'index': 0, 'delta': {'content': f'{actor} 群聊模拟回复'}, 'finish_reason': None}]})
            time.sleep(.7)
            if Model.hold:
                Model.release.wait(45)
            emit({'id': uid(), 'choices': [{'index': 0, 'delta': {'content': '完成'}, 'finish_reason': None}]})
            emit({'id': uid(), 'choices': [{'index': 0, 'delta': {}, 'finish_reason': 'stop'}],
                  'usage': {'prompt_tokens': 60, 'completion_tokens': 8, 'total_tokens': 68}})
            emit('[DONE]')
        except (BrokenPipeError, ConnectionResetError, ConnectionAbortedError):
            pass  # Membership changes deliberately interrupt fixture streams.
        finally:
            with Model.lock:
                Model.active -= 1


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', required=True)
    parser.add_argument('--accounts', required=True)
    parser.add_argument('--ca', required=True)
    parser.add_argument('--workdir', required=True)
    parser.add_argument('--client-host')
    parser.add_argument('--serve-ui', action='store_true')
    args = parser.parse_args()
    directory = Path(args.workdir).resolve()
    directory.mkdir(parents=True, exist_ok=False)
    binary = str(Path(args.binary).resolve())
    ca = str(Path(args.ca).resolve())
    saved = json.loads(Path(args.accounts).read_text(encoding='utf-8'))
    humans = saved['accounts'][:3]
    assert len(humans) == 3, 'Three disposable accounts are needed for permission checks'
    clients = [Client(saved['url'], ca) for _ in humans]
    for api, human in zip(clients, humans):
        api.request('/api/auth/login', 'POST', {k: human[k] for k in ('email', 'password')})
    a, b, outsider = clients
    model = ThreadingHTTPServer(('127.0.0.1', 0), Model)
    threading.Thread(target=model.serve_forever, daemon=True).start()
    process_options = {'creationflags': subprocess.CREATE_NO_WINDOW} if os.name == 'nt' else {}
    env = {**os.environ, 'LLMN_GROUP_FIXTURE_KEY': 'mock-group-key'}
    actors, processes, logs = [], [], []
    suffix = uid()[:6]
    try:
        for index, api in enumerate((a, b)):
            ai = api.request('/api/ai', 'POST', {'requestId': uid(), 'name': f'群聊验收{"甲" if index == 0 else "乙"}-{suffix}', 'bio': '模拟模型，仅用于群聊验收'})
            actors.append(ai)
            node = directory / f'node-{index}'
            subprocess.run([binary, 'init', str(node)], check=True, capture_output=True, **process_options)
            key = f'fixture-{"a" if index == 0 else "b"}'
            (node/'node.toml').write_text(f'''server = {json.dumps(saved['url'])}
ca_file = {json.dumps(ca.replace(chr(92), '/'))}
node_id = "{uid()}"
node_name = "群聊验收节点{index + 1}"
daily_calls = 100
daily_tokens = 3276800
[profile]
revision = "1"
persona = "这是独立群聊验收角色 {key}。"
max_tokens = 256
[profile.selection]
provider = "fixture"
model = "{key}"
[providers.fixture]
protocol = "openai"
base_url = "http://127.0.0.1:{model.server_port}/v1"
api_key = {{ env = "LLMN_GROUP_FIXTURE_KEY" }}
[providers.fixture.models.{key}]
model = "{key}"
context_window = 32768
max_tokens = 512
image_input = true
''', encoding='utf-8')
            log = open(node/'run.log', 'ab')
            logs.append(log)
            pair_path = node/'pair.log'
            with pair_path.open('wb') as pair_log:
                pair = subprocess.Popen([binary, 'pair', str(node)], stdout=pair_log, stderr=log, env=env, **process_options)
            processes.append(pair)

            def code():
                match = re.search(r'pairing code (\S+)', pair_path.read_text(encoding='utf-8'))
                assert match or pair.poll() is None, 'Pairing exited; inspect private fixture log'
                return match.group(1) if match else None

            ai = api.request(f"/api/ai/{ai['account']['id']}/node", 'POST', {
                'requestId': uid(), 'pairingCode': wait_for(code, bool), 'expectedBindingVersion': ai['bindingVersion']})
            actors[index] = ai
            assert pair.wait(timeout=30) == 0
            processes.append(subprocess.Popen([binary, 'run', str(node)], stdout=log, stderr=log, env=env, **process_options))
            wait_for(lambda: api.request('/api/ai'), lambda rows: any(r['account']['id'] == ai['account']['id'] and r['node']['online'] for r in rows))
        ai0, ai1 = [item['account']['id'] for item in actors]
        group = a.request('/api/chat/groups', 'POST', {'requestId': uid(), 'name': f'群聊闭环验收-{suffix}',
            'memberIds': [humans[1]['account']['id'], ai0, ai1]})
        cid = group['id']
        path = f'/api/chat/conversations/{cid}/messages'

        def latest(api=a):
            return next(c for c in api.request('/api/chat/init')['conversations'] if c['id'] == cid)

        def send(text, mentions=None, reply=None, api=a, **extra):
            return api.request(path, 'POST', {'requestId': uid(), 'text': text, 'mentions': mentions or [], 'replyTo': reply, 'images': [], 'files': [], **extra})

        def states(mid):
            return next(m for m in a.request(path)['messages'] if m['id'] == mid)['aiResponses']

        def completed(message, count):
            def read():
                values = states(message['id'])
                assert len(values) == count, values
                assert all(v['status'] in ('queued', 'running', 'completed') for v in values), values
                return values
            return wait_for(read, lambda values: all(v['status'] == 'completed' for v in values))

        first = send('普通消息与纯文本 @名字 不应该触发 AI')
        assert first['aiResponses'] == []
        Model.hold = True
        Model.release.clear()
        both = send('请两位按顺序介绍自己', [ai0, ai1])
        wait_for(lambda: len(Model.calls), lambda count: count == 1)
        future_text = '后来的普通真人消息不属于前一条触发上下文'
        send(future_text)
        Model.hold = False
        Model.release.set()
        completed(both, 2)
        assert [c['model'] for c in Model.calls] == ['fixture-a', 'fixture-b']
        assert Model.peak == 1, 'Nodes overlapped model calls in one conversation'
        second_input = json.dumps(Model.calls[1]['messages'], ensure_ascii=False)
        assert 'fixture-a 群聊模拟回复完成' in second_input
        assert group['name'] in second_input and humans[0]['account']['name'] in second_input
        assert future_text not in second_input
        reply0 = next(m for m in a.request(path)['messages'] if m['senderId'] == ai0 and m['status'] == 'committed')
        completed(send('单独回复第一位 AI', reply=reply0['id']), 1)
        assert len(Model.calls) == 3 and Model.calls[-1]['model'] == 'fixture-a'
        print('PASS: two actual nodes, ordered mentions, previous AI context and structured reply', flush=True)

        attachment = send('群附件', images=[{'mime': 'image/png', 'data': base64.b64encode(png()).decode()}],
            files=[{'name': '群附件.txt', 'mime': 'text/plain', 'data': base64.b64encode('群文件'.encode()).decode()}])
        image_path = f"/api/chat/messages/{attachment['id']}/attachments/0?revision={attachment['revision']}"
        assert b.request(image_path) == png()
        outsider.request(path, status=404)
        outsider.request(image_path, status=404)
        outsider.request(f'/api/chat/conversations/{cid}/export?format=json', status=404)
        assert outsider.request('/api/chat/search?query=' + quote('群附件'))['messages'] == []
        b.request(f'/api/chat/groups/{cid}', 'PUT', {'requestId': uid(), 'expectedRevision': latest()['revision'], 'name': '无权修改'}, status=403)
        a.request(f"/api/chat/groups/{cid}/members/{humans[1]['account']['id']}", 'PUT', {'requestId': uid(), 'expectedRevision': latest()['revision'], 'role': 'admin'})
        b.request(f'/api/chat/groups/{cid}/members/{ai0}', 'PUT', {'requestId': uid(), 'expectedRevision': latest()['revision'], 'role': 'admin'}, status=403)
        b.request(f'/api/chat/groups/{cid}/members', 'POST', {'requestId': uid(), 'expectedRevision': latest()['revision'], 'accountId': humans[2]['account']['id']})
        assert any(m['id'] == first['id'] for m in outsider.request(path)['messages'])
        b.request(f"/api/chat/groups/{cid}/members/{humans[2]['account']['id']}", 'DELETE', {'requestId': uid(), 'expectedRevision': latest()['revision']})
        outsider.request(path, status=404)
        outsider.request(image_path, status=404)
        print('PASS: role boundaries, full pre-join history, attachments and removed-member isolation', flush=True)

        Model.hold = True
        Model.release.clear()
        slow = send('成员退出时中断', [ai0])
        wait_for(lambda: states(slow['id']), lambda values: values[0]['status'] == 'running')
        b.request(f'/api/chat/groups/{cid}/members/{ai0}', 'DELETE', {'requestId': uid(), 'expectedRevision': latest()['revision']})
        Model.release.set()
        Model.hold = False
        assert states(slow['id'])[0]['status'] == 'cancelled'
        b.request(f'/api/chat/groups/{cid}/members', 'POST', {'requestId': uid(), 'expectedRevision': latest()['revision'], 'accountId': ai0})
        completed(send('重新入群后的新消息', [ai0]), 1)

        managed = next(v for v in b.request('/api/ai') if v['account']['id'] == ai1)
        disabled = b.request(f'/api/ai/{ai1}/status', 'PUT', {'requestId': uid(), 'expectedStatusRevision': managed['statusRevision'], 'status': 'disabled'})
        calls_before = len(Model.calls)
        disabled_message = send('停用时的消息不补发', [ai1])
        assert disabled_message['aiResponses'] == []
        disabled_direct = b.request('/api/chat/direct', 'POST', {'accountId': ai1})
        assert disabled_direct['name'] == actors[1]['account']['name']
        saved_while_disabled = b.request(f"/api/chat/conversations/{disabled_direct['id']}/messages", 'POST', {
            'requestId': uid(), 'text': '停用期间私聊仍保存，但恢复后不补发'})
        assert saved_while_disabled['status'] == 'committed' and saved_while_disabled['aiResponses'] == []
        b.request(f'/api/ai/{ai1}/status', 'PUT', {'requestId': uid(), 'expectedStatusRevision': disabled['statusRevision'], 'status': 'active'})
        completed(send('恢复后只处理新消息', [ai1]), 1)
        assert len(Model.calls) == calls_before + 1
        print('PASS: membership cancellation and rejoin, disabled AI does not replay old inputs', flush=True)

        direct = a.request('/api/chat/direct', 'POST', {'accountId': ai0})
        assert direct['name'] == actors[0]['account']['name']
        relation = a.request(f'/api/chat/contacts/{ai0}')
        relation = a.request(f'/api/chat/contacts/{ai0}/block', 'PUT', {'requestId': uid(), 'expectedRevision': relation['revision'], 'blocked': True})
        a.request(f"/api/chat/conversations/{direct['id']}/messages", 'POST', {'requestId': uid(), 'text': '屏蔽后的私聊'}, status=403)
        completed(send('屏蔽私聊不影响共同群', [ai0]), 1)
        a.request('/api/chat/contacts', 'POST', {'accountId': ai0, 'listed': False})
        assert a.request(f'/api/chat/contacts/{ai0}')['blocked']
        a.request('/api/chat/contacts', 'POST', {'accountId': ai0, 'listed': True})
        relation = a.request(f'/api/chat/contacts/{ai0}')
        assert relation['blocked']
        a.request(f'/api/chat/contacts/{ai0}/block', 'PUT', {'requestId': uid(), 'expectedRevision': relation['revision'], 'blocked': False})
        assert not next(c for c in a.request('/api/chat/init')['conversations'] if c['id'] == direct['id'])['directBlocked']
        print('PASS: direct block survives contact removal/re-add and preserves shared-group interaction', flush=True)

        a.request(f'/api/chat/groups/{cid}/owner', 'POST', {'requestId': uid(), 'expectedRevision': latest()['revision'], 'accountId': humans[1]['account']['id']})
        a.request(f'/api/chat/groups/{cid}/leave', 'POST', {'requestId': uid(), 'expectedRevision': latest()['revision']})
        a.request(path, status=404)
        b.request(f'/api/chat/groups/{cid}/members', 'POST', {'requestId': uid(), 'expectedRevision': latest(b)['revision'], 'accountId': humans[0]['account']['id']})
        assert any(m['id'] == first['id'] for m in a.request(path)['messages'])
        # Return ownership to the primary browser account for the optional UI session.
        b.request(f'/api/chat/groups/{cid}/owner', 'POST', {'requestId': uid(), 'expectedRevision': latest()['revision'], 'accountId': humans[0]['account']['id']})
        print('PASS: owner transfer, leave and rejoin retain the same group history', flush=True)

        if args.client_host:
            code = '''import json,sys,tempfile,ssl,http.cookiejar,urllib.request,uuid
v=json.load(sys.stdin)
with tempfile.NamedTemporaryFile(mode='w',suffix='.pem') as ca:
 ca.write(v['ca']);ca.flush()
 op=urllib.request.build_opener(urllib.request.HTTPCookieProcessor(http.cookiejar.CookieJar()),urllib.request.HTTPSHandler(context=ssl.create_default_context(cafile=ca.name)))
 def call(path,body=None):
  req=urllib.request.Request(v['url']+path,None if body is None else json.dumps(body).encode(),{'Content-Type':'application/json','X-LLMN-Client':'1','Origin':v['url']})
  return json.load(op.open(req,timeout=30))
 call('/api/auth/login',{'email':v['email'],'password':v['password']})
 assert any(m['id']==v['first'] for m in call(v['path'])['messages'])
 result=call(v['path'],{'requestId':str(uuid.uuid4()),'text':'第二台客户端已读完整群历史并回复'})
 print(json.dumps({'id':result['id']}))
'''
            payload = {k: humans[1][k] for k in ('email', 'password')}
            payload.update(url=saved['url'], ca=Path(ca).read_text(), path=path, first=first['id'])
            reply = json.loads(remote(args.client_host, ['python3', '-c', code], json.dumps(payload).encode()))
            assert any(m['id'] == reply['id'] for m in a.request(path)['messages'])
            print('PASS: second device reads complete group history and replies over verified HTTPS', flush=True)
        result = {'conversationId': cid, 'name': group['name'], 'aiIds': [ai0, ai1], 'mockCalls': len(Model.calls), 'maxConcurrentCalls': Model.peak}
        (directory/'result.json').write_text(json.dumps(result, ensure_ascii=False, indent=2), encoding='utf-8')
        print('PASS: group acceptance complete', flush=True)
        if args.serve_ui:
            print('Mock nodes stay online for browser checks; create WORKDIR/stop to finish.', flush=True)
            while not (directory/'stop').exists():
                time.sleep(1)
    finally:
        Model.release.set()
        for process in processes:
            if process.poll() is None:
                process.terminate()
                process.wait(timeout=15)
        for log in logs:
            log.close()
        model.shutdown()
        for api, ai in zip(clients, actors):
            current = next(v for v in api.request('/api/ai') if v['account']['id'] == ai['account']['id'])
            if current['node']:
                api.request(f"/api/ai/{ai['account']['id']}/node/revoke", 'POST', {'requestId': uid(), 'expectedBindingVersion': current['bindingVersion']})


if __name__ == '__main__':
    main()
