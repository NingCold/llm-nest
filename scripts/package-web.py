"""Package an explicit Web dist directory with a fixed version and SHA256 manifest."""
import argparse
import hashlib
import json
import re
from pathlib import Path
import tarfile


def package(dist, output, version):
    if not re.fullmatch(r'[0-9]+\.[0-9]+\.[0-9]+(?:-[0-9A-Za-z.-]+)?', version):
        raise ValueError('An exact Web version is required')
    files = sorted(dist.rglob('*'))
    if not (dist / 'index.html').is_file():
        raise ValueError('Web dist must contain index.html')
    if any(p.is_symlink() for p in files):
        raise ValueError('Web dist must not contain symlinks')
    output.mkdir(parents=True, exist_ok=True)
    archive = output / f'llmn-web-{version}.tar.gz'
    manifest = output / 'web-manifest.json'
    if archive.exists() or manifest.exists():
        raise ValueError('Artifact or manifest already exists')
    with tarfile.open(archive, 'w:gz') as tar:
        for p in files:
            if p.is_file():
                tar.add(p, arcname=p.relative_to(dist).as_posix(), recursive=False)
    manifest.write_text(json.dumps({'schemaVersion': 1, 'webVersion': version, 'file': archive.name,
                                   'sha256': hashlib.sha256(archive.read_bytes()).hexdigest()}, indent=2) + '\n', encoding='utf-8')
    return archive


if __name__ == '__main__':
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--dist', type=Path, required=True)
    p.add_argument('--output', type=Path, required=True)
    p.add_argument('--version', required=True)
    a = p.parse_args()
    print(package(a.dist, a.output, a.version))
