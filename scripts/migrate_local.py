#!/usr/bin/env python3
"""Offline v13 migration preparation and explicit HTTPS preview/import. No TOML is read."""
import argparse
import base64
import contextlib
import csv
import subprocess
import getpass
import http.cookiejar
import json
import os
from pathlib import Path
import sqlite3
import ssl
import urllib.parse
import urllib.request
import uuid

LIMIT = 128 * 1024 * 1024


def write_new(path, value):
    with open(path, 'x', encoding='utf-8') as stream:
        json.dump(value, stream, ensure_ascii=False, indent=2)


def save_result(path, value, replace=False):
    if path.exists():
        if read_json(path)==value:return
        if not replace:raise ValueError('Existing result differs: '+str(path))
        temporary=path.with_suffix('.new.json')
        write_new(temporary,value)
        temporary.replace(path)
    else:write_new(path,value)


def read_json(path):
    return json.loads(Path(path).read_text(encoding='utf-8'))


@contextlib.contextmanager
def directory_lock(directory):
    # The Rust guard uses flock on Unix and whole-file LockFileEx on Windows.
    with open(directory / '.llmn.lock', 'r+b') as stream:
        if os.name == 'nt':
            import msvcrt
            msvcrt.locking(stream.fileno(), msvcrt.LK_NBLCK, 1)
        else:
            import fcntl
            fcntl.flock(stream, fcntl.LOCK_EX | fcntl.LOCK_NB)
        try:
            yield
        finally:
            if os.name == 'nt':
                stream.seek(0)
                msvcrt.locking(stream.fileno(), msvcrt.LK_UNLCK, 1)
            else:
                fcntl.flock(stream, fcntl.LOCK_UN)


def payloads(db, table):
    return [json.loads(row[0]) for row in db.execute('SELECT payload FROM ' + table)]


def snapshot(db):
    if db.execute('SELECT MAX(version) FROM schema_migrations').fetchone()[0] != 13:
        raise ValueError('Only local schema v13 is supported; upgrade the old application first')
    actors = payloads(db, 'actors')
    human, = [a for a in actors if a['kind'] == 'human']
    uuid.UUID(human['id'])
    members = list(db.execute('SELECT conversation_id,actor_id,status FROM memberships'))
    allowed = {c for c, a, s in members if a == human['id'] and s == 'active'}
    deferred = []
    conversations = []
    for c in payloads(db, 'conversations'):
        if c['id'] not in allowed:
            deferred.append(dict(kind='excludedWindow', conversationId=c['id'], description='Local human is no longer a member; history and consciousness are not uploaded'))
            continue
        conversations.append(dict(id=c['id'],kind=c['kind'],name=c['title'],revision=str(c['revision']), archived=c['status']=='archived',legacy=c['legacy'],members=[dict(actorId=a,role='owner' if a==human['id'] else 'member',left=s=='left') for cid,a,s in members if cid==c['id']]))
    messages = []
    for m in payloads(db, 'messages'):
        if m['conversation_id'] not in allowed:
            continue
        text, attachments = [], []
        if m['status'] != 'withdrawn':
            for index, part in enumerate(m['body']):
                if part['kind'] == 'text':
                    text.append(part['text'])
                else:
                    attachments.append(dict(sourceIndex=index,kind=part['kind'],name=part.get('name'),mime=part.get('mime') or 'application/octet-stream',data=base64.b64encode(bytes(part['data'])).decode('ascii')))
        reference = None
        if m.get('context_reference'):
            r = m['context_reference']
            selection = r['selection']
            if selection['conversationId'] not in allowed:
                deferred.append(dict(kind='excludedReference',conversationId=m['conversation_id'],description='Reference in message '+m['id']+' points to an inaccessible window and is omitted'))
            else:
                reference = dict(conversationId=selection['conversationId'],sources=[dict(messageId=s['message_id'],revision=str(s['revision'])) for s in selection['sources'] if s['kind']=='message'],stale=r['stale'])
        messages.append(dict(id=m['id'],conversationId=m['conversation_id'],senderId=m['sender_id'],senderName=m['sender_name'],sequence=str(m['sequence']),revision=str(m['revision']),status='interrupted' if m['status']=='generating' else m['status'],createdAt=m['created_at'],text='\n'.join(text),attachments=attachments,replyTo=m['reply_to'],mentions=m.get('mentions',[]),reference=reference))
    states = []
    for cid, actor, read, unread, pinned in db.execute('SELECT conversation_id,actor_id,read_through,marked_unread,pinned FROM conversation_list_state'):
        if cid in allowed:
            read_ids = [r[0] for r in db.execute('SELECT message_id FROM message_deliveries WHERE conversation_id=? AND position<=? ORDER BY position',(cid,read))]
            states.append(dict(ownerId=actor,conversationId=cid,pinned=bool(pinned),manualUnread=bool(unread),readMessageIds=read_ids))
    for table in ('participation_policies','contact_settings','contact_intents'):
        for row in db.execute('SELECT conversation_id FROM '+table):
            # These are local preview reminders; no bodies/internal prompts are uploaded.
            deferred.append(dict(kind=table,conversationId=str(row[0]),description='Old '+table+' retained in backup; manually review and recreate after migration'))
    for memory in payloads(db,'memories'):
        dependencies=[s for s in memory['sources'] if s['kind']!='manual']
        if len(dependencies)>1 or any(s['kind']!='message' for s in dependencies):
            deferred.append(dict(kind='memoryNeedsReview',conversationId=memory['conversation_id'],description='Memory '+memory['id']+' has dependencies unsupported by the node; retained in backup, imported stale with empty text'))
    return dict(version=1,sourceId=human['id'],sourceHumanId=human['id'],actors=[dict(id=a['id'],kind=a['kind'],name=a['display_name'],bio=a.get('bio',''),status='disabled' if a['status']=='archived' else a['status'],revision=str(a['revision'])) for a in actors],contacts=[dict(ownerId=o,targetId=t,listed=bool(l),blocked=bool(b)) for o,t,l,b in db.execute('SELECT owner_id,account_id,listed,blocked FROM address_book')],conversations=conversations,messages=messages,listState=states,deferred=deferred)


def prepare(source, output):
    source, output = Path(source).resolve(), Path(output).resolve()
    # Refuse existing output, including partial prior attempts.
    output.mkdir(mode=0o700)
    if os.name=='nt':
        identity=subprocess.check_output(['whoami','/user','/fo','csv','/nh'],text=True)
        sid=next(csv.reader([identity.strip()]))[1]
        subprocess.run(['icacls',str(output),'/inheritance:r','/grant:r','*'+sid+':(OI)(CI)F'],check=True,stdout=subprocess.DEVNULL)
    with directory_lock(source):
        backup = output / 'local.sqlite3'
        with contextlib.closing(sqlite3.connect((source/'social.sqlite3').as_uri()+'?mode=ro', uri=True)) as old, contextlib.closing(sqlite3.connect(backup)) as new:
            old.backup(new)
        with contextlib.closing(sqlite3.connect(backup)) as db:
            cloud = snapshot(db)
            local_deferred={table:payloads(db,table) for table in ('participation_policies','contact_settings','contact_intents')}
            write_new(output/'deferred-local.json',local_deferred)
        if len(json.dumps(cloud,ensure_ascii=False).encode()) > LIMIT:
            raise ValueError('Cloud snapshot exceeds the 128 MiB endpoint limit; no upload attempted')
        write_new(output/'cloud.json', cloud)
        write_new(output/'preview-local.json',dict(sourceId=cloud['sourceId'],actors=len(cloud['actors']),conversations=len(cloud['conversations']),messages=len(cloud['messages']),deferred=cloud['deferred'],notice='Backup contains private consciousness. Only cloud.json may be uploaded. Exit the old program and switch to cloud after import; do not maintain two writable histories.'))


def request_client(server, ca, email):
    parsed = urllib.parse.urlparse(server)
    if parsed.scheme != 'https' or parsed.path not in ('','/') or parsed.query or parsed.fragment:
        raise ValueError('--server must be an HTTPS origin')
    origin = server.rstrip('/')
    opener = urllib.request.build_opener(urllib.request.HTTPSHandler(context=ssl.create_default_context(cafile=ca)),urllib.request.HTTPCookieProcessor(http.cookiejar.CookieJar()))
    def post(path, body):
        data = json.dumps(body,ensure_ascii=False).encode()
        if len(data)>LIMIT:
            raise ValueError('Request exceeds 128 MiB')
        req = urllib.request.Request(origin+path,data=data,headers={'Content-Type':'application/json','X-LLMN-Client':'1','Origin':origin})
        with opener.open(req, timeout=120) as response:
            return json.load(response)
    post('/api/auth/login',dict(email=email,password=getpass.getpass('Cloud password: ')))
    return post


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    sub=parser.add_subparsers(dest='command',required=True)
    p=sub.add_parser('prepare');p.add_argument('--source',required=True);p.add_argument('--output',required=True)
    for name in ('preview','apply'):
        p=sub.add_parser(name);p.add_argument('--directory',required=True);p.add_argument('--server',required=True);p.add_argument('--ca');p.add_argument('--email',required=True)
        if name=='apply':p.add_argument('--apply-digest',required=True,help='Exact previewDigest returned by the server')
    p=sub.add_parser('node-packages');p.add_argument('--directory',required=True)
    args=parser.parse_args()
    if args.command=='prepare':prepare(args.source,args.output);return
    if args.command=='node-packages':node_packages(args.directory);return
    directory=Path(args.directory)
    post=request_client(args.server,args.ca,args.email)
    cloud=read_json(directory/'cloud.json')
    if args.command=='preview':
        result=post('/api/migration/preview',cloud)
        save_result(directory/'preview-cloud.json',result,replace=True)
        print(json.dumps(result,ensure_ascii=False,indent=2))
    else:
        result=post('/api/migration/import',dict(snapshot=cloud,previewDigest=args.apply_digest))
        save_result(directory/'mapping.json',result)
        print('Import mapping saved to mapping.json')




def node_packages(directory):
    directory=Path(directory)
    mapping=read_json(directory/'mapping.json')
    cloud=read_json(directory/'cloud.json')
    if mapping['sourceId']!=cloud['sourceId']:raise ValueError('Mapping source does not match cloud snapshot')
    for category in ('actors','conversations','messages'):
        entries=mapping[category]
        if {e['sourceId'] for e in entries}!={e['id'] for e in cloud[category]} or len({e['targetId'] for e in entries})!=len(entries) or len({e['sourceId'] for e in entries})!=len(entries):
            raise ValueError('Mapping must be complete and unique: '+category)
    actors={m['sourceId']:m['targetId'] for m in mapping['actors']}
    windows={m['sourceId']:m['targetId'] for m in mapping['conversations']}
    messages={m['sourceId']:m for m in mapping['messages']}
    def source(s):
        m=messages[s['message_id']]
        return dict(messageId=m['targetId'],revision=m['targetRevision'])
    def valid(s):
        return s['kind']=='message' and s['message_id'] in messages and str(s['revision'])==messages[s['message_id']]['sourceRevision'] and local_messages[s['message_id']]['status']=='committed'
    def binding(b):
        return {k:b[k] for k in ('providerId','modelId','reasoningEffort','maxTokens')}
    packages=directory/'node-packages';packages.mkdir(mode=0o700)
    with contextlib.closing(sqlite3.connect((directory/'local.sqlite3').resolve().as_uri()+'?mode=ro',uri=True)) as db:
        profiles={p['actor_id']:p for p in payloads(db,'ai_profiles')}
        scopes={c['id']:c.get('relationship_id') for c in payloads(db,'conversations')}
        bindings={b['actor_id']:b for b in payloads(db,'execution_bindings')}
        active={(c,a) for c,a in db.execute("SELECT conversation_id,actor_id FROM memberships WHERE status='active'")}
        local_messages={x['id']:x for x in payloads(db,'messages')}
        memories=payloads(db,'memories')
        summaries=payloads(db,'conversation_summaries')
        settings={s['id']:s for s in payloads(db,'summary_settings')}
        for ai,p in profiles.items():
            b=bindings[ai]
            profile=dict(persona=p['persona'],providerId=b['provider_id'],modelId=b['model_id'],reasoningEffort=b.get('reasoning_effort'),temperature=b.get('temperature'),maxTokens=b.get('max_tokens'),autoDailyTokens=None)
            transfer=[]
            for cid,target in windows.items():
                if (cid,ai) not in active:continue
                converted=[]
                for m in memories:
                    if m['owner_actor_id']!=ai or m['conversation_id']!=cid:continue
                    sources=[x for x in m['sources'] if x['kind']!='manual']
                    scope=m['scope']
                    scope_valid=(scope['kind']=='conversation' and scope['conversation_id']==cid) or (scope['kind']=='actor' and scope['actor_id']==ai) or (scope['kind']=='relationship' and scope['relationship_id']==scopes[cid])
                    usable=scope_valid and m['status']=='confirmed' and len(sources)<=1 and all(valid(s) and s['conversation_id']==cid for s in sources)
                    status=m['status'] if usable or m['status']=='forgotten' else 'stale'
                    converted.append(dict(id=m['id'],revision=str(m['revision']),kind=m['kind'],text=m['text'] if usable else '',status=status,source=source(sources[0]) if usable and sources else None,createdAt=m['created_at'],updatedAt=m['updated_at']))
                converted_summaries=[]
                for s in summaries:
                    if s['conversationId']!=cid:continue
                    sources=s['sources']
                    usable=s['status']=='ready' and bool(sources) and all(valid(x) and x['conversation_id']==cid for x in sources)
                    positions=[int(messages[x['id']]['targetPosition']) for x in local_messages.values() if x['conversation_id']==cid and int(s['fromSequence'])<=int(x['sequence'])<=int(s['throughSequence']) and x['id'] in messages]
                    if not positions:
                        usable=False
                        positions=[1]
                    item=dict(id=str(uuid.uuid5(uuid.UUID(ai),s['id'])),revision=str(s['revision']),status='ready' if usable else 'stale',text=s['text'] if usable else '',fromPosition=str(min(positions)),throughPosition=str(max(positions)),sources=[source(x) for x in sources if valid(x)],error=None,createdAt=s['createdAt'],updatedAt=s['updatedAt'])
                    item['importOrigin']='local:'+mapping['sourceId']
                    converted_summaries.append(dict(item=item,binding=binding(s['binding']),requestedBy=actors[s['requestedBy']]))
                # Rebind and explicitly enable summaries after local model setup.
                old=settings.get(cid)
                setting=dict(revision='1',enabled=False,binding=binding(old['binding']) if old and old.get('binding') else None)
                transfer.append(dict(conversationId=target,memories=converted,settings=setting,summaries=converted_summaries))
            write_new(packages/(actors[ai]+'.json'),dict(version=1,packageId=str(uuid.uuid5(uuid.UUID(mapping['sourceId']),mapping['importId']+ai)),origin='local',aiId=actors[ai],profile=profile,windows=transfer))


if __name__=='__main__':
    main()
