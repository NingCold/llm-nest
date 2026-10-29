"""Package an already built Agent; never builds or reads another checkout."""
import argparse
import hashlib
import json
import re
from pathlib import Path
import shutil
import subprocess


def package(binary, output, version, target, url=None):
    if not re.fullmatch(r'[0-9]+\.[0-9]+\.[0-9]+(?:-[0-9A-Za-z.-]+)?', version) or not re.fullmatch(r'[a-zA-Z0-9_]+(?:-[a-zA-Z0-9_]+){2,}', target):
        raise ValueError('An exact version and target triple are required')
    actual = subprocess.check_output([str(binary.resolve()), '--version'], text=True).strip()
    if actual != f'llmn-agent {version}':
        raise ValueError(f'Agent version mismatch: {actual!r}')
    if url and not url.startswith('https://'):
        raise ValueError('Published artifact URL must use HTTPS')
    output.mkdir(parents=True, exist_ok=True)
    name = f'llmn-agent-{version}-{target}' + ('.exe' if 'windows' in target else '')
    destination = output / name
    if destination.exists() or (output / 'agent-manifest.json').exists():
        raise ValueError('Artifact or manifest already exists; choose an empty output directory')
    shutil.copy2(binary, destination)
    entry = {'sha256': hashlib.sha256(destination.read_bytes()).hexdigest(), 'file': name}
    if url:
        entry['url'] = url
    manifest = {'schemaVersion': 1, 'agentVersion': version, 'artifacts': {target: entry}}
    (output / 'agent-manifest.json').write_text(json.dumps(manifest, indent=2) + '\n', encoding='utf-8')
    return destination


if __name__ == '__main__':
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--binary', type=Path, required=True)
    p.add_argument('--output', type=Path, required=True)
    p.add_argument('--version', required=True)
    p.add_argument('--target', required=True)
    p.add_argument('--url')
    a = p.parse_args()
    print(package(a.binary, a.output, a.version, a.target, a.url))
