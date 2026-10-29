"""Create an isolated cloud Docker context from an explicit, verified Web artifact."""
import argparse
import hashlib
import json
from pathlib import Path, PurePosixPath
import shutil
import subprocess
import tarfile
import tomllib

ROOT = Path(__file__).resolve().parents[1]
MEMBERS = ('server/cloud', 'connector', 'crates/social')


def toml_value(value):
    if isinstance(value, dict):
        return '{' + ', '.join(f'{json.dumps(k)} = {toml_value(v)}' for k, v in value.items()) + '}'
    if isinstance(value, bool):
        return str(value).lower()
    return json.dumps(value)


def create(output, web, manifest_path, version, root=ROOT, lock=True):
    manifest = json.loads(manifest_path.read_text(encoding='utf-8'))
    if not version or version == 'latest' or manifest.get('schemaVersion') != 1 or manifest.get('webVersion') != version:
        raise ValueError('Web artifact version mismatch')
    if hashlib.sha256(web.read_bytes()).hexdigest() != manifest.get('sha256'):
        raise ValueError('Web artifact SHA256 mismatch')
    with tarfile.open(web, 'r:gz') as tar:
        entries = tar.getmembers()
        if any(not (e.isfile() or e.isdir()) or PurePosixPath(e.name).is_absolute() or '..' in PurePosixPath(e.name).parts or '\\' in e.name or ':' in e.name for e in entries):
            raise ValueError('Web archive contains an unsafe path or link')
        if not any(e.name == 'index.html' and e.isfile() for e in entries):
            raise ValueError('Web archive has no index.html')
        if output.exists() and any(output.iterdir()):
            raise ValueError('Build context must be empty')
        output.mkdir(parents=True, exist_ok=True)
        (output / 'web').mkdir()
        for entry in entries:
            target = output / 'web' / entry.name
            if entry.isdir():
                target.mkdir(parents=True, exist_ok=True)
            else:
                target.parent.mkdir(parents=True, exist_ok=True)
                target.write_bytes(tar.extractfile(entry).read())
    workspace = tomllib.loads((root / 'Cargo.toml').read_text(encoding='utf-8'))['workspace']
    names = set()
    for member in MEMBERS:
        package = tomllib.loads((root / member / 'Cargo.toml').read_text(encoding='utf-8'))
        for section, values in package.items():
            if section.endswith('dependencies'):
                names.update(k for k, v in values.items() if isinstance(v, dict) and v.get('workspace'))
        # Copy the tracked working-tree snapshot, including staged new files and local edits; never build/data directories.
        tracked = subprocess.check_output(['git', 'ls-files', '-z', '--', member], cwd=root).decode().split('\0')
        for name in filter(None, tracked):
            source = root / name
            if source.is_file():
                destination = output / name
                destination.parent.mkdir(parents=True, exist_ok=True)
                shutil.copy2(source, destination)
    text = '[workspace]\nresolver = "2"\nmembers = ' + toml_value(list(MEMBERS)) + '\n\n[workspace.package]\n'
    text += ''.join(f'{k} = {toml_value(v)}\n' for k, v in workspace['package'].items())
    text += '\n[workspace.dependencies]\n'
    text += ''.join(f'{json.dumps(k)} = {toml_value(workspace["dependencies"][k])}\n' for k in sorted(names))
    (output / 'Cargo.toml').write_text(text, encoding='utf-8')
    shutil.copy2(root / 'Cargo.lock', output / 'Cargo.lock')
    shutil.copy2(root / 'server/cloud/Dockerfile', output / 'Dockerfile')
    shutil.copy2(manifest_path, output / 'web-manifest.json')
    if lock:
        # Converge workspace members offline without fetching dependency source archives.
        # Pruning workspace members may rewrite the lock, but dependency versions must stay pinned.
        original = tomllib.loads((root / 'Cargo.lock').read_text(encoding='utf-8'))
        subprocess.run(['cargo', 'update', '--workspace', '--offline'], cwd=output, check=True, stdout=subprocess.DEVNULL)
        resolved = tomllib.loads((output / 'Cargo.lock').read_text(encoding='utf-8'))
        def identities(data):
            return {(p['name'], p['version'], p.get('source'), p.get('checksum')) for p in data['package']}
        if not identities(resolved) <= identities(original):
            raise ValueError('Cloud context resolution changed pinned dependencies; update the source lock explicitly')
    return output


if __name__ == '__main__':
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--output', type=Path, required=True)
    p.add_argument('--web-artifact', type=Path, required=True)
    p.add_argument('--web-manifest', type=Path, required=True)
    p.add_argument('--web-version', required=True)
    a = p.parse_args()
    print(create(a.output, a.web_artifact, a.web_manifest, a.web_version))
