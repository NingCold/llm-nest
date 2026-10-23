"""Deployed cloud AI management acceptance with a disposable local mock Agent.
Requires three disposable cloud_acceptance.py accounts and a new ignored workdir.
No real model key is read; --serve-ui keeps fixtures until WORKDIR/stop exists.
"""
import argparse
import base64
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import re
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
    release = threading.Event()

    def log_message(self, *args):
        pass

    def do_POST(self):
        assert self.path == '/v1/chat/completions'
        payload = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
        self.calls.append(payload)
        held = 'MANAGEMENT_HELD_INPUT' in str(payload) and not self.release.is_set()
        self.send_response(200)
        self.send_header('Connection', 'close')
        self.send_header('Content-Type', 'text/event-stream')
        self.end_headers()
        def emit(value):
            value = json.dumps(value) if isinstance(value, dict) else value
            self.wfile.write(('data: ' + value + '\n\n').encode())
            self.wfile.flush()
        try:
            emit({'id': uid(), 'choices': [{'index': 0, 'delta': {'content': payload['model'] + ' management fixture reply'}, 'finish_reason': None}]})
            if held:
                assert self.release.wait(90), 'Held mock call was not released'
            emit({'id': uid(), 'choices': [{'index': 0, 'delta': {}, 'finish_reason': 'stop'}], 'usage': {'prompt_tokens': 50, 'completion_tokens': 10, 'total_tokens': 60}})
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
    repository = Path(__file__).resolve().parents[1]
    if directory.is_relative_to(repository):
        assert subprocess.run(['git', 'check-ignore', '-q', '--', str(directory)], cwd=repository).returncode == 0, 'Workdir inside repository must be ignored'
    directory.mkdir(parents=True, exist_ok=False)
    binary, ca = str(Path(args.binary).resolve()), str(Path(args.ca).resolve())
    saved = json.loads(Path(args.accounts).read_text(encoding='utf-8'))
    humans = saved['accounts'][:3]
    assert len(humans) == 3, 'Three disposable test accounts are required'
    clients = [Client(saved['url'], ca) for _ in humans]
    for api, human in zip(clients, humans):
        api.request('/api/auth/login', 'POST', {k: human[k] for k in ('email', 'password')})
    a, b, outsider = clients
    model = ThreadingHTTPServer(('127.0.0.1', 0), Model)
    threading.Thread(target=model.serve_forever, daemon=True).start()
    options = {'creationflags': subprocess.CREATE_NO_WINDOW} if os.name == 'nt' else {}
    mock_key, endpoint = 'management-mock-' + uid(), f'http://127.0.0.1:{model.server_port}/v1'
    env = {**os.environ, 'LLMN_MANAGEMENT_FIXTURE_KEY': mock_key}
    processes, logs, aid = [], [], None
    node = directory/'node'

    def command(action):
        return subprocess.run([binary, action, str(node)], check=True, capture_output=True, env=env, **options)

    def managed():
        return next(r for r in a.request('/api/ai') if r['account']['id'] == aid)

    def start():
        log = open(node/'run.log', 'ab')
        logs.append(log)
        process = subprocess.Popen([binary, 'run-managed', str(node)], stdout=log, stderr=log, env=env, **options)
        processes.append(process)
        wait_for(lambda: managed()['node']['online'], bool)
        status = json.loads(command('status').stdout)
        assert status['running'] and aid in status['aiIds']
        return process

    def manage(operation, api=None, request=None, status=200):
        return (api or a).request(f'/api/ai/{aid}/management', 'POST', {'requestId': request or uid(), 'operation': operation}, status=status)

    def save(view, persona, selected):
        return {'type': 'saveProfile', 'expectedRevision': view['profile']['revision'], 'persona': persona, 'providerId': 'fixture', 'modelId': selected, 'reasoningEffort': None, 'temperature': None, 'maxTokens': 128, 'autoDailyTokens': 65536}

    def safe(value):
        serialized = json.dumps(value)
        assert mock_key not in serialized and endpoint not in serialized
        def inspect(item):
            if isinstance(item, dict):
                assert not set(item).intersection({'apiKey', 'api_key', 'baseUrl', 'base_url', 'token', 'tokenHash', 'nodeToken'})
                for child in item.values():
                    inspect(child)
            elif isinstance(item, list):
                for child in item:
                    inspect(child)
        inspect(value)

    try:
        ai = a.request('/api/ai', 'POST', {'requestId': uid(), 'name': '管理验收-' + uid()[:6], 'bio': '仅本机模拟模型'})
        aid = ai['account']['id']
        manage({'type': 'view'}, status=409)
        command('init')
        (node/'node.toml').write_text(f'''server = {json.dumps(saved['url'])}
ca_file = {json.dumps(ca.replace(chr(92), '/'))}
node_id = "{uid()}"
node_name = "在线管理验收节点"
daily_calls = 100
daily_tokens = 3276800
[profile]
revision = "1"
persona = "MANAGEMENT_INITIAL_PERSONA"
max_tokens = 128
[profile.selection]
provider = "fixture"
model = "fixture-a"
[providers.fixture]
protocol = "openai"
base_url = "{endpoint}"
api_key = {{ env = "LLMN_MANAGEMENT_FIXTURE_KEY" }}
[providers.fixture.models.fixture-a]
model = "fixture-a"
context_window = 32768
max_tokens = 512
image_input = true
[providers.fixture.models.fixture-b]
model = "fixture-b"
context_window = 32768
max_tokens = 512
image_input = true
''', encoding='utf-8')
        pair_path = directory/'pair.log'
        pair_log = open(pair_path, 'wb')
        logs.append(pair_log)
        pair = subprocess.Popen([binary, 'pair', str(node)], stdout=pair_log, stderr=pair_log, env=env, **options)
        processes.append(pair)
        def pairing_code():
            match = re.search(r'pairing code (\S+)', pair_path.read_text(encoding='utf-8'))
            assert match or pair.poll() is None, 'Pairing process exited'
            return match.group(1) if match else None
        a.request(f'/api/ai/{aid}/node', 'POST', {'requestId': uid(), 'pairingCode': wait_for(pairing_code, bool), 'expectedBindingVersion': ai['bindingVersion']})
        assert pair.wait(timeout=30) == 0
        live = start()
        assert 'profileManagement' in managed()['node']['capabilities']
        identity = a.request('/api/me')
        view = manage({'type': 'view'})
        assert {m['modelId'] for m in view['models']} == {'fixture-a', 'fixture-b'}
        assert view['profile']['persona'] == 'MANAGEMENT_INITIAL_PERSONA'
        first_id = uid()
        first_op = save(view, 'MANAGEMENT_PERSONA_A', 'fixture-a')
        first = manage(first_op, request=first_id)
        assert first['profile']['revision'] != view['profile']['revision']
        cid = a.request('/api/chat/direct', 'POST', {'accountId': aid})['id']
        def send(text):
            return a.request(f'/api/chat/conversations/{cid}/messages', 'POST', {'requestId': uid(), 'text': text, 'images': [], 'files': []})
        def finished(mid):
            return wait_for(lambda: next(m for m in a.request(f'/api/chat/conversations/{cid}/messages')['messages'] if m['id'] == mid)['aiResponses'], lambda rows: rows and all(r['status'] == 'completed' for r in rows), timeout=45)
        held = send('MANAGEMENT_HELD_INPUT：使用当前档案回答。')
        wait_for(lambda: list(Model.calls), bool)
        assert Model.calls[-1]['model'] == 'fixture-a'
        assert 'MANAGEMENT_PERSONA_A' in str(Model.calls[-1]['messages'][0])
        second = manage(save(first, 'MANAGEMENT_PERSONA_B', 'fixture-b'))
        assert second['profile']['modelId'] == 'fixture-b'
        assert manage(first_op, request=first_id) == second, 'Replay must return current profile'
        manage({**first_op, 'persona': 'changed request body'}, request=first_id, status=409)
        Model.release.set()
        finished(held['id'])
        following = send('使用更新后的档案回答。')
        finished(following['id'])
        assert len(Model.calls) == 2
        assert Model.calls[0]['model'] == 'fixture-a' and Model.calls[1]['model'] == 'fixture-b'
        assert 'MANAGEMENT_PERSONA_B' in str(Model.calls[1]['messages'][0])
        assert 'MANAGEMENT_PERSONA_A' not in str(Model.calls[1]['messages'][0])
        print('PASS: profile save/replay and frozen in-flight model/persona snapshot', flush=True)

        public_path, admin_path = f'/api/ai/{aid}/profile', f'/api/ai/{aid}/administrators'
        public_op = {'requestId': uid(), 'expectedProfileRevision': managed()['profileRevision'], 'name': '管理验收已编辑', 'bio': '云端公开资料'}
        public = a.request(public_path, 'PUT', public_op)
        assert public['profileRevision'] != ai['profileRevision']
        admins = a.request(admin_path)
        admins = a.request(admin_path + '/' + humans[1]['account']['id'], 'PUT', {'requestId': uid(), 'expectedRevision': admins['revision'], 'enabled': True})
        assert len(admins['administrators']) == 2
        b.request(admin_path + '/' + humans[2]['account']['id'], 'PUT', {'requestId': uid(), 'expectedRevision': admins['revision'], 'enabled': True}, status=403)
        b.request(admin_path + '/' + humans[0]['account']['id'], 'PUT', {'requestId': uid(), 'expectedRevision': admins['revision'], 'enabled': False}, status=403)
        a.request(admin_path + '/' + humans[0]['account']['id'], 'PUT', {'requestId': uid(), 'expectedRevision': admins['revision'], 'enabled': False}, status=400)
        assert manage({'type': 'view'}, api=b) == second
        b_public = {'requestId': uid(), 'expectedProfileRevision': public['profileRevision'], 'name': '管理验收普通管理员编辑', 'bio': '普通管理员获授权'}
        public = b.request(public_path, 'PUT', b_public)
        replay = a.request(public_path, 'PUT', public_op)
        assert replay['account'] == public['account'] and replay['profileRevision'] == public['profileRevision']
        assert replay['role'] == 'initial' and public['role'] == 'admin'
        print('PASS: public revision/current replay and initial-only administrator authority', flush=True)

        private_cid = b.request('/api/chat/direct', 'POST', {'accountId': aid})['id']
        sent = b.request(f'/api/chat/conversations/{private_cid}/messages', 'POST', {'requestId': uid(), 'text': '仅另一真人与 AI 的验收私聊', 'files': [{'name': 'management-private.txt', 'mime': 'text/plain', 'data': base64.b64encode(b'management-private-file').decode()}]})
        def read_state():
            current = next(c for c in b.request('/api/chat/init')['conversations'] if c['id'] == private_cid)
            return current['readPosition'], current['manualUnread']
        before = read_state()
        a.request(f'/api/chat/conversations/{private_cid}/messages', status=404)
        scoped = f'/api/ai/{aid}/view/conversations/{private_cid}/messages'
        scoped_view = a.request(f'/api/ai/{aid}/view')
        ai_before = next(c for c in scoped_view['conversations'] if c['id'] == private_cid)
        assert private_cid in [c['id'] for c in scoped_view['conversations']]
        messages = a.request(scoped)
        assert any(m['id'] == sent['id'] for m in messages['messages'])
        a.request(scoped, 'POST', {'requestId': uid(), 'text': '不得代发'}, status=405)
        attachment = scoped + f"/{sent['id']}/attachments/0?revision={sent['revision']}"
        assert a.request(attachment) == b'management-private-file'
        outsider.request(attachment, status=403)
        assert read_state() == before
        ai_after = next(c for c in a.request(f'/api/ai/{aid}/view')['conversations'] if c['id'] == private_cid)
        assert (ai_before['readPosition'], ai_before['manualUnread']) == (ai_after['readPosition'], ai_after['manualUnread'])
        assert a.request('/api/me') == identity
        for value in (view, first, second, public, scoped_view, messages, a.request('/api/ai')):
            safe(value)
        a.request(admin_path + '/' + humans[1]['account']['id'], 'PUT', {'requestId': uid(), 'expectedRevision': admins['revision'], 'enabled': False})
        b.request(public_path, 'PUT', b_public, status=403)
        manage({'type': 'view'}, api=b, status=403)
        b.request(f'/api/ai/{aid}/view', status=403)
        assert len(a.request(admin_path)['administrators']) == 1
        print('PASS: scoped private history/attachment, unchanged human identity/read state, revoked access', flush=True)

        wait_for(lambda: next(m for m in b.request(f'/api/chat/conversations/{private_cid}/messages')['messages'] if m['id'] == sent['id'])['aiResponses'], lambda rows: rows and all(r['status'] == 'completed' for r in rows), timeout=45)
        command('stop')
        assert live.wait(timeout=15) == 0
        assert not json.loads(command('status').stdout)['running']
        wait_for(lambda: managed()['node']['online'], lambda online: not online)
        assert manage({'type': 'view'}, status=409)['code'] == 'node_offline'
        manage(save(second, 'OFFLINE_MUST_NOT_SAVE', 'fixture-a'), status=409)
        live = start()
        assert manage({'type': 'view'}) == second
        print('PASS: managed run/status/stop and offline changes are not queued', flush=True)
        (directory/'result.json').write_text(json.dumps({'aiId': aid, 'directId': cid, 'otherHumanDirectId': private_cid, 'mockCalls': len(Model.calls)}, ensure_ascii=False, indent=2), encoding='utf-8')
        print('PASS: management acceptance complete', flush=True)
        if args.serve_ui:
            print('Mock node remains online; create WORKDIR/stop after browser verification.', flush=True)
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
        model.server_close()
        if aid:
            current = managed()
            if current['node']:
                a.request(f'/api/ai/{aid}/node/revoke', 'POST', {'requestId': uid(), 'expectedBindingVersion': current['bindingVersion']})


if __name__ == '__main__':
    main()
