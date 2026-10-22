"""Run the real Agent binary against a test cloud, using a local mock model.

Creates one disposable AI under an existing test account, tests chat, restart,
cancellation and revocation. No real model or model key is used. Credentials
come from the ignored output of cloud_acceptance.py; do not use daily accounts.
"""
import argparse
import json
import os
from pathlib import Path
import re
import subprocess
import threading
import time
import uuid
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

from cloud_acceptance import Client


def uid():
    return str(uuid.uuid4())


def wait_for(read, predicate, timeout=90):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        value = read()
        if predicate(value):
            return value
        time.sleep(1)
    raise AssertionError("Timed out waiting for the expected cloud state")


class Model(BaseHTTPRequestHandler):
    calls = 0
    slow = False
    release = threading.Event()

    def log_message(self, *args):
        pass

    def do_POST(self):
        payload = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
        assert self.path == '/v1/chat/completions'
        assert payload['model'] == 'fixture' and payload['stream']
        assert any('节点验收角色' in str(m) for m in payload['messages'])
        Model.calls += 1
        self.send_response(200)
        self.send_header('Content-Type', 'text/event-stream')
        self.send_header('Connection', 'close')
        self.end_headers()

        def event(body):
            if isinstance(body, dict):
                body['id'] = 'fixture'
            data = json.dumps(body, ensure_ascii=False) if isinstance(body, dict) else body
            self.wfile.write(('data: ' + data + '\n\n').encode())
            self.wfile.flush()

        try:
            event({'choices': [{'index': 0, 'delta': {'content': '模拟节点'}, 'finish_reason': None}]})
            time.sleep(0.7)
            event({'choices': [{'index': 0, 'delta': {'content': '回复'}, 'finish_reason': None}]})
            if Model.slow:
                Model.release.wait(60)
            event({'choices': [{'index': 0, 'delta': {'content': '完成'}, 'finish_reason': None}]})
            event({'choices': [{'index': 0, 'delta': {}, 'finish_reason': 'stop'}],
                   'usage': {'prompt_tokens': 20, 'completion_tokens': 8, 'total_tokens': 28}})
            event('[DONE]')
        except (BrokenPipeError, ConnectionResetError, ConnectionAbortedError):
            pass  # Cancellation is part of this fixture's expected behavior.


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', required=True)
    parser.add_argument('--accounts', required=True, help='Private cloud_acceptance.py output')
    parser.add_argument('--ca', required=True)
    parser.add_argument('--workdir', required=True, help='New, ignored private test directory')
    args = parser.parse_args()
    binary = str(Path(args.binary).resolve())
    directory = Path(args.workdir).resolve()
    directory.mkdir(parents=True, exist_ok=False)
    saved = json.loads(Path(args.accounts).read_text(encoding='utf-8'))
    human = saved['accounts'][0]
    api = Client(saved['url'], str(Path(args.ca).resolve()))
    api.request('/api/auth/login', 'POST', {'email': human['email'], 'password': human['password']})
    ai = api.request('/api/ai', 'POST', {'requestId': uid(), 'name': '节点验收 ' + uid()[:6], 'bio': '仅用于模拟模型验收'})
    model = ThreadingHTTPServer(('127.0.0.1', 0), Model)
    threading.Thread(target=model.serve_forever, daemon=True).start()
    process_options = {'creationflags': subprocess.CREATE_NO_WINDOW} if os.name == 'nt' else {}
    env = {**os.environ, 'LLMN_NODE_FIXTURE_KEY': 'mock-model-key'}
    node = directory / 'node'
    subprocess.run([binary, 'init', str(node)], check=True, capture_output=True, **process_options)
    toml = f'''server = {json.dumps(saved['url'])}
ca_file = {json.dumps(str(Path(args.ca).resolve()).replace(chr(92), '/'))}
node_id = "{uid()}"
node_name = "模拟验收节点"
daily_calls = 12
daily_tokens = 262144
[profile]
revision = "1"
persona = "节点验收角色；这是模拟测试。"
max_tokens = 256
[profile.selection]
provider = "fixture"
model = "fixture"
[providers.fixture]
protocol = "openai"
base_url = "http://127.0.0.1:{model.server_port}/v1"
api_key = {{ env = "LLMN_NODE_FIXTURE_KEY" }}
[providers.fixture.models.fixture]
model = "fixture"
context_window = 8192
max_tokens = 512
image_input = true
'''
    (node / 'node.toml').write_text(toml, encoding='utf-8')
    log = open(directory / 'node.log', 'ab')
    pair_log = directory / 'pair.log'
    with pair_log.open('wb') as output:
        pairing = subprocess.Popen([binary, 'pair', str(node)], stdout=output, stderr=log, env=env, **process_options)
    agent = None
    revoked = False
    try:
        def pairing_code():
            match = re.search(r'pairing code (\S+)', pair_log.read_text(encoding='utf-8'))
            if pairing.poll() is not None and not match:
                raise AssertionError('Pairing exited; inspect the private node.log')
            return match.group(1) if match else None

        code = wait_for(pairing_code, bool)
        ai = api.request(f"/api/ai/{ai['account']['id']}/node", 'POST',
                         {'requestId': uid(), 'pairingCode': code, 'expectedBindingVersion': ai['bindingVersion']})
        assert pairing.wait(timeout=30) == 0

        def start():
            return subprocess.Popen([binary, 'run', str(node)], stdout=log, stderr=log, env=env, **process_options)

        agent = start()
        wait_for(lambda: api.request('/api/ai'),
                 lambda items: any(a['account']['id'] == ai['account']['id'] and a['node']['online'] for a in items))
        conversation = api.request('/api/chat/direct', 'POST', {'accountId': ai['account']['id']})
        path = f"/api/chat/conversations/{conversation['id']}/messages"

        def send(text):
            return api.request(path, 'POST', {'requestId': uid(), 'text': text, 'images': [], 'files': []})

        def reply_state(mid):
            return next(m for m in api.request(path)['messages'] if m['id'] == mid).get('aiResponse')

        def completed(mid):
            def read():
                state = reply_state(mid)
                assert not state or state['status'] in ('queued', 'running', 'completed'), state
                return state
            return wait_for(read, lambda r: r and r['status'] == 'completed')

        first = send('第一条模拟消息')
        completed(first['id'])
        assert Model.calls == 1
        messages = api.request(path)['messages']
        assert sum(m['senderId'] == ai['account']['id'] and m['text'] == '模拟节点回复完成' for m in messages) == 1
        print('PASS: pairing, real WSS, model streaming, committed reply', flush=True)

        agent.terminate()
        agent.wait(timeout=15)
        agent = start()
        second = send('重启后的新消息')
        completed(second['id'])
        assert Model.calls == 2
        print('PASS: node restart processes only new work, no repeated model call', flush=True)

        Model.slow = True
        third = send('这条回复稍后停止')
        wait_for(lambda: api.request(path)['messages'], lambda ms: any(m['status'] == 'generating' for m in ms))
        task_id = reply_state(third['id'])['taskId']
        api.request(f'/api/chat/ai-tasks/{task_id}/cancel', 'POST', {'requestId': uid()})
        Model.release.set()
        wait_for(lambda: reply_state(third['id']), lambda r: r and r['status'] == 'cancelled')
        Model.slow = False
        fourth = send('停止后继续聊天')
        completed(fourth['id'])
        assert Model.calls == 4
        print('PASS: cancellation fences the late result and next message still works', flush=True)

        api.request(f"/api/ai/{ai['account']['id']}/node/revoke", 'POST',
                    {'requestId': uid(), 'expectedBindingVersion': ai['bindingVersion']})
        revoked = True
        last = send('撤销连接后不能调用模型')
        time.sleep(5)
        assert Model.calls == 4
        assert reply_state(last['id'])['status'] == 'queued'
        (directory / 'result.json').write_text(json.dumps({'aiId': ai['account']['id'],
            'conversationId': conversation['id'], 'mockCalls': Model.calls}, indent=2), encoding='utf-8')
        print('PASS: revoked node cannot read new tasks or call the model; total mock calls=4', flush=True)
    finally:
        Model.release.set()
        for process in (pairing, agent):
            if process and process.poll() is None:
                process.terminate()
                process.wait(timeout=15)
        model.shutdown()
        log.close()
        if ai['node'] and not revoked:
            api.request(f"/api/ai/{ai['account']['id']}/node/revoke", 'POST',
                        {'requestId': uid(), 'expectedBindingVersion': ai['bindingVersion']})


if __name__ == '__main__':
    main()
