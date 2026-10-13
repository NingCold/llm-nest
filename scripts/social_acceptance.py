"""M1/M2 social HTTP acceptance: temporary data and a loopback mock provider.
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

    summary_mode = 'ok'

    def summary_response(self, request):
        mode = Provider.summary_mode
        if mode == 'error':
            self.send_response(429); self.end_headers(); return
        self.send_response(200); self.send_header('Content-Type', 'text/event-stream'); self.end_headers()
        def emit(delta, finish=None):
            data = {'id':'summary-fixture','model':request['model'],'choices':[{'index':0,'delta':delta,'finish_reason':finish}]}
            self.wfile.write(('data: ' + json.dumps(data, ensure_ascii=False) + '\n\n').encode()); self.wfile.flush()
        try:
            if mode == 'slow':
                for _ in range(100):
                    time.sleep(.05); emit({'reasoning_content':'background-thought'})
            if mode == 'tool':
                emit({'tool_calls':[{'index':0,'id':'summary-tool','type':'function','function':{'name':'add','arguments':'{}'}}]}, 'tool_calls')
            else:
                if mode != 'empty': emit({'content':'窗口摘要：我提出项目采用蓝色界面；仍需确认发布日期。来源为本段原文。'})
                if mode == 'eof': return
                emit({}, 'stop')
            self.wfile.write(b'data: [DONE]\n\n'); self.wfile.flush()
        except (BrokenPipeError, ConnectionResetError, ConnectionAbortedError):
            pass

    def do_POST(self):
        request = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
        REQUESTS.append(request)
        if any(m['content'].startswith('[窗口分段摘要原文]') for m in request['messages'] if m['role'] == 'user'):
            return self.summary_response(request)
        content = next(m['content'] for m in reversed(request['messages']) if m['role'] == 'user' and (m['content'].startswith('[我 ·') or m['content'].startswith('[用户明确授权的一次性约定')))
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
                        time.sleep(getattr(self, 'stream_delay', .05))
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
    config.write_text(f'[gui]\ntemperature=1.75\nmaxTokens=7\n[providers.test]\nprotocol="openai"\nbase_url="http://127.0.0.1:{provider.server_port}/v1"\napi_key="fixture-only"\n[providers.test.models.chat]\nmodel="chat-wire"\ncontext_window=8192\nmax_tokens=1024\n[providers.test.models.alt]\nmodel="alt-wire"\ncontext_window=8192\n', encoding='utf-8')
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

    def managed(value):
        return dict(value['account'], persona=value['persona'], binding=value['binding'])

    def contact(name, model='chat', temperature=None, max_tokens=None):
        return managed(api('/ai-accounts', 'POST', {'requestId': str(uuid.uuid4()), 'name': name, 'bio':'public-' + name, 'persona': 'persona-' + name, 'binding': None if model is None else {'providerId': 'test', 'modelId': model, 'reasoningEffort': None, 'temperature':temperature,'maxTokens':max_tokens}}))

    def direct(actor):
        return api(f'/accounts/{actor["id"]}/direct', 'POST', {'requestId': str(uuid.uuid4())})

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
        return [r for r in REQUESTS if next((m['content'] for m in reversed(r['messages']) if m['role'] == 'user' and m['content'].startswith('[我 ·')), '').endswith('\n' + text)]

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
        assert initial['addressBook'] == [] and len(initial['accounts']) == 1 and initial['conversations'] == []
        # Parsing must fail before a command can write, and still return the shared error body.
        invalid_requests = [
            ('/ai-accounts', 'POST', b'{', 'application/json', 400),
            ('/ai-accounts', 'POST', b'{}', 'application/json', 422),
            ('/ai-accounts', 'POST', b'{"requestId":"bad-id","name":"invalid","persona":""}', 'application/json', 422),
            ('/ai-accounts', 'POST', b'[]', 'application/json', 422),
            ('/ai-accounts', 'POST', b'{}', 'text/plain', 415),
            ('/ai-accounts', 'POST', b'"' + b'x' * (2 * 1024 * 1024) + b'"', 'application/json', 413),
            ('/accounts/bad-id', 'GET', None, 'application/json', 400),
            ('/conversations/bad-id/messages', 'GET', None, 'application/json', 400),
            ('/messages/bad-id', 'GET', None, 'application/json', 400),
            ('/runs/bad-id/cancel', 'POST', None, 'application/json', 400),
            ('/ai-accounts/' + str(uuid.uuid4()) + '/view/conversations/bad-id/messages', 'GET', None, 'application/json', 400),
            ('/events?after=invalid', 'GET', None, 'application/json', 400),
            ('/subscribe?after=-1', 'GET', None, 'application/json', 400),
            ('/conversations/' + str(uuid.uuid4()) + '/messages?before=invalid', 'GET', None, 'application/json', 400),
        ]
        for path, method, body, content_type, status in invalid_requests:
            try:
                urllib.request.urlopen(urllib.request.Request(base + path, data=body, method=method,
                    headers={'X-LLMN-Client': '1', 'Content-Type': content_type}), timeout=15)
                raise AssertionError('malformed request accepted: ' + path)
            except urllib.error.HTTPError as error:
                assert error.code == status, (path, error.code)
                assert error.headers.get_content_type() == 'application/json', path
                rejection = json.load(error)
                assert rejection['code'] == 'invalid_input' and rejection['retryable'] is False and rejection['message'], rejection
        assert api('/init') == initial and not REQUESTS
        print('PASS: malformed JSON/path/query, unsupported content type and oversized body use structured non-retryable errors without writes', flush=True)
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
        assert len(api('/init')['accounts']) == len(before['accounts'])

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
        assert REQUESTS[-1]['max_tokens'] == 1024 and 'temperature' not in REQUESTS[-1]
        capability = next(m for m in api('/init')['models'] if m['modelId'] == 'chat')['generationLimits']['default']
        assert capability == {'temperatureMax':2.0,'minOutputTokens':1,'maxOutputTokens':1024}
        before_accounts = api('/init')['accounts']
        for temperature, max_tokens in [(2.1,128),(-.1,128),(.5,0),(.5,1025),(.5,8192),(.5,1.5)]:
            try:
                contact('Invalid parameters', 'chat', temperature, max_tokens)
                raise AssertionError('invalid parameters were saved')
            except urllib.error.HTTPError as error:
                assert 400 <= error.code < 500
        assert api('/init')['accounts'] == before_accounts
        parameter_a = contact('Parameters A','alt',0.25,4096)
        parameter_b = contact('Parameters B','alt',1.25,2048)
        parameter_private = direct(parameter_a)
        complete(parameter_private,'independent-parameters-private')
        assert calls_for('independent-parameters-private')[0]['temperature'] == .25
        assert calls_for('independent-parameters-private')[0]['max_tokens'] == 4096
        parameter_group = api('/groups','POST',{'requestId':str(uuid.uuid4()),'title':'Parameters group','memberIds':[parameter_a['id'],parameter_b['id']]})
        send(parameter_group,'independent-parameters-group',mentions=[parameter_a['id'],parameter_b['id']])
        wait_for(lambda: len([m for m in history(parameter_group) if m['runId'] and m['status']=='committed'])==2)
        parameter_requests = calls_for('independent-parameters-group')
        assert [(r['temperature'],r['max_tokens']) for r in parameter_requests]==[(.25,4096),(1.25,2048)]
        with sqlite3.connect(work/'data/social.sqlite3') as connection:
            snapshots = [json.loads(r[0])['snapshot']['budget'] for r in connection.execute('SELECT payload FROM ai_runs WHERE conversation_id=? ORDER BY rowid',(parameter_group,))]
        assert [int(s['reserved_output_tokens']) for s in snapshots]==[4096,2048]
        assert [int(s['input_token_limit']) for s in snapshots]==[4096,6144]
        print('PASS: independent temperature/output caps in private/group, no 1024 clamping, matching context reservation and rejected invalid saves',flush=True)

        # Public identities, personal contacts, and administrator privileges are separate.
        public = api('/init')
        assert public['selfAccount']['kind'] == 'human'
        assert set(api(f'/accounts/{alice["id"]}')) == set(public['selfAccount'])
        assert all('persona' not in a and 'binding' not in a and 'administrators' not in a for a in public['accounts'])
        assert api(f'/accounts/{alice["id"]}')['bio'] == 'public-Alice'
        root = public['selfAccount']
        root = api(f'/accounts/{self_id}', 'PATCH', {'requestId':str(uuid.uuid4()), 'name':root['name'], 'bio':'public human biography', 'expectedRevision':root['revision']})
        assert api('/init')['selfAccount'] == root
        relation = next(e for e in public['addressBook'] if e['accountId'] == alice['id'])
        send(private, 'slow-contact-removal')
        old_run = wait_for(lambda: api(f'/conversations/{private}')['activeRun'])
        removed = api(f'/contacts/{alice["id"]}', 'POST', {'requestId':str(uuid.uuid4()), 'listed':False, 'expectedRevision':relation['revision']})
        assert not removed['listed'] and api(f'/accounts/{alice["id"]}')['status'] == 'active'
        assert api(f'/conversations/{private}')['activeRun']['id'] == old_run['id']
        wait_for(lambda: any(m['runId'] == old_run['id'] and m['status'] == 'committed' for m in history(private)))
        assert complete(private, 'private-after-removal')['senderId'] == alice['id']
        relation_group = group('Relationship remains', [alice,bob])
        send(relation_group, 'group-after-removal', mentions=[alice['id']])
        assert settled(relation_group)[0]['status'] == 'succeeded'
        added = api(f'/contacts/{alice["id"]}', 'POST', {'requestId':str(uuid.uuid4()), 'listed':True, 'expectedRevision':removed['revision']})
        assert added['listed'] and direct(alice) == private
        managed_ai = api(f'/ai-accounts/{alice["id"]}')
        assert managed_ai['persona'] == 'persona-Alice' and managed_ai['isInitialAdministrator']
        granted = api(f'/ai-accounts/{alice["id"]}/administrators', 'POST', {'requestId':str(uuid.uuid4()), 'accountId':bob['id'], 'granted':True, 'expectedRevision':managed_ai['account']['revision']})
        assert len(granted['administrators']) == 2
        revoked = api(f'/ai-accounts/{alice["id"]}/administrators', 'POST', {'requestId':str(uuid.uuid4()), 'accountId':bob['id'], 'granted':False, 'expectedRevision':granted['account']['revision']})
        alice = managed(revoked)
        view = api(f'/ai-accounts/{alice["id"]}/view')
        assert view['operatorId'] == self_id and view['snapshot']['selfId'] == alice['id']
        assert {c['id'] for c in view['snapshot']['conversations']} == {private,relation_group}
        assert api(f'/ai-accounts/{alice["id"]}/view/conversations/{private}/messages')['messages'] == history(private)
        try:
            api(f'/ai-accounts/{alice["id"]}/view/conversations/{bob_id}/messages')
            raise AssertionError('AI viewpoint read another account\'s private chat')
        except urllib.error.HTTPError as error:
            assert error.code == 403
        member(relation_group,alice,'remove')
        try:
            api(f'/ai-accounts/{alice["id"]}/view/conversations/{relation_group}/messages')
            raise AssertionError('AI viewpoint read a group after leaving')
        except urllib.error.HTTPError as error:
            assert error.code == 403
        assert all(c['id'] != relation_group for c in api(f'/ai-accounts/{alice["id"]}/view')['snapshot']['conversations'])
        print('PASS: identical public account fields without internal settings, own profile, independent contact removal during generation, continued private/group replies, re-add identity, grants/revocation, scoped read-only AI viewpoint and membership checks', flush=True)

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
        alice = managed(api(f'/ai-accounts/{alice["id"]}', 'PATCH', {'requestId': str(uuid.uuid4()), 'name': 'Alice', 'bio':'public-Alice-v2', 'persona': 'persona-Alice-v2', 'binding': {'providerId': 'test', 'modelId': 'alt', 'reasoningEffort': None,'temperature':.5,'maxTokens':4096}, 'expectedRevision': alice['revision']}))
        revised = wait_for(lambda: next((m for m in history(private) if m['text'].startswith('回应：slow-profile') and m['status'] == 'committed'), None))
        assert api(f'/runs/{revised["runId"]}')['model'] == 'chat'
        assert api(f'/runs/{revised["runId"]}')['error'] is None
        old_request=calls_for('slow-profile')[0]
        assert 'temperature' not in old_request and old_request['max_tokens']==1024
        assert direct(alice) == private
        complete(private, 'new-model')
        assert REQUESTS[-1]['model'] == 'alt-wire' and 'persona-Alice-v2' in REQUESTS[-1]['messages'][0]['content']
        assert REQUESTS[-1]['temperature']==.5 and REQUESTS[-1]['max_tokens']==4096

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

        # Revisions retain following messages; withdrawal and controls commit before returning.
        controls = group('Conversation controls', [alice, bob])
        original = send(controls, 'slow-edit-source', mentions=[bob['id'], alice['id']])['message']
        old_run = wait_for(lambda: api(f'/conversations/{controls}')['activeRun'])
        following = send(controls, 'following-message')['message']
        edit = {'requestId': str(uuid.uuid4()), 'expectedRevision': original['revision'], 'text': 'edited-source'}
        revised = api(f"/messages/{original['id']}/revise", 'POST', edit)
        assert revised['id'] == original['id'] and revised['text'] == 'edited-source'
        assert api(f"/messages/{original['id']}/revise", 'POST', edit) == revised
        settled(controls)
        assert next(p for p in plans(controls) if p['runId'] == old_run['id'])['status'] == 'interrupted'
        assert [p['status'] for p in plans(controls) if p['triggerId'] == original['id'] and p['actorId'] == alice['id']] == ['cancelled']
        assert any(m['id'] == following['id'] for m in history(controls))
        assert api(f"/runs/{old_run['id']}")['error']['code'] == 'context_changed'
        withdraw = {'requestId': str(uuid.uuid4()), 'expectedRevision': revised['revision']}
        withdrawn = api(f"/messages/{original['id']}/withdraw", 'POST', withdraw)
        assert withdrawn['status'] == 'withdrawn' and withdrawn['text'] == ''
        assert api(f"/messages/{original['id']}/revise", 'POST', edit) == withdrawn
        assert api(f"/messages/{original['id']}") == withdrawn
        def control(action):
            current = api(f'/conversations/{controls}')
            return api(f'/conversations/{controls}/control', 'POST', {'requestId': str(uuid.uuid4()), 'expectedRevision': current['revision'], 'action': action})
        def pause_member(actor, paused):
            current = api(f'/conversations/{controls}')
            membership = next(m for m in current['members'] if m['actorId'] == actor['id'])
            return api(f'/conversations/{controls}/members/participation', 'POST', {'requestId': str(uuid.uuid4()), 'expectedRevision': membership['revision'], 'actorId': actor['id'], 'paused': paused})
        pause_member(bob, True)
        shared = send(controls, 'member-paused', mentions=[bob['id'], alice['id']])['message']
        settled(controls)
        assert provider_count('member-paused') == 1 and 'persona-Alice-v2' in calls_for('member-paused')[0]['messages'][0]['content']
        assert all('slow-edit-source' not in str(m) and 'edited-source' not in str(m) for m in calls_for('member-paused')[0]['messages'])
        control('pause')
        paused_msg = send(controls, 'space-paused', mentions=[alice['id']])
        assert paused_msg['executionNotice']['code'] == 'paused'
        assert not any(p['triggerId'] == paused_msg['message']['id'] for p in plans(controls))
        archived = control('archive')
        assert archived['status'] == 'archived'
        try:
            send(controls, 'cannot-send-to-archive')
            raise AssertionError('archived chat accepted a message')
        except urllib.error.HTTPError as error:
            assert error.code == 400
        assert control('restore')['paused']
        control('resume')
        assert provider_count('space-paused') == 0
        pause_member(bob, False)
        ending = send(controls, 'slow-end-bout', mentions=[bob['id'], alice['id']])['message']
        active = wait_for(lambda: api(f'/conversations/{controls}')['activeRun'])
        previous_bout = api(f'/conversations/{controls}')['currentBout']['id']
        ended = control('end')
        assert ended['currentBout'] is None and ended['activeRun'] is None and not ended['pendingPlans']
        assert all(p['status'] == 'cancelled' for p in plans(controls) if p['triggerId'] == ending['id'])
        send(controls, 'new-bout', mentions=[bob['id']]); settled(controls)
        assert api(f'/conversations/{controls}')['currentBout']['id'] != previous_bout
        assert any('following-message' in str(m) for m in calls_for('new-bout')[0]['messages'])
        print('PASS: revisions/withdrawal retain history, revoke old replies, preserve receipts; per-member pause, space pause/archive/restore and independent Bout ending', flush=True)

        # Administrator-controlled account lifecycle is global, while chat controls and group memberships remain independent.
        lifecycle_a, lifecycle_b = contact('Lifecycle A'), contact('Lifecycle B')
        lifecycle_private = direct(lifecycle_a)
        lifecycle_group = group('Contact lifecycle', [lifecycle_a, lifecycle_b])
        unrelated_group = group('Other active AI', [lifecycle_a, lifecycle_b])
        private_trigger = send(lifecycle_private, 'slow-life-private')['message']
        group_trigger = send(lifecycle_group, 'slow-life-group', mentions=[lifecycle_a['id'], lifecycle_b['id']])['message']
        other_trigger = send(unrelated_group, 'slow-life-other', mentions=[lifecycle_b['id'], lifecycle_a['id']])['message']
        private_fragment = wait_for(lambda: next((m for m in history(lifecycle_private) if m['status'] == 'generating' and m['text']), None))
        group_fragment = wait_for(lambda: next((m for m in history(lifecycle_group) if m['status'] == 'generating' and m['text']), None))
        wait_for(lambda: api(f'/conversations/{unrelated_group}')['activeRun'])
        send(lifecycle_private, 'life-queued-private')
        archive_input = {'requestId': str(uuid.uuid4()), 'expectedRevision': lifecycle_a['revision'], 'action': 'archive'}
        contact_path = f"/ai-accounts/{lifecycle_a['id']}/control"
        archived_contact = api(contact_path, 'POST', archive_input)
        assert archived_contact['status'] == 'archived' and api(f"/ai-accounts/{lifecycle_a['id']}")['binding'] == lifecycle_a['binding']
        assert api(contact_path, 'POST', archive_input) == archived_contact
        for cid, fragment in [(lifecycle_private, private_fragment), (lifecycle_group, group_fragment)]:
            saved_fragment = next(m for m in history(cid) if m['id'] == fragment['id'])
            assert saved_fragment['status'] == 'interrupted' and saved_fragment['text'].startswith(fragment['text'])
            assert saved_fragment['senderName'] == 'Lifecycle A'
            assert api(f'/conversations/{cid}')['status'] == 'active'
            assert lifecycle_a['id'] in api(f'/conversations/{cid}')['memberIds']
        other_run = api(f'/conversations/{unrelated_group}')['activeRun']
        assert other_run and other_run['actorId'] == lifecycle_b['id'], 'archive must not cancel another AI'
        settled(lifecycle_group); settled(unrelated_group)
        assert [p['status'] for p in sorted(plans(lifecycle_group), key=lambda p:p['ordinal']) if p['triggerId'] == group_trigger['id']] == ['cancelled', 'succeeded']
        assert [p['status'] for p in sorted(plans(unrelated_group), key=lambda p:p['ordinal']) if p['triggerId'] == other_trigger['id']] == ['succeeded', 'cancelled']
        assert provider_count('life-queued-private') == 0
        for data in [dict(archive_input, requestId=str(uuid.uuid4())), dict(archive_input, action='restore')]:
            try:
                api(contact_path, 'POST', data)
                raise AssertionError('stale or reused contact control accepted')
            except urllib.error.HTTPError as error:
                assert error.code == 409
        assert direct(lifecycle_a) == lifecycle_private
        archived_send = send(lifecycle_private, 'life-archived-private')
        assert archived_send['executionNotice']['code'] == 'account_disabled'
        mixed_send = send(lifecycle_group, 'life-archived-group', mentions=[lifecycle_a['id'], lifecycle_b['id']])
        assert mixed_send['executionNotice']['code'] == 'account_disabled'
        settled(lifecycle_group)
        assert provider_count('life-archived-private') == 0 and provider_count('life-archived-group') == 1
        current = api(f'/conversations/{lifecycle_private}')
        api(f'/conversations/{lifecycle_private}/control', 'POST', {'requestId':str(uuid.uuid4()), 'expectedRevision':current['revision'], 'action':'pause'})
        member = next(m for m in api(f'/conversations/{lifecycle_group}')['members'] if m['actorId'] == lifecycle_b['id'])
        api(f'/conversations/{lifecycle_group}/members/participation', 'POST', {'requestId':str(uuid.uuid4()), 'expectedRevision':member['revision'], 'actorId':lifecycle_b['id'], 'paused':True})
        stop(); start()
        persisted_contact = next(c for c in api('/init')['accounts'] if c['id'] == lifecycle_a['id'])
        assert persisted_contact == archived_contact
        assert direct(lifecycle_a) == lifecycle_private
        assert provider_count('life-queued-private') == 0
        restored_contact = api(contact_path, 'POST', {'requestId':str(uuid.uuid4()), 'expectedRevision':archived_contact['revision'], 'action':'restore'})
        assert restored_contact['status'] == 'active' and api(contact_path, 'POST', archive_input) == restored_contact
        assert api(f'/conversations/{lifecycle_private}')['paused']
        assert next(m for m in api(f'/conversations/{lifecycle_group}')['members'] if m['actorId'] == lifecycle_b['id'])['paused']
        current = api(f'/conversations/{lifecycle_private}')
        api(f'/conversations/{lifecycle_private}/control', 'POST', {'requestId':str(uuid.uuid4()), 'expectedRevision':current['revision'], 'action':'resume'})
        assert complete(lifecycle_private, 'life-after-restore')['senderId'] == lifecycle_a['id']
        assert direct(lifecycle_a) == lifecycle_private and provider_count('life-queued-private') == 0
        unused_contact = contact('Archived without chat', None)
        api(f"/ai-accounts/{unused_contact['id']}/control", 'POST', {'requestId':str(uuid.uuid4()), 'expectedRevision':unused_contact['revision'], 'action':'archive'})
        try:
            direct(unused_contact)
            raise AssertionError('archived contact created a private chat')
        except urllib.error.HTTPError as error:
            assert error.code == 400
        print('PASS: administrator account stop/restore, cross-space cancellation and FIFO continuation, saved fragments/history, archived sends, current-state receipts/conflicts, independent pause settings, restart/no replay and unique private identity', flush=True)

        from social_memory_acceptance import verify_memory
        verify_memory(api, contact, direct, group, complete, send, history, wait_for, stop, start, calls_for)

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
        persisted=api(f'/ai-accounts/{alice["id"]}')['binding']
        assert persisted['temperature']==.5 and persisted['maxTokens']==4096
        complete(private,'parameters-after-restart')
        assert REQUESTS[-1]['temperature']==.5 and REQUESTS[-1]['max_tokens']==4096
        print('PASS: contacts/bindings, unique direct chat, independent persistence, strict models, duplicate request/run, context isolation/reasoning exclusion, continuous input/cancel, provider/EOF/tool failure, source change, save rollback/retry, subscription disconnect, crash recovery/no replay', flush=True)
        from social_summary_acceptance import verify_summaries
        verify_summaries(api, contact, group, send, history, wait_for, stop, start, REQUESTS, Provider)

        from social_reference_acceptance import verify_references
        verify_references(api, contact, direct, group, send, history, wait_for, REQUESTS, stop, start)

        from social_participation_acceptance import verify_participation
        verify_participation(api, contact, direct, group, send, history, wait_for, REQUESTS, stop, start)

        from social_contact_acceptance import verify_contacts
        verify_contacts(api, contact, direct, group, send, history, wait_for, REQUESTS, stop, start)

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
