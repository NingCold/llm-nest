"""M1 private/group HTTP acceptance: temporary data and a loopback mock provider.
python scripts/social_acceptance.py [--binary PATH] [--dist PATH] [--serve]
No personal configuration or real provider is used. --serve retains the fixture for GUI QA.
"""
import argparse
import concurrent.futures
import http.server
import json
import os
from pathlib import Path
import socket
import sqlite3
import subprocess
import tempfile
import threading
import time
import urllib.error
import urllib.request
import uuid

ROOT = Path(__file__).resolve().parents[1]
REQUESTS = []


class Provider(http.server.BaseHTTPRequestHandler):
    def log_message(self, *_):
        pass

    def do_POST(self):
        request = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
        REQUESTS.append(request)
        content = next(m['content'] for m in reversed(request['messages']) if m['role'] == 'user' and m['content'].startswith('[我 ·'))
        text = content.rsplit('\n', 1)[-1]
        if text == 'provider-error':
            self.send_response(429)
            self.end_headers()
            self.wfile.write(b'{"error":{"message":"synthetic rate limit"}}')
            return
        self.send_response(200)
        self.send_header('Content-Type', 'text/event-stream')
        self.end_headers()

        def emit(delta=None, finish=None, usage=None):
            data = {'id': 'social-fixture', 'object': 'chat.completion.chunk', 'model': request['model'], 'choices': [] if usage else [{'index': 0, 'delta': delta or {}, 'finish_reason': finish}]}
            if usage:
                data['usage'] = usage
            self.wfile.write(('data: ' + json.dumps(data, ensure_ascii=False) + '\n\n').encode())
            self.wfile.flush()

        try:
            emit({'reasoning_content': 'fixture-private-reasoning'})
            if text == 'tool':
                emit({'tool_calls': [{'index': 0, 'id': 'fixture-call', 'type': 'function', 'function': {'name': 'add', 'arguments': '{"a":2,"b":3}'}}]}, 'tool_calls')
            else:
                if text.startswith('queue-'):
                    time.sleep(.2)
                emit({'content': '回应：' + text})
                if text.startswith('slow') or text == 'crash':
                    for _ in range(100):
                        time.sleep(.05)
                        emit({'content': '…'})
                if text == 'eof':
                    return
                emit({}, 'stop')
            emit(usage={'prompt_tokens': 12, 'completion_tokens': 8, 'total_tokens': 20})
            self.wfile.write(b'data: [DONE]\n\n')
            self.wfile.flush()
        except (BrokenPipeError, ConnectionResetError, ConnectionAbortedError):
            pass


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--binary', type=Path)
    parser.add_argument('--dist', type=Path)
    parser.add_argument('--serve', action='store_true')
    args = parser.parse_args()
    work = Path(tempfile.mkdtemp(prefix='llmn-social-acceptance-'))
    provider = http.server.ThreadingHTTPServer(('127.0.0.1', 0), Provider)
    threading.Thread(target=provider.serve_forever, daemon=True).start()
    with socket.socket() as sock:
        sock.bind(('127.0.0.1', 0))
        port = sock.getsockname()[1]
    base = f'http://127.0.0.1:{port}/api/social'
    config = work / 'llmn.toml'
    config.write_text(f'[providers.test]\nprotocol="openai"\nbase_url="http://127.0.0.1:{provider.server_port}/v1"\napi_key="fixture-only"\n[providers.test.models.chat]\nmodel="chat-wire"\ncontext_window=8192\nmax_tokens=1024\n[providers.test.models.alt]\nmodel="alt-wire"\ncontext_window=8192\n', encoding='utf-8')
    env = os.environ.copy()
    env.update(LLMN_CONFIG=str(config), LLMN_DATA_DIR=str(work / 'data'), LLMN_PORT=str(port))
    if args.dist:
        env['LLMN_WEB_DIST'] = str(args.dist.resolve())
    exe = (args.binary or ROOT / 'target/debug' / ('web-server.exe' if os.name == 'nt' else 'web-server')).resolve()
    log = (work / 'server.log').open('w', encoding='utf-8')
    proc = None

    def request(path, method='GET', data=None):
        body = None if data is None else json.dumps(data).encode()
        return urllib.request.urlopen(urllib.request.Request(base + path, data=body, method=method, headers={'X-LLMN-Client': '1', 'Content-Type': 'application/json'}), timeout=15)

    def api(path, method='GET', data=None):
        with request(path, method, data) as response:
            body = response.read()
            return json.loads(body) if body else None

    def start():
        nonlocal proc
        proc = subprocess.Popen([str(exe)], cwd=ROOT, env=env, stdout=log, stderr=log, creationflags=subprocess.CREATE_NO_WINDOW if os.name == 'nt' else 0)
        for _ in range(100):
            if proc.poll() is not None:
                raise RuntimeError((work / 'server.log').read_text())
            try:
                api('/init')
                return
            except OSError:
                time.sleep(.1)
        raise AssertionError('server did not start')

    def stop():
        if proc and proc.poll() is None:
            proc.kill()
            proc.wait(timeout=10)

    def wait_for(fn):
        deadline = time.monotonic() + 10
        while time.monotonic() < deadline:
            value = fn()
            if value:
                return value
            time.sleep(.05)
        raise AssertionError('condition timed out')

    def contact(name, model='chat'):
        return api('/contacts', 'POST', {'requestId': str(uuid.uuid4()), 'name': name, 'persona': 'persona-' + name, 'binding': None if model is None else {'providerId': 'test', 'modelId': model, 'reasoningEffort': None}})

    def direct(actor):
        return api(f'/contacts/{actor["id"]}/direct', 'POST', {'requestId': str(uuid.uuid4())})

    def history(cid):
        return api(f'/conversations/{cid}/messages')['messages']

    def send(cid, text, request_id=None, mentions=None, reply_to=None):
        return api(f'/conversations/{cid}/messages', 'POST', {'requestId': request_id or str(uuid.uuid4()), 'text': text, 'replyTo': reply_to, 'mentions': mentions or []})

    def complete(cid, text):
        send(cid, text)
        return wait_for(lambda: next((m for m in history(cid) if m['senderId'] != self_id and m['text'].startswith('回应：' + text) and m['status'] != 'generating'), None))

    def provider_count(text):
        return len(calls_for(text))

    def calls_for(text):
        return [r for r in REQUESTS if next(m['content'] for m in reversed(r['messages']) if m['role'] == 'user' and m['content'].startswith('[我 ·')).endswith('\n' + text)]

    def group(title, actors):
        return api('/groups', 'POST', {'requestId': str(uuid.uuid4()), 'title': title, 'memberIds': [a['id'] for a in actors]})

    def plans(cid):
        return api(f'/conversations/{cid}/plans')

    def settled(cid):
        return wait_for(lambda: plans(cid) if plans(cid) and all(p['status'] not in ('queued', 'running') for p in plans(cid)) else None)

    def member(cid, actor, action):
        current = api(f'/conversations/{cid}')
        return api(f'/conversations/{cid}/members', 'POST', {'requestId': str(uuid.uuid4()), 'actorId': actor['id'], 'action': action, 'expectedRevision': current['revision']})

    try:
        start()
        initial = api('/init')
        self_id = initial['selfId']
        assert initial['contacts'] == [] and initial['conversations'] == []
        alice, bob, draft = contact('Alice'), contact('Bob'), contact('Draft', None)
        private = direct(alice)
        assert private == direct(alice)
        draft_id = direct(draft)
        receipt = send(draft_id, 'saved-without-model')
        assert receipt['executionNotice']['code'] == 'model_unavailable'
        assert len(history(draft_id)) == 1 and provider_count('saved-without-model') == 0
        before = api('/init')
        try:
            contact('Invalid', 'missing')
            raise AssertionError('invalid model accepted')
        except urllib.error.HTTPError as error:
            assert error.code == 422 and json.load(error)['code'] == 'model_unavailable'
        assert len(api('/init')['contacts']) == len(before['contacts'])

        request_id = str(uuid.uuid4())
        with concurrent.futures.ThreadPoolExecutor() as pool:
            receipts = list(pool.map(lambda _: send(private, 'idempotent', request_id), range(5)))
        assert len({r['message']['id'] for r in receipts}) == 1
        wait_for(lambda: next((m for m in history(private) if m['runId'] and m['status'] == 'committed' and m['text'] == '回应：idempotent'), None))
        assert provider_count('idempotent') == 1
        message = history(private)[-1]
        details = api(f'/runs/{message["runId"]}')
        assert details['reasoning'] == 'fixture-private-reasoning' and details['usage']['total_tokens'] == 20
        assert details['wireModel'] == 'chat-wire'
        complete(private, 'second-turn')
        prompt = REQUESTS[-1]['messages']
        assert 'persona-Alice' in prompt[0]['content']
        assert any(m['role'] == 'assistant' and '回应：idempotent' in m['content'] for m in prompt)
        assert all('fixture-private-reasoning' not in str(m) for m in prompt)
        bob_id = direct(bob)
        complete(bob_id, 'separate-window')
        assert 'persona-Bob' in REQUESTS[-1]['messages'][0]['content']
        assert all('second-turn' not in str(m) for m in REQUESTS[-1]['messages'])

        send(private, 'slow-cancel')
        running = wait_for(lambda: api(f'/conversations/{private}')['activeRun'])
        started = time.monotonic()
        send(private, 'continuous-input')
        assert time.monotonic() - started < 1
        assert any(m['text'] == 'continuous-input' for m in history(private))
        api(f'/runs/{running["id"]}/cancel', 'POST')
        wait_for(lambda: any(m['runId'] == running['id'] and m['status'] == 'interrupted' for m in history(private)))
        wait_for(lambda: any(m['text'] == '回应：continuous-input' and m['status'] == 'committed' for m in history(private)))
        assert all('回应：slow-cancel' not in str(m) for m in REQUESTS[-1]['messages'])

        for text, expected in [('provider-error', 'provider_error'), ('eof', 'unexpected_eof'), ('tool', 'tool_not_allowed')]:
            isolated = direct(contact(text))
            send(isolated, text)
            failed = wait_for(lambda: next((m for m in history(isolated) if m['runId'] and m['status'] == 'interrupted'), None))
            assert api(f'/runs/{failed["runId"]}')['error']['code'] == expected

        send(private, 'slow-profile')
        wait_for(lambda: api(f'/conversations/{private}')['activeRun'])
        alice = api(f'/contacts/{alice["id"]}', 'PATCH', {'requestId': str(uuid.uuid4()), 'name': 'Alice', 'persona': 'persona-Alice-v2', 'binding': {'providerId': 'test', 'modelId': 'alt', 'reasoningEffort': None}, 'expectedRevision': alice['revision']})
        revised = wait_for(lambda: next((m for m in history(private) if m['text'].startswith('回应：slow-profile') and m['status'] == 'interrupted'), None))
        assert api(f'/runs/{revised["runId"]}')['error']['code'] == 'context_changed'
        assert direct(alice) == private
        complete(private, 'new-model')
        assert REQUESTS[-1]['model'] == 'alt-wire' and 'persona-Alice-v2' in REQUESTS[-1]['messages'][0]['content']

        db = work / 'data/social.sqlite3'
        with sqlite3.connect(db) as connection:
            connection.execute("CREATE TRIGGER fixture_save_failure BEFORE INSERT ON messages BEGIN SELECT RAISE(FAIL,'synthetic failure'); END")
        request_id = str(uuid.uuid4())
        before = len(history(private))
        try:
            send(private, 'save-retry', request_id)
            raise AssertionError('failed save acknowledged')
        except urllib.error.HTTPError as error:
            body = json.load(error)
            assert body['code'] == 'storage_failure' and str(work) not in body['message']
        assert len(history(private)) == before and provider_count('save-retry') == 0
        with sqlite3.connect(db) as connection:
            connection.execute('DROP TRIGGER fixture_save_failure')
        send(private, 'save-retry', request_id)
        wait_for(lambda: provider_count('save-retry') == 1 and history(private)[-1]['status'] == 'committed')

        stream = request('/subscribe?after=' + api('/init')['cursor'])
        send(private, 'slow-disconnect')
        while True:
            if b'event: generation' in stream.readline():
                break
        stream.close()
        wait_for(lambda: any(m['text'].startswith('回应：slow-disconnect') and m['status'] == 'committed' for m in history(private)))

        # Real groups: no text parsing, independent identities, durable deterministic FIFO.
        group_id = group('Study group', [alice, bob])
        other_group = group('Another space', [alice, bob])
        assert group_id != other_group
        send(group_id, '@Bob ordinary-message')
        assert not plans(group_id) and provider_count('@Bob ordinary-message') == 0
        request_id = str(uuid.uuid4())
        with concurrent.futures.ThreadPoolExecutor() as pool:
            receipts = list(pool.map(lambda _: send(group_id, 'group-ordered', request_id, [bob['id'], alice['id'], bob['id']]), range(5)))
        assert len({r['message']['id'] for r in receipts}) == 1
        assert receipts[0]['message']['mentions'] == [bob['id'], alice['id']]
        settled(group_id)
        calls = calls_for('group-ordered')
        assert len(calls) == 2 and 'persona-Bob' in calls[0]['messages'][0]['content'] and 'persona-Alice-v2' in calls[1]['messages'][0]['content']
        assert calls[0]['model'] == 'chat-wire' and calls[1]['model'] == 'alt-wire'
        assert '当前成员：' in calls[0]['messages'][0]['content'] and 'Alice' in calls[0]['messages'][0]['content']
        assert 'persona-Alice-v2' not in calls[0]['messages'][0]['content'], 'roster must not inject another AI private persona'
        assert any('[Bob ·' in m['content'] and '回应：group-ordered' in m['content'] and m['role'] == 'user' for m in calls[1]['messages'])
        assert all('new-model' not in str(m) and 'fixture-private-reasoning' not in str(m) for r in calls for m in r['messages'])
        assert history(other_group) == []
        ai_answer = next(m for m in history(group_id) if m['senderId'] == bob['id'])
        reply = send(group_id, 'group-reply', reply_to=ai_answer['id'])
        settled(group_id)
        assert [p['actorId'] for p in plans(group_id) if p['triggerId'] == reply['message']['id']] == [bob['id']]
        assert provider_count('group-reply') == 1
        assert any(f"（回复第 {ai_answer['sequence']} 条消息）" in m['content'] for m in calls_for('group-reply')[0]['messages'])
        try:
            send(other_group, 'bad-cross-space-reply', reply_to=ai_answer['id'])
            raise AssertionError('cross-space reply accepted')
        except urllib.error.HTTPError as error:
            assert error.code == 400
        assert history(other_group) == []
        first = send(group_id, 'queue-first', mentions=[bob['id'], alice['id']])
        started = time.monotonic()
        second = send(group_id, 'queue-second', mentions=[alice['id']])
        assert time.monotonic() - started < 1
        settled(group_id)
        ordered = sorted([p for p in plans(group_id) if p['triggerId'] in (first['message']['id'], second['message']['id'])], key=lambda p: (int(p['triggerSequence']), p['ordinal']))
        assert [p['actorId'] for p in ordered] == [bob['id'], alice['id'], alice['id']]
        assert [p['status'] for p in ordered] == ['succeeded'] * 3
        assert all('queue-second' not in str(m) for r in calls_for('queue-first') for m in r['messages'])
        member(group_id, draft, 'add')
        mixed = send(group_id, 'mixed-binding', mentions=[draft['id'], bob['id']])
        settled(group_id)
        mixed_plans = sorted([p for p in plans(group_id) if p['triggerId'] == mixed['message']['id']], key=lambda p: p['ordinal'])
        assert [p['status'] for p in mixed_plans] == ['failed', 'succeeded'] and mixed_plans[0]['failure']
        assert provider_count('mixed-binding') == 1
        carol = contact('Carol')
        member(group_id, carol, 'add')
        three = send(group_id, 'three-AI', mentions=[bob['id'], alice['id'], carol['id']])
        settled(group_id)
        assert len(calls_for('three-AI')) == 3
        assert [p['actorId'] for p in sorted([p for p in plans(group_id) if p['triggerId'] == three['message']['id']], key=lambda p:p['ordinal'])] == [bob['id'], alice['id'], carol['id']]

        scoped = group('History scope', [alice])
        send(scoped, 'shared-before-join')
        joined = member(scoped, bob, 'add')
        assert bob['id'] in joined['memberIds'] and all('historyFromSeq' not in m for m in joined['members'])
        send(scoped, 'visible-after-join', mentions=[bob['id']])
        settled(scoped)
        assert any('shared-before-join' in str(m) for m in calls_for('visible-after-join')[0]['messages'])
        send(scoped, 'slow-member-remove', mentions=[bob['id']])
        active = wait_for(lambda: api(f'/conversations/{scoped}')['activeRun'])
        queued = send(scoped, 'after-removal', mentions=[bob['id'], alice['id']])
        removed = member(scoped, bob, 'remove')
        assert bob['id'] not in removed['memberIds']
        settled(scoped)
        assert next(p for p in plans(scoped) if p['runId'] == active['id'])['status'] == 'cancelled'
        assert sorted((p['actorId'], p['status']) for p in plans(scoped) if p['triggerId'] == queued['message']['id']) == sorted([(bob['id'], 'cancelled'), (alice['id'], 'succeeded')])
        member(scoped, bob, 'add')
        send(scoped, 'visible-after-rejoin', mentions=[bob['id']])
        settled(scoped)
        assert any('shared-before-join' in str(m) for m in calls_for('visible-after-rejoin')[0]['messages'])

        # Crash recovery never resends the started response; untouched queue entries resume.
        recovery_group = group('Recovery group', [alice, bob])
        trigger = send(recovery_group, 'crash', mentions=[bob['id'], alice['id']])
        broken = wait_for(lambda: next((m for m in history(recovery_group) if m['status'] == 'generating' and m['text'].startswith('回应：crash')), None))
        before_calls = provider_count('crash')
        stop(); start()
        assert next(m for m in history(recovery_group) if m['id'] == broken['id'])['status'] == 'interrupted'
        recovered = settled(recovery_group)
        relevant = sorted([p for p in recovered if p['triggerId'] == trigger['message']['id']], key=lambda p: p['ordinal'])
        assert [p['status'] for p in relevant] == ['interrupted', 'succeeded']
        assert provider_count('crash') == before_calls + 1
        print('PASS: real groups, structured mentions/replies, FIFO/idempotency, per-AI bindings, same-trigger collaboration, window isolation, continuous input, mixed failure, shared group history, member revocation and queued crash recovery', flush=True)

        send(private, 'crash')
        interrupted = wait_for(lambda: next((m for m in history(private) if m['status'] == 'generating' and m['text'].startswith('回应：crash')), None))
        count = provider_count('crash')
        stop()
        start()
        restored = next(m for m in history(private) if m['id'] == interrupted['id'])
        assert restored['status'] == 'interrupted' and restored['text'].startswith('回应：crash')
        assert api(f'/runs/{restored["runId"]}')['reasoning'] == 'fixture-private-reasoning'
        time.sleep(.2)
        assert provider_count('crash') == count
        saved = history(private)
        stop()
        start()
        assert history(private) == saved and direct(alice) == private
        print('PASS: contacts/bindings, unique direct chat, independent persistence, strict models, duplicate request/run, context isolation/reasoning exclusion, continuous input/cancel, provider/EOF/tool failure, source change, save rollback/retry, subscription disconnect, crash recovery/no replay', flush=True)
        print(f'Fixture: {work}\nGUI: http://127.0.0.1:{port}', flush=True)
        if args.serve:
            print('Serving fixture until STOP file or interrupt.', flush=True)
            while not (work / 'STOP').exists():
                time.sleep(.25)
    finally:
        stop()
        provider.shutdown()
        log.close()


if __name__ == '__main__':
    main()
