"""Disposable HTTPS migration and two-node continuity acceptance; only a local mock model is used."""
import argparse
import base64
import hashlib
from http.server import ThreadingHTTPServer
import io
import json
import os
from pathlib import Path
import re
import sqlite3
import subprocess
import threading
import time
import uuid
import zipfile
from agent_acceptance import wait_for
from cloud_acceptance import Client
from cloud_management_acceptance import Model
from migrate_local import prepare, node_packages, read_json, write_new
from test_migrate_local import build_fixture


def uid():return str(uuid.uuid4())


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    for key in ('binary','accounts','ca','workdir'):parser.add_argument('--'+key,required=True)
    parser.add_argument('--serve-ui',action='store_true')
    args=parser.parse_args()
    work=Path(args.workdir).resolve();repo=Path(__file__).resolve().parents[1]
    if work.is_relative_to(repo):
        assert subprocess.run(['git','check-ignore','-q','--',str(work)],cwd=repo).returncode==0,'Workdir must be gitignored'
    work.mkdir(parents=True,exist_ok=False)
    saved=read_json(args.accounts);ca=str(Path(args.ca).resolve());binary=str(Path(args.binary).resolve())
    clients=[Client(saved['url'],ca) for _ in saved['accounts'][:2]]
    assert len(clients)==2,'Two disposable test accounts required'
    for client,human in zip(clients,saved['accounts']):client.request('/api/auth/login','POST',{k:human[k] for k in ('email','password')})
    api,outsider=clients
    processes=[];logs=[];aid=None
    options={'creationflags':subprocess.CREATE_NO_WINDOW} if os.name=='nt' else {}
    env={**os.environ,'LLMN_MIGRATION_MOCK_KEY':'migration-local-mock-only'}
    Model.calls=[];Model.release.set()
    model=ThreadingHTTPServer(('127.0.0.1',0),Model)
    threading.Thread(target=model.serve_forever,daemon=True).start()
    def command(node,action,*extra):
        return subprocess.run([binary,action,str(node),*map(str,extra)],check=True,capture_output=True,env=env,**options)
    def managed():return next(x for x in api.request('/api/ai') if x['account']['id']==aid)
    def init_node(name):
        node=work/name;command(node,'init')
        (node/'node.toml').write_text(f'''server = {json.dumps(saved['url'])}
ca_file = {json.dumps(ca.replace(chr(92),'/'))}
node_id = "{uid()}"
node_name = "Migration acceptance {name}"
daily_calls = 100
daily_tokens = 3276800
[profile]
revision = "1"
persona = "initial fixture"
max_tokens = 128
[profile.selection]
provider = "local"
model = "mock"
[providers.local]
protocol = "openai"
base_url = "http://127.0.0.1:{model.server_port}/v1"
api_key = {{ env = "LLMN_MIGRATION_MOCK_KEY" }}
[providers.local.models.mock]
model = "mock"
context_window = 32768
max_tokens = 512
image_input = true
''',encoding='utf-8')
        path=node/'pair.log';log=path.open('wb');logs.append(log)
        pair=subprocess.Popen([binary,'pair',str(node)],stdout=log,stderr=log,env=env,**options);processes.append(pair)
        def code():
            match=re.search(r'pairing code (\S+)',path.read_text(encoding='utf-8'))
            assert match or pair.poll() is None,'Pairing exited; inspect private pair.log'
            return match.group(1) if match else None
        current=managed()
        api.request(f'/api/ai/{aid}/node','POST',dict(requestId=uid(),pairingCode=wait_for(code,bool),expectedBindingVersion=current['bindingVersion']))
        assert pair.wait(timeout=30)==0
        return node
    def import_package(node,path):
        preview=json.loads(command(node,'preview-import-ai',aid,path).stdout)
        if preview['type']=='alreadyImported':return preview
        return json.loads(command(node,'import-ai',aid,path,preview['expectedProfileRevision']).stdout)
    def start(node):
        log=(node/'run.log').open('ab');logs.append(log)
        process=subprocess.Popen([binary,'run-managed',str(node)],stdout=log,stderr=log,env=env,**options);processes.append(process)
        wait_for(lambda:managed()['node']['online'],bool)
        assert json.loads(command(node,'status').stdout)['running']
        return process
    def stop(node,process):
        command(node,'stop');assert process.wait(timeout=20)==0
        wait_for(lambda:managed()['node']['online'],lambda x:not x)
    def consciousness(cid,operation):
        return api.request(f'/api/chat/conversations/{cid}/ai/{aid}/consciousness','POST',dict(requestId=uid(),operation=operation))
    try:
        source=work/'old';ids=build_fixture(source);before=hashlib.sha256((source/'social.sqlite3').read_bytes()).digest()
        prepared=work/'prepared';prepare(source,prepared);snapshot=read_json(prepared/'cloud.json')
        assert 'private persona' not in json.dumps(snapshot) and 'NEVER_UPLOAD_CREDENTIAL' not in json.dumps(snapshot)
        preview=api.request('/api/migration/preview','POST',snapshot)
        body=dict(snapshot=snapshot,previewDigest=preview['previewDigest'])
        mapping=api.request('/api/migration/import','POST',body)
        assert api.request('/api/migration/import','POST',body)==mapping
        assert hashlib.sha256((source/'social.sqlite3').read_bytes()).digest()==before
        write_new(prepared/'mapping.json',mapping);node_packages(prepared)
        amap={x['sourceId']:x['targetId'] for x in mapping['actors']};cmap={x['sourceId']:x['targetId'] for x in mapping['conversations']};mmap={x['sourceId']:x for x in mapping['messages']}
        aid=amap[ids['ai']];direct=cmap[ids['direct']];group=cmap[ids['conversation']]
        init=api.request('/api/chat/init');direct_view=next(c for c in init['conversations'] if c['id']==direct);group_view=next(c for c in init['conversations'] if c['id']==group)
        assert direct_view['unreadCount']==0 and group_view['pinned'] and group_view['manualUnread']
        outsider.request(f'/api/chat/conversations/{direct}/messages',status=404)
        api.request(f'/api/chat/conversations/{group}/messages','POST',dict(requestId=uid(),text='read-only must reject'),status=409)
        public=api.request('/api/ai')
        assert 'private persona' not in json.dumps(public)
        for message in snapshot['messages']:
            target=mmap[message['id']]
            indices={x['sourceIndex']:x['targetIndex'] for x in target['attachments']}
            for attachment in message['attachments']:
                actual=api.request(f"/api/chat/messages/{target['targetId']}/attachments/{indices[attachment['sourceIndex']]}?revision={target['targetRevision']}")
                assert actual==base64.b64decode(attachment['data'])
        archive=api.request(f'/api/chat/conversations/{direct}/export?format=json&includeImages=true&includeFiles=true')
        with zipfile.ZipFile(io.BytesIO(archive)) as z:
            expected=[base64.b64decode(x['data']) for message in snapshot['messages'] if message['conversationId']==ids['direct'] for x in message['attachments']]
            actual=[z.read(p) for p in z.namelist() if p.startswith(('images/','files/'))]
            assert sorted(actual)==sorted(expected)
        print('PASS: migration/replay, source backup, read state, isolation, read-only and attachment ZIP bytes',flush=True)
        package=prepared/'node-packages'/f'{aid}.json'
        node_a=init_node('node-a');import_package(node_a,package);live_a=start(node_a)
        time.sleep(2);assert len(Model.calls)==0,'Import must not start models'
        memories=consciousness(direct,dict(type='memories'))['memories']
        memory=next(m for m in memories if m['text']=='remember this');assert memory['status']=='confirmed'
        summary_view=consciousness(group,dict(type='summaries'))
        summaries=summary_view['summaries']
        assert not summary_view['settings']['enabled']
        summary=next(s for s in summaries if s['text']=='shared summary')
        assert summary['status']=='ready' and summary['importOrigin']
        participation=api.request(f'/api/chat/conversations/{direct}/participation')
        assert participation['state']['paused']
        api.request(f'/api/chat/conversations/{direct}/participation/control','POST',dict(requestId=uid(),expectedRevision=participation['revision'],action='resume'))
        def send_and_finish(text):
            count=len(Model.calls)
            sent=api.request(f'/api/chat/conversations/{direct}/messages','POST',dict(requestId=uid(),text=text,images=[],files=[]))
            def responses():return next(m for m in api.request(f'/api/chat/conversations/{direct}/messages')['messages'] if m['id']==sent['id'])['aiResponses']
            wait_for(responses,lambda rows:rows and all(x['status']=='completed' for x in rows))
            assert len(Model.calls)==count+1
            transcript=json.dumps(Model.calls[-1]['messages'],ensure_ascii=False)
            assert 'private persona' in transcript and 'remember this' in transcript
            assert '[导入的旧聊天记录；不代表当前云端 AI 执行过该历史调用]' in transcript
            latest=api.request(f'/api/chat/conversations/{direct}/messages')['messages']
            assert any(m['senderId']==aid and 'management fixture reply' in m['text'] for m in latest)
        send_and_finish('MIGRATION_NODE_A_CONTINUITY')
        stop(node_a,live_a);bundle=work/'exported-ai.json';command(node_a,'export-ai',aid,bundle)
        with sqlite3.connect(node_a/'node.sqlite') as old_store:
            old_secret=old_store.execute('SELECT secret FROM bindings WHERE ai=?',(aid,)).fetchone()[0]
        old_store.close()
        node_b=init_node('node-b')
        old_client=Client(saved['url'],ca)
        old_client.request('/api/node/tasks',status=401,headers={'Authorization':'Bearer '+old_secret})
        del old_secret
        import_package(node_b,bundle);live_b=start(node_b)
        send_and_finish('MIGRATION_NODE_B_CONTINUITY')
        current=next(m for m in consciousness(direct,dict(type='memories'))['memories'] if m['id']==memory['id'])
        consciousness(direct,dict(type='forgetMemory',id=current['id'],revision=current['revision']))
        stop(node_b,live_b);count=len(Model.calls);import_package(node_b,bundle);live_b=start(node_b)
        forgotten=next(m for m in consciousness(direct,dict(type='memories'))['memories'] if m['id']==memory['id'])
        assert forgotten['status']=='forgotten' and not forgotten['text'] and len(Model.calls)==count
        print('PASS: local materials, two-node identity/context continuity, no replay or forgotten-memory revival',flush=True)
        assert hashlib.sha256((source/'social.sqlite3').read_bytes()).digest()==before
        write_new(work/'result.json',dict(aiId=aid,directId=direct,groupId=group,mockCalls=len(Model.calls),sourceId=mapping['sourceId']))
        if args.serve_ui:
            print('Fixtures online; create WORKDIR/stop after browser verification.',flush=True)
            while not (work/'stop').exists():time.sleep(1)
    finally:
        for process in processes:
            if process.poll() is None:
                process.terminate();process.wait(timeout=20)
        for log in logs:log.close()
        model.shutdown();model.server_close()
        if aid:
            current=managed()
            if current['node']:
                api.request(f'/api/ai/{aid}/node/revoke','POST',dict(requestId=uid(),expectedBindingVersion=current['bindingVersion']))


if __name__=='__main__':main()
