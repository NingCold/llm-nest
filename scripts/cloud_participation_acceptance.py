"""Verify cloud participation and contact scheduling with two actual Agent processes and a mock model.

Use disposable cloud_acceptance.py accounts, a trusted test CA and a new ignored
work directory. No real model key is read. --serve-ui keeps the fixtures online
until WORKDIR/stop exists so the same data can be checked in the browser.
"""
import argparse
from datetime import datetime, timedelta, timezone
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
        text = f"{payload['model']} {'摘要：群里约好了周六去图书馆。' if summary else '接话验收模拟回复完成'}"
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
    env = {**os.environ, 'LLMN_PARTICIPATION_FIXTURE_KEY': 'mock-participation-key'}
    processes, logs, actors, nodes = [], [], [], []
    suffix = uid()[:6]

    def run_node(index):
        process = subprocess.Popen([binary, 'run', str(nodes[index])], stdout=logs[index], stderr=logs[index], env=env, **process_options)
        processes.append(process)
        return process

    try:
        for index, api in enumerate((a, b)):
            ai = api.request('/api/ai', 'POST', {'requestId': uid(), 'name': f'接话验收{"甲" if index == 0 else "乙"}-{suffix}', 'bio': '模拟模型，验证接话与约定'})
            actors.append(ai)
            node = directory / f'node-{index}'
            nodes.append(node)
            subprocess.run([binary, 'init', str(node)], check=True, capture_output=True, **process_options)
            key = f'fixture-{"a" if index == 0 else "b"}'
            (node/'node.toml').write_text(f'''server = {json.dumps(saved['url'])}
ca_file = {json.dumps(ca.replace(chr(92), '/'))}
node_id = "{uid()}"
node_name = "接话验收节点{index + 1}"
daily_calls = 100
daily_tokens = 3276800
[profile]
revision = "1"
persona = "这是接话验收角色 {key}。"
max_tokens = 256
[profile.selection]
provider = "fixture"
model = "{key}"
[providers.fixture]
protocol = "openai"
base_url = "http://127.0.0.1:{model.server_port}/v1"
api_key = {{ env = "LLMN_PARTICIPATION_FIXTURE_KEY" }}
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
        group = a.request('/api/chat/groups', 'POST', {'requestId': uid(), 'name': f'接话与约定验收-{suffix}',
            'memberIds': [humans[1]['account']['id'], ai0, ai1]})
        cid = group['id']
        direct = a.request('/api/chat/direct', 'POST', {'accountId': ai0})
        did = direct['id']

        def view(window=cid, api=a):
            return api.request(f'/api/chat/conversations/{window}/participation')

        def policy(window=cid, **changes):
            current = view(window)
            return a.request(f'/api/chat/conversations/{window}/participation', 'PUT', {
                'requestId': uid(), 'expectedRevision': current['revision'], 'policy': {**current['policy'], **changes}})

        def control(action, window=cid, ai=None):
            current = view(window)
            return a.request(f'/api/chat/conversations/{window}/participation/control', 'POST', {
                'requestId': uid(), 'expectedRevision': current['revision'], 'action': action, **({'aiId': ai} if ai else {})})

        def settings(window=cid, **changes):
            current = view(window)['settings']
            return a.request(f'/api/chat/conversations/{window}/contact-settings', 'PUT', {
                'requestId': uid(), 'expectedRevision': current['revision'],
                **{k: current[k] for k in ('enabled', 'quietUntil', 'quietAction')}, **changes})

        def stamp(seconds):
            return (datetime.now(timezone.utc) + timedelta(seconds=seconds)).isoformat()

        def intent(window=cid, api=a, target=None, seconds=3, lifetime=60, text='请按约定提醒我休息。'):
            return api.request(f'/api/chat/conversations/{window}/contact-intents', 'POST', {
                'requestId': uid(), 'aiId': target or ai0, 'content': text,
                'dueAt': stamp(seconds), 'expiresAt': stamp(seconds + lifetime), 'utcOffsetMinutes': 480})

        def intent_state(item, expected, window=cid):
            return wait_for(lambda: next(i for i in view(window)['intents'] if i['id'] == item['id']),
                lambda i: i['status'] == expected, timeout=35)

        def send(window, text, mentions=None, api=a):
            return api.request(f'/api/chat/conversations/{window}/messages', 'POST', {
                'requestId': uid(), 'text': text, 'mentions': mentions or [], 'images': [], 'files': []})

        def messages(window):
            return a.request(f'/api/chat/conversations/{window}/messages')['messages']

        def finished(window, mid):
            return wait_for(lambda: next(m for m in messages(window) if m['id'] == mid)['aiResponses'],
                lambda rows: rows and all(r['status'] == 'completed' for r in rows), timeout=35)

        assert view()['policy']['mode'] == 'mentionedOnly'
        assert not view()['settings']['enabled'] and not view(api=b)['canManage']
        current = view(api=b)
        b.request(f'/api/chat/conversations/{cid}/participation', 'PUT', {
            'requestId': uid(), 'expectedRevision': current['revision'], 'policy': current['policy']}, status=403)
        outsider.request(f'/api/chat/conversations/{cid}/participation', status=404)
        send(cid, '默认不点名，只保存这句话。')
        time.sleep(2)
        assert not Model.calls
        print('PASS: default explicit-only behavior and group authority', flush=True)

        # Two quick messages stay separate in history, but become one complete input.
        policy(did, directMergeMs=2000)
        first = send(did, '合并输入甲：周六去图书馆。')
        second = send(did, '合并输入乙：上午九点集合。')
        finished(did, second['id'])
        assert len(Model.calls) == 1
        prompt = json.dumps(Model.calls[-1]['messages'], ensure_ascii=False)
        assert '合并输入甲' in prompt and '合并输入乙' in prompt
        assert first['id'] != second['id']
        print('PASS: direct input merge preserves originals and both necessary inputs', flush=True)

        before = len(Model.calls)
        policy(mode='limitedAuto', maxConsecutiveAi=3, maxBoutCalls=12, maxDailyCalls=3)
        send(cid, '有限自动接话：请大家讨论周末读书。')
        wait_for(lambda: view()['state'], lambda s: s['consecutiveAi'] == 3, timeout=35)
        time.sleep(2)
        assert len(Model.calls) == before + 3
        models = [call['model'] for call in Model.calls[before:]]
        assert models[0] != models[1] and models[0] == models[2]
        assert view()['state']['dailyCalls'] == 3
        explicit = send(cid, '额度已到，仍请明确点名的 AI 回答。', [ai0])
        finished(cid, explicit['id'])
        time.sleep(1)
        assert len(Model.calls) == before + 4 and view()['state']['dailyCalls'] == 3
        print('PASS: two nodes alternate, stop at quota, and explicit mentions remain available', flush=True)

        control('pause')
        before = len(Model.calls)
        paused = send(cid, '暂停期间点名也不会生成。', [ai0])
        time.sleep(1)
        assert not next(m for m in messages(cid) if m['id'] == paused['id'])['aiResponses']
        control('resume')
        time.sleep(2)
        assert len(Model.calls) == before
        policy(mode='mentionedOnly', maxDailyCalls=48)
        control('end')
        settings(enabled=True)
        future = intent(seconds=300)
        control('end')
        assert intent_state(future, 'pending')['id'] == future['id']
        print('PASS: pause wins over mentions; resume has no replay; ending preserves future contacts', flush=True)

        # The intent is the independent trigger; no extra human message is inserted.
        marker = '独立约定标记' + suffix
        human_before = sum(m['senderId'] in {h['account']['id'] for h in humans} for m in messages(cid))
        before = len(Model.calls)
        due = intent(text=marker)
        intent_state(due, 'delivered')
        assert marker in json.dumps(Model.calls[-1]['messages'], ensure_ascii=False)
        time.sleep(2)
        assert len(Model.calls) == before + 1
        assert sum(m['senderId'] in {h['account']['id'] for h in humans} for m in messages(cid)) == human_before
        print('PASS: contact uses its authorized content once without fabricating human history', flush=True)

        settings(quietUntil=stamp(60), quietAction='defer')
        deferred = intent()
        time.sleep(5)
        assert next(i for i in view()['intents'] if i['id'] == deferred['id'])['status'] in ('pending', 'claimed')
        normal = send(cid, '免打扰期间普通点名仍正常。', [ai1])
        finished(cid, normal['id'])
        settings(quietUntil=None)
        intent_state(deferred, 'delivered')
        settings(quietUntil=stamp(60), quietAction='skip')
        skipped = intent()
        intent_state(skipped, 'skipped')
        settings(quietUntil=None)
        print('PASS: DND defer/skip affects contacts only and does not block ordinary replies', flush=True)

        # A member's own agreement is cancelled by their private block, other creators remain.
        own = intent(api=b, seconds=300)
        entry = b.request(f'/api/chat/contacts/{ai0}')
        b.request(f'/api/chat/contacts/{ai0}/block', 'PUT', {'requestId': uid(), 'expectedRevision': entry['revision'], 'blocked': True})
        intent_state(own, 'cancelled')
        assert intent_state(future, 'pending')['id'] == future['id']
        print('PASS: blocking cancels the creator\'s group contact without cancelling other members\' agreements', flush=True)

        # Offline expiry remains terminal after the node starts again.
        live0 = next(p for p in reversed(processes) if p.poll() is None and str(nodes[0]) in p.args)
        live0.terminate()
        live0.wait(timeout=15)
        expired = intent(seconds=2, lifetime=2)
        intent_state(expired, 'expired')
        before = len(Model.calls)
        run_node(0)
        wait_for(lambda: a.request('/api/ai'), lambda rows: any(r['account']['id'] == ai0 and r['node']['online'] for r in rows))
        time.sleep(2)
        assert len(Model.calls) == before
        print('PASS: offline expired contact does not replay after reconnect', flush=True)

        # A call that started before node shutdown must never open another model stream.
        Model.hold = True
        Model.release.clear()
        uncertain = intent()
        intent_state(uncertain, 'executing')
        wait_for(lambda: len(Model.calls), lambda n: n == before + 1)
        live0 = next(p for p in reversed(processes) if p.poll() is None and str(nodes[0]) in p.args)
        live0.terminate()
        live0.wait(timeout=15)
        Model.release.set()
        Model.hold = False
        run_node(0)
        intent_state(uncertain, 'uncertain')
        time.sleep(2)
        assert len(Model.calls) == before + 1
        print('PASS: started contact recovery becomes uncertain without duplicate model invocation', flush=True)

        # Leave no scheduled fixture behind. Keep nodes only when browser verification needs them.
        for item in view()['intents']:
            if item['status'] in ('pending', 'claimed', 'executing'):
                a.request(f"/api/chat/conversations/{cid}/contact-intents/{item['id']}/cancel", 'POST', {
                    'requestId': uid(), 'expectedRevision': item['revision']})
        result = {'conversationId': cid, 'directId': did, 'name': group['name'], 'aiIds': [ai0, ai1], 'mockCalls': len(Model.calls)}
        (directory/'result.json').write_text(json.dumps(result, ensure_ascii=False, indent=2), encoding='utf-8')
        print('PASS: participation acceptance complete', flush=True)
        if args.serve_ui:
            print('Mock nodes remain online; create WORKDIR/stop after browser verification.', flush=True)
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
