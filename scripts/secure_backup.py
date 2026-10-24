#!/usr/bin/env python3
"""Create and verify a private, consistent backup before encrypted history migration."""
import argparse
import contextlib
import csv
import hashlib
import json
import os
from pathlib import Path
import shlex
import shutil
import subprocess
import uuid


def digest(path):
    with Path(path).open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def private_directory(path):
    path.mkdir(parents=True, exist_ok=False, mode=0o700)
    if os.name == 'nt':
        sid = list(csv.reader(subprocess.check_output(['whoami', '/user', '/fo', 'csv', '/nh'], text=True).splitlines()))[0][1]
        subprocess.run(['icacls', str(path), '/inheritance:r', '/grant:r', '*' + sid + ':(OI)(CI)F'], check=True, stdout=subprocess.DEVNULL)


@contextlib.contextmanager
def node_lock(directory):
    with (directory / 'node.lock').open('r+b') as stream:
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


# Runs as root on the explicitly selected deployment host. No credentials are
# returned or passed in argv: compose reads the existing private configuration.
REMOTE = r'''
import hashlib,json,os,pathlib,subprocess,sys,uuid
os.umask(0o077)
root=pathlib.Path(sys.argv[1]).resolve()
backup=root/'backups'/('before-encryption-'+sys.argv[2])
backup.mkdir(parents=True,exist_ok=False,mode=0o700)
compose=['docker','compose','--env-file',str(root/'private/cloud.env'),'-f',str(root/'src/server/cloud/deploy/compose.yaml')]
def run(args,**kwargs):
    return subprocess.run(args,check=True,stderr=subprocess.PIPE,**kwargs)
def text(args,input=None):
    return run(args,input=input,text=True,stdout=subprocess.PIPE).stdout.strip()
pg=compose+['exec','-T','postgres']
def counts(database):
    query="SELECT tablename FROM pg_tables WHERE schemaname='public' ORDER BY tablename"
    tables=text(pg+['psql','-X','-At','-U','llmn','-d',database,'-c',query]).splitlines()
    return {table:int(text(pg+['psql','-X','-At','-U','llmn','-d',database,'-c','SELECT count(*) FROM "'+table.replace('"','""')+'"'])) for table in tables}
container=text(compose+['ps','-q','app'])
running=bool(container) and text(['docker','inspect','-f','{{.State.Running}}',container])=='true'
try:
    if running: run(compose+['stop','app'],stdout=subprocess.PIPE)
    with (backup/'database.dump').open('xb') as output:
        run(pg+['pg_dump','-U','llmn','-d','llmn','--format=custom'],stdout=output)
        output.flush();os.fsync(output.fileno())
    run(['tar','-cpf',str(backup/'attachments.tar'),'-C',str(root/'data'),'attachments'],stdout=subprocess.PIPE)
    expected=counts('llmn')
finally:
    if running: run(compose+['start','app'],stdout=subprocess.PIPE)
# Restore into an independent database, never over the deployment database.
test_database='llmn_backup_check_'+uuid.uuid4().hex
run(pg+['createdb','-U','llmn',test_database],stdout=subprocess.PIPE)
try:
    with (backup/'database.dump').open('rb') as source:
        run(pg+['pg_restore','-U','llmn','-d',test_database,'--exit-on-error'],stdin=source,stdout=subprocess.PIPE)
    if counts(test_database)!=expected: raise RuntimeError('Restored table counts differ')
finally:
    run(pg+['dropdb','-U','llmn',test_database],stdout=subprocess.PIPE)
files=[]
for name in ['database.dump','attachments.tar']:
    path=backup/name
    with path.open('rb') as stream: sha=hashlib.file_digest(stream,'sha256').hexdigest()
    files.append(dict(name=name,bytes=path.stat().st_size,sha256=sha))
result=dict(path=str(backup),files=files,tables=expected,restoreVerified=True)
(backup/'manifest.json').write_text(json.dumps(result,ensure_ascii=False,indent=2),encoding='utf-8')
print(json.dumps(result))
'''


def prepare(host, server_root, output, nodes):
    output = Path(output).resolve()
    nodes = [Path(node).resolve(strict=True) for node in nodes]
    if any(output == node or output.is_relative_to(node) for node in nodes):
        raise ValueError('Backup output must be outside every source node directory')
    private_directory(output)
    with contextlib.ExitStack() as locks:
        for node in nodes:
            locks.enter_context(node_lock(node))
        # Snapshot stopped nodes, including their protected SDK stores. These are
        # recovery backups only, never a second active node or an ordinary export.
        for index, node in enumerate(nodes):
            shutil.copytree(node, output / 'nodes' / str(index), ignore=shutil.ignore_patterns('bin', '*.log', 'node.lock', 'direct-endpoint.json', 'agent.pid'))
        command = 'sudo -n python3 - ' + shlex.quote(server_root) + ' ' + uuid.uuid4().hex
        result = subprocess.run(['ssh', '-o', 'BatchMode=yes', host, command], input=REMOTE, text=True, capture_output=True)
        if result.returncode:
            # Remote exceptions can contain private paths; retain them only here.
            (output / 'backup-error.private.txt').write_text(result.stderr, encoding='utf-8')
            raise RuntimeError('Backup did not complete; inspect backup-error.private.txt')
        remote = json.loads(result.stdout)
        for item in remote['files']:
            remote_path = str(Path(remote['path']).as_posix()) + '/' + item['name']
            with (output / item['name']).open('xb') as target:
                subprocess.run(['ssh', '-o', 'BatchMode=yes', host, 'sudo -n cat -- ' + shlex.quote(remote_path)], check=True, stdout=target)
                target.flush(); os.fsync(target.fileno())
            if digest(output / item['name']) != item['sha256']:
                raise RuntimeError('Downloaded backup checksum differs')
        files = [{'name': str(p.relative_to(output)).replace('\\', '/'), 'bytes': p.stat().st_size, 'sha256': digest(p)} for p in sorted(output.rglob('*')) if p.is_file()]
        manifest = dict(version=1, serverHost=host, serverRoot=server_root, remote=remote, nodeSources=[str(p) for p in nodes], files=files, containsSecrets=True, cleanup='Retain until encrypted migration and independent recovery checks are accepted. Delete only with explicit approval.')
        (output / 'manifest.json').write_text(json.dumps(manifest, ensure_ascii=False, indent=2), encoding='utf-8')
    print('Backup verified; backupDigest=' + digest(output / 'manifest.json'))


def verify(directory):
    directory = Path(directory).resolve(strict=True)
    manifest = json.loads((directory / 'manifest.json').read_text(encoding='utf-8'))
    for item in manifest['files']:
        path = (directory / item['name']).resolve(strict=True)
        if not path.is_relative_to(directory):
            raise ValueError('Backup manifest points outside the backup directory')
        if path.stat().st_size != item['bytes'] or digest(path) != item['sha256']:
            raise ValueError('Backup checksum differs: ' + item['name'])
    if not manifest['remote']['restoreVerified']:
        raise ValueError('Database recovery was not verified')
    print('Backup verified; backupDigest=' + digest(directory / 'manifest.json'))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest='command', required=True)
    p = sub.add_parser('prepare')
    for name in ('server-host', 'server-root', 'output'):
        p.add_argument('--' + name, required=True)
    p.add_argument('--node-dir', action='append', default=[], help='Stopped local node; may be repeated. Includes secrets in the private recovery backup.')
    v = sub.add_parser('verify'); v.add_argument('--directory', required=True)
    args = parser.parse_args()
    if args.command == 'prepare':
        prepare(args.server_host, args.server_root, args.output, args.node_dir)
    else:
        verify(args.directory)


if __name__ == '__main__':
    main()
