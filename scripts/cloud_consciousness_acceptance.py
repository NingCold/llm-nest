"""Verify node-owned consciousness with two actual Agent processes and a mock model.

Use disposable cloud_acceptance.py accounts, a trusted test CA and a new ignored
work directory. No real model key is read. --serve-ui keeps the fixtures online
until WORKDIR/stop exists so the same data can be checked in the browser.
"""
import argparse
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import re
import sqlite3
import subprocess
import threading
import time
import uuid

from agent_acceptance import wait_for
from cloud_acceptance import Client


def uid():
    return str(uuid.uuid4())


class Model(BaseHTTPRequestHandler):
    calls = []
    hold = False
    release = threading.Event()
    lock = threading.Lock()

    def log_message(self, *args):
        pass

    def do_POST(self):
        payload = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
        assert self.path == '/v1/chat/completions'
        with self.lock:
            self.calls.append(payload)
        summary = '窗口分段摘要' in str(payload['messages'][0])
        text = f"{payload['model']} {'摘要：群里约好了周六去图书馆。' if summary else '意识验收模拟回复完成'}"
        self.send_response(200)
        self.send_header('Connection', 'close')
        if not payload.get('stream'):
            self.send_header('Content-Type', 'application/json')
            self.end_headers()
            self.wfile.write(json.dumps({'id': uid(), 'choices': [{'index': 0,
                'message': {'role': 'assistant', 'content': text}, 'finish_reason': 'stop'}],
                'usage': {'prompt_tokens': 60, 'completion_tokens': 12, 'total_tokens': 72}}).encode())
            return
        self.send_header('Content-Type', 'text/event-stream')
        self.end_headers()

        def emit(body):
            value = json.dumps(body, ensure_ascii=False) if isinstance(body, dict) else body
            self.wfile.write(('data: ' + value + '\n\n').encode())
            self.wfile.flush()

        try:
            emit({'id': uid(), 'choices': [{'index': 0, 'delta': {'content': text}, 'finish_reason': None}]})
            time.sleep(.7)
            if self.hold and not summary:
                self.release.wait(45)
            emit({'id': uid(), 'choices': [{'index': 0, 'delta': {}, 'finish_reason': 'stop'}],
                'usage': {'prompt_tokens': 60, 'completion_tokens': 12, 'total_tokens': 72}})
            emit('[DONE]')
        except (BrokenPipeError, ConnectionResetError, ConnectionAbortedError):
            pass


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for option in ('binary', 'accounts', 'ca', 'workdir'):
        parser.add_argument('--' + option, required=True)
    parser.add_argument('--serve-ui', action='store_true')
    args = parser.parse_args()
    directory = Path(args.workdir).resolve()
    directory.mkdir(parents=True, exist_ok=False)
    binary = str(Path(args.binary).resolve())
    ca = str(Path(args.ca).resolve())
    saved = json.loads(Path(args.accounts).read_text(encoding='utf-8'))
    humans = saved['accounts'][:3]
    assert len(humans) == 3
    clients = [Client(saved['url'], ca) for _ in humans]
    for api, human in zip(clients, humans):
        api.request('/api/auth/login', 'POST', {k: human[k] for k in ('email', 'password')})
    a, b, outsider = clients
    model = ThreadingHTTPServer(('127.0.0.1', 0), Model)
    threading.Thread(target=model.serve_forever, daemon=True).start()
    process_options = {'creationflags': subprocess.CREATE_NO_WINDOW} if os.name == 'nt' else {}
    env = {**os.environ, 'LLMN_CONSCIOUSNESS_FIXTURE_KEY': 'mock-consciousness-key'}
    processes, logs, actors, nodes = [], [], [], []
    suffix = uid()[:6]

    def run_node(index):
        process = subprocess.Popen([binary, 'run', str(nodes[index])], stdout=logs[index], stderr=logs[index], env=env, **process_options)
        processes.append(process)
        return process

    try:
        for index, api in enumerate((a, b)):
            ai = api.request('/api/ai', 'POST', {'requestId': uid(), 'name': f'意识验收{"甲" if index == 0 else "乙"}-{suffix}', 'bio': '模拟模型，验证记忆与摘要'})
            actors.append(ai)
            node = directory / f'node-{index}'
            nodes.append(node)
            subprocess.run([binary, 'init', str(node)], check=True, capture_output=True, **process_options)
            key = f'fixture-{"a" if index == 0 else "b"}'
            (node/'node.toml').write_text(f'''server = {json.dumps(saved['url'])}
ca_file = {json.dumps(ca.replace(chr(92), '/'))}
node_id = "{uid()}"
node_name = "意识验收节点{index + 1}"
daily_calls = 100
daily_tokens = 3276800
[profile]
revision = "1"
persona = "这是意识验收角色 {key}。"
max_tokens = 256
[profile.selection]
provider = "fixture"
model = "{key}"
[providers.fixture]
protocol = "openai"
base_url = "http://127.0.0.1:{model.server_port}/v1"
api_key = {{ env = "LLMN_CONSCIOUSNESS_FIXTURE_KEY" }}
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

            def pairing_code():
                match = re.search(r'pairing code (\S+)', pair_path.read_text(encoding='utf-8'))
                assert match or pair.poll() is None, 'Pairing process exited'
                return match.group(1) if match else None

            actors[index] = api.request(f"/api/ai/{ai['account']['id']}/node", 'POST', {
                'requestId': uid(), 'pairingCode': wait_for(pairing_code, bool), 'expectedBindingVersion': ai['bindingVersion']})
            assert pair.wait(timeout=30) == 0
            run_node(index)
            wait_for(lambda: api.request('/api/ai'), lambda rows: any(r['account']['id'] == ai['account']['id'] and r['node']['online'] for r in rows))

        ai0, ai1 = [item['account']['id'] for item in actors]
        group = a.request('/api/chat/groups', 'POST', {'requestId': uid(), 'name': f'意识系统验收-{suffix}',
            'memberIds': [humans[1]['account']['id'], ai0, ai1]})
        cid = group['id']
        direct = a.request('/api/chat/direct', 'POST', {'accountId': ai0})
        did = direct['id']

        def manage(ai, window, operation, api=a, request_id=None, status=200):
            return api.request(f'/api/chat/conversations/{window}/ai/{ai}/consciousness', 'POST',
                {'requestId': request_id or uid(), 'operation': operation}, status=status)

        def memories(ai=ai0, window=cid, api=a):
            return manage(ai, window, {'type': 'memories'}, api)['memories']

        def send(window, text, mentions=None, api=a, **extra):
            return api.request(f'/api/chat/conversations/{window}/messages', 'POST', {
                'requestId': uid(), 'text': text, 'mentions': mentions or [], 'images': [], 'files': [], **extra})

        def messages(window):
            return a.request(f'/api/chat/conversations/{window}/messages')['messages']

        def finished(window, mid, expected='completed'):
            def read():
                return next(m for m in messages(window) if m['id'] == mid)['aiResponses']
            return wait_for(read, lambda rows: rows and all(r['status'] == expected for r in rows))

        source = send(cid, '周六去图书馆；早上九点在门口集合。')
        source_ref = {'messageId': source['id'], 'revision': source['revision']}
        command = {'type': 'createMemory', 'kind': 'fact', 'text': '群记忆：我们约好周六去图书馆。', 'source': source_ref}
        command_id = uid()
        first = manage(ai0, cid, command, request_id=command_id)['memories']
        assert len(first) == 1 and first[0]['status'] == 'confirmed'
        replay = manage(ai0, cid, command, request_id=command_id)['memories']
        assert replay[0]['id'] == first[0]['id'] and len(replay) == 1
        assert memories(ai1) == [] and memories(window=did) == []
        manage(ai0, cid, {'type': 'memories'}, api=outsider, status=403)
        manage(ai0, cid, {'type': 'summaries'}, api=b, status=403)
        print('PASS: node memory save/replay, AI/window isolation and summary permissions', flush=True)

        # A marker not present in raw chat must reach only this AI in this DM.
        private_marker = '私聊专用记忆标记' + suffix
        private = manage(ai0, did, {'type': 'createMemory', 'kind': 'preference', 'text': private_marker, 'source': None})['memories'][0]
        chat = send(did, '请结合这个窗口的记忆回答。')
        finished(did, chat['id'])
        assert private_marker in json.dumps(Model.calls[-1]['messages'], ensure_ascii=False)
        group_chat = send(cid, '请回答群里的话题。', [ai0])
        finished(cid, group_chat['id'])
        assert private_marker not in json.dumps(Model.calls[-1]['messages'], ensure_ascii=False)

        reference = {'conversationId': cid, 'sources': [source_ref]}
        preview = a.request(f'/api/chat/conversations/{did}/reference-preview', 'POST', {'reference': reference})
        assert preview['messages'][0]['id'] == source['id']
        continued = send(did, '我们单独继续讨论群里这件事。', reference=reference)
        assert continued['contextReference']['conversationId'] == cid
        finished(did, continued['id'])
        assert '九点' in json.dumps(Model.calls[-1]['messages'], ensure_ascii=False)
        assert a.request('/api/chat/direct', 'POST', {'accountId': ai0})['id'] == did
        print('PASS: private memory stays private; selected group original enters the existing DM', flush=True)

        # Both AIs summarize the same original independently; no model call until enabled.
        before = len(Model.calls)
        for i in range(31):
            send(cid, f'分段摘要原文 {i + 1}：读书活动继续讨论。')
        assert len(Model.calls) == before
        summary_views = []
        for index, api in enumerate((a, b)):
            ai = (ai0, ai1)[index]
            view = manage(ai, cid, {'type': 'summaries'}, api)
            assert not view['settings']['enabled'] and view['summaries'] == []
            view = manage(ai, cid, {'type': 'configureSummaries', 'revision': view['settings']['revision'],
                'enabled': True, 'binding': {'providerId': 'fixture', 'modelId': f'fixture-{"a" if index == 0 else "b"}',
                    'reasoningEffort': None, 'maxTokens': 256}}, api)
            view = wait_for(lambda: manage(ai, cid, {'type': 'summaries'}, api),
                lambda v: any(s['status'] == 'ready' for s in v['summaries']))
            ready = next(s for s in view['summaries'] if s['status'] == 'ready')
            assert f'fixture-{"a" if index == 0 else "b"}' in ready['text']
            assert len(ready['sources']) <= 32
            recent_ids = {m['id'] for m in messages(cid)[-12:]}
            assert not recent_ids.intersection(s['messageId'] for s in ready['sources'])
            summary_views.append(view)
        assert summary_views[0]['summaries'][0]['id'] != summary_views[1]['summaries'][0]['id']
        print('PASS: summaries opt in, run per AI, preserve latest 12 originals and record source segments', flush=True)

        # Forget during generation must not publish a response based on old memory.
        Model.hold = True
        Model.release.clear()
        slow = send(did, '这条回复等待遗忘操作完成。')
        wait_for(lambda: next(m for m in messages(did) if m['id'] == slow['id'])['aiResponses'],
            lambda rows: rows and rows[0]['status'] == 'running')
        forgotten = manage(ai0, did, {'type': 'forgetMemory', 'id': private['id'], 'revision': private['revision']})['memories']
        assert next(m for m in forgotten if m['id'] == private['id'])['text'] == ''
        Model.release.set()
        Model.hold = False
        wait_for(lambda: next(m for m in messages(did) if m['id'] == slow['id'])['aiResponses'],
            lambda rows: rows[0]['status'] not in ('queued', 'running'))
        assert next(m for m in messages(did) if m['id'] == slow['id'])['aiResponses'][0]['status'] != 'completed'
        print('PASS: forgetting while the model streams prevents publication from the old memory', flush=True)

        # Stop one node, change a source, and prove durable invalidation after reconnect.
        live0 = next(p for p in reversed(processes) if p.poll() is None and str(nodes[0]) in p.args)
        live0.terminate()
        live0.wait(timeout=15)
        a.request(f"/api/chat/messages/{source['id']}", 'PATCH', {
            'requestId': uid(), 'expectedRevision': source['revision'], 'text': '活动已经取消，请忽略原定时间。'})
        run_node(0)
        wait_for(lambda: a.request('/api/ai'), lambda rows: any(r['account']['id'] == ai0 and r['node']['online'] for r in rows))
        rows = wait_for(memories, lambda rows: rows and rows[0]['status'] == 'stale')
        assert rows[0]['text'] == ''
        view = manage(ai0, cid, {'type': 'summaries'})
        assert any(s['status'] == 'stale' and not s['text'] for s in view['summaries'])
        attached = next(m for m in messages(did) if m['id'] == continued['id'])
        assert attached['contextReference']['stale']
        a.request(f'/api/chat/conversations/{did}/reference-preview', 'POST', {'reference': reference}, status=409)
        print('PASS: offline source edits invalidate memories, private summaries and persisted references after reconnect', flush=True)

        # Check node SQLite storage without printing its content or credentials.
        for node in nodes:
            with sqlite3.connect(node/'node.sqlite') as db:
                tables = {row[0] for row in db.execute("SELECT name FROM sqlite_master WHERE type='table'")}
                assert any('memor' in name for name in tables) and any('summar' in name for name in tables)
        result = {'conversationId': cid, 'directId': did, 'name': group['name'], 'aiIds': [ai0, ai1], 'mockCalls': len(Model.calls)}
        (directory/'result.json').write_text(json.dumps(result, ensure_ascii=False, indent=2), encoding='utf-8')
        print('PASS: consciousness acceptance complete', flush=True)
        if args.serve_ui:
            print('Mock nodes remain online; create WORKDIR/stop when browser verification is complete.', flush=True)
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
                api.request(f"/api/ai/{ai['account']['id']}/node/revoke", 'POST', {
                    'requestId': uid(), 'expectedBindingVersion': current['bindingVersion']})


if __name__ == '__main__':
    main()
