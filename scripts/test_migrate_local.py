"""A real sequential v13 SQLite fixture; never reads daily user data."""
import base64
import hashlib
import importlib.util
import json
from pathlib import Path
import sqlite3
import tempfile
import unittest
import uuid
from cloud_acceptance import png

ROOT=Path(__file__).resolve().parents[1]
spec=importlib.util.spec_from_file_location('migrate_local',ROOT/'scripts/migrate_local.py')
m=importlib.util.module_from_spec(spec);spec.loader.exec_module(m)

def build_fixture(directory):
    directory=Path(directory);directory.mkdir(exist_ok=True)
    (directory/'.llmn.lock').touch()
    db=sqlite3.connect(directory/'social.sqlite3')
    schema=ROOT/'server/cloud/tests/fixtures/local-v13'
    for version in range(1,14):
        db.executescript((schema/('schema.sql' if version==1 else f'schema-v{version}.sql')).read_text())
        db.execute('INSERT INTO schema_migrations VALUES(?,?,?)',(version,'fixture','2026-01-01T00:00:00Z'))
    human,ai,ai2,cid,rel=[str(uuid.uuid4()) for _ in range(5)]
    now='2026-01-01T00:00:00Z'
    for actor,kind in ((human,'human'),(ai,'ai'),(ai2,'ai')):
        p=dict(id=actor,kind=kind,display_name=kind,bio='',status='active',revision=1,created_at=now)
        db.execute('INSERT INTO actors VALUES(?,?,?,?,?)',(actor,kind,'active',1,json.dumps(p)))
        if kind=='ai':
            db.execute('INSERT INTO ai_profiles VALUES(?,?,?)',(actor,1,json.dumps(dict(actor_id=actor,persona='private persona',revision=1))))
            db.execute('INSERT INTO execution_bindings VALUES(?,?,?)',(actor,1,json.dumps(dict(actor_id=actor,id=str(uuid.uuid4()),provider_id='local',model_id='mock',reasoning_effort=None,revision=1))))
    c=dict(id=cid,kind='group',title='Fixture',status='archived',legacy=True,revision=1)
    db.execute('INSERT INTO conversations VALUES(?,?,?,?,?,?,?,?,?,?)',(cid,'group',None,'archived',0,1,1,1,2,json.dumps(c)))
    for actor in (human,ai,ai2):
        p=dict(conversation_id=cid,actor_id=actor,status='active',revision=1)
        db.execute('INSERT INTO memberships VALUES(?,?,?,?,?,?)',(cid,actor,'active',1,json.dumps(p),0))
    ids=[str(uuid.uuid4()) for _ in range(2)]
    for seq,mid in enumerate(ids,1):
        p=dict(id=mid,conversation_id=cid,sender_id=human,sender_name='human',sequence=seq,revision=1,status='committed',created_at=now,body=[dict(kind='text',text='hello'),dict(kind='file',name='empty.txt',mime='text/plain',data=[])],reply_to=None)
        db.execute('INSERT INTO messages VALUES(?,?,?,?,?,?,?,?,?)',(mid,cid,human,seq,1,'committed',None,None,json.dumps(p)))
    db.execute('DELETE FROM message_deliveries')
    db.execute('INSERT INTO message_deliveries VALUES(?,?,?)',(10,ids[1],cid))
    db.execute('INSERT INTO message_deliveries VALUES(?,?,?)',(20,ids[0],cid))
    db.execute('INSERT INTO conversation_list_state VALUES(?,?,?,?,?,?)',(cid,human,10,1,1,1))
    memory=str(uuid.uuid4())
    source=dict(kind='message',conversation_id=cid,message_id=ids[0],revision=1)
    mem=dict(id=memory,owner_actor_id=ai,conversation_id=cid,scope=dict(kind='conversation',conversation_id=cid),kind='fact',text='remember this',status='confirmed',revision=1,sources=[source,dict(kind='manual',author_id=human,revision=1)],created_by=human,updated_by=human,created_at=now,updated_at=now)
    db.execute('INSERT INTO memories VALUES(?,?,?,?,?,?,?)',(memory,cid,ai,'confirmed',1,now,json.dumps(mem)))
    summary=str(uuid.uuid4())
    summary_binding=dict(providerId='local',modelId='mock',reasoningEffort=None,maxTokens=256)
    item=dict(id=summary,conversationId=cid,fromSequence=1,throughSequence=2,text='shared summary',status='ready',revision=1,sources=[source],binding=summary_binding,settingsRevision=1,requestedBy=human,membershipRevision=1,participationEpoch=1,attempt=1,error=None,createdAt=now,updatedAt=now)
    db.execute('INSERT INTO conversation_summaries VALUES(?,?,?,?,?,?,?)',(summary,cid,1,2,'ready',1,json.dumps(item)))
    db.execute('INSERT INTO summary_settings VALUES(?,?)',(cid,json.dumps(dict(id=cid,enabled=True,binding=summary_binding,revision=1))))
    direct=str(uuid.uuid4())
    direct_ids=[str(uuid.uuid4()),str(uuid.uuid4())]
    a,b=sorted((human,ai))
    db.execute('INSERT INTO relationships VALUES(?,?,?,?)',(rel,a,b,json.dumps(dict(id=rel,participant_a=a,participant_b=b,revision=1))))
    db.execute('INSERT INTO conversations VALUES(?,?,?,?,?,?,?,?,?,?)',(direct,'direct',rel,'active',0,1,1,1,2,json.dumps(dict(id=direct,kind='direct',relationship_id=rel,title='Direct',status='active',legacy=False,revision=1))))
    for actor in (human,ai):
        db.execute('INSERT INTO memberships VALUES(?,?,?,?,?,?)',(direct,actor,'active',1,json.dumps(dict(conversation_id=direct,actor_id=actor,status='active',revision=1)),0))
    image_bytes=png()
    for i,mid in enumerate(direct_ids):
        body=[dict(kind='text',text='direct '+str(i))]
        if i==0:body += [dict(kind='image',mime='image/png',data=list(image_bytes)),dict(kind='file',name='note.txt',mime='text/plain',data=list(b'file content'))]
        p=dict(id=mid,conversation_id=direct,sender_id=human if i==0 else ai,sender_name='human' if i==0 else 'ai',sequence=i+1,revision=1,status='committed',created_at=now,body=body,reply_to=None)
        db.execute('INSERT INTO messages VALUES(?,?,?,?,?,?,?,?,?)',(mid,direct,p['sender_id'],i+1,1,'committed',None,None,json.dumps(p)))
    db.execute('DELETE FROM message_deliveries WHERE conversation_id=?',(direct,))
    db.execute('INSERT INTO message_deliveries VALUES(?,?,?)',(30,direct_ids[1],direct))
    db.execute('INSERT INTO message_deliveries VALUES(?,?,?)',(40,direct_ids[0],direct))
    db.execute('INSERT INTO conversation_list_state VALUES(?,?,?,?,?,?)',(direct,human,30,0,0,1))
    direct_memory=str(uuid.uuid4());mem.update(id=direct_memory,conversation_id=direct,scope=dict(kind='relationship',relationship_id=rel),sources=[dict(kind='message',conversation_id=direct,message_id=direct_ids[0],revision=1)])
    db.execute('INSERT INTO memories VALUES(?,?,?,?,?,?,?)',(direct_memory,direct,ai,'confirmed',1,now,json.dumps(mem)))
    db.commit();db.close()
    (directory/'llmn.toml').write_text('secret = "NEVER_UPLOAD_CREDENTIAL"')
    return dict(human=human,ai=ai,ai2=ai2,conversation=cid,messages=ids,direct=direct,directMessages=direct_ids)

class MigrationTests(unittest.TestCase):
    def test_prepare_read_position_and_no_secrets(self):
        with tempfile.TemporaryDirectory() as tmp:
            src=Path(tmp)/'old';ids=build_fixture(src);out=Path(tmp)/'new'
            before=hashlib.sha256((src/'social.sqlite3').read_bytes()).digest()
            m.prepare(src,out)
            cloud=m.read_json(out/'cloud.json')
            self.assertEqual(cloud['listState'][0]['readMessageIds'],[ids['messages'][1]])
            self.assertTrue(cloud['conversations'][0]['archived'])
            self.assertTrue(cloud['conversations'][0]['legacy'])
            self.assertEqual(cloud['messages'][0]['attachments'][0]['data'],'')
            self.assertNotIn('private persona',(out/'cloud.json').read_text())
            self.assertNotIn('NEVER_UPLOAD_CREDENTIAL',(out/'cloud.json').read_text())
            self.assertEqual(before,hashlib.sha256((src/'social.sqlite3').read_bytes()).digest())
            with self.assertRaises(FileExistsError):m.prepare(src,out)
            mapping=dict(importId=str(uuid.uuid4()),sourceId=ids['human'],actors=[dict(sourceId=x,targetId=str(uuid.uuid4())) for x in (ids['human'],ids['ai'],ids['ai2'])],conversations=[dict(sourceId=x,targetId=str(uuid.uuid4())) for x in (ids['conversation'],ids['direct'])],messages=[dict(sourceId=x,targetId=str(uuid.uuid4()),sourceRevision='1',targetRevision='1',targetPosition=str(i+101)) for i,x in enumerate(ids['messages']+ids['directMessages'])])
            m.write_new(out/'mapping.json',mapping)
            m.node_packages(out)
            packages=[m.read_json(p) for p in (out/'node-packages').glob('*.json')]
            self.assertEqual(len(packages),2)
            self.assertTrue(all(p['windows'][0]['summaries'][0]['item']['text']=='shared summary' for p in packages))
            self.assertTrue(all(p['windows'][0]['summaries'][0]['item']['fromPosition']=='101' for p in packages))
            memory=next(p['windows'][0]['memories'][0] for p in packages if p['windows'][0]['memories'])
            self.assertEqual(memory['status'],'confirmed')
            self.assertEqual(memory['source']['messageId'],mapping['messages'][0]['targetId'])

    def test_stale_source_and_wrong_mapping(self):
        with tempfile.TemporaryDirectory() as tmp:
            src=Path(tmp)/'old';ids=build_fixture(src);out=Path(tmp)/'new'
            db=sqlite3.connect(src/'social.sqlite3')
            row=db.execute('SELECT id,payload FROM conversation_summaries').fetchone()
            summary=json.loads(row[1]);summary['sources'][0]['revision']=2
            db.execute('UPDATE conversation_summaries SET payload=? WHERE id=?',(json.dumps(summary),row[0]));db.commit();db.close()
            m.prepare(src,out);cloud=m.read_json(out/'cloud.json')
            mapping=dict(importId=str(uuid.uuid4()),sourceId=ids['human'],actors=[dict(sourceId=x['id'],targetId=str(uuid.uuid4())) for x in cloud['actors']],conversations=[dict(sourceId=x['id'],targetId=str(uuid.uuid4())) for x in cloud['conversations']],messages=[dict(sourceId=x['id'],targetId=str(uuid.uuid4()),sourceRevision=x['revision'],targetRevision='1',targetPosition=str(i+1)) for i,x in enumerate(cloud['messages'])])
            m.write_new(out/'mapping.json',mapping);m.node_packages(out)
            for path in (out/'node-packages').glob('*.json'):
                summary=m.read_json(path)['windows'][0]['summaries'][0]['item']
                self.assertEqual(summary['status'],'stale');self.assertEqual(summary['text'],'')
            mapping['sourceId']=str(uuid.uuid4())
            (out/'mapping.json').write_text(json.dumps(mapping))
            with self.assertRaisesRegex(ValueError,'source'):m.node_packages(out)

if __name__=='__main__':unittest.main()
