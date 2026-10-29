"""Smoke-test a packaged Linux executable without model credentials or user data.

python3 scripts/linux/smoke.py --deb <package.deb> [--gui]
python3 scripts/linux/smoke.py --binary <installed-executable> --agent-directory <agent-resource-dir> [--gui]
python3 scripts/linux/smoke.py --appimage <package.AppImage> [--gui]
--gui uses Xvfb to check startup only, not GNOME/Wayland interaction.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import signal
import subprocess
import tempfile


def verify_agent(directory):
    record_path = directory / 'agent-artifact.json'
    record = json.loads(record_path.read_text(encoding='utf-8'))
    if not record.get('agentVersion') or 'linux' not in record.get('target', ''):
        raise RuntimeError(f'Invalid Linux Agent record: {record_path}')
    binary = directory / 'llmn-agent'
    if hashlib.sha256(binary.read_bytes()).hexdigest() != record.get('sha256'):
        raise RuntimeError(f'Bundled Agent SHA256 mismatch: {binary}')
    result = subprocess.run([str(binary.resolve()), '--version'], capture_output=True,
                            text=True, env={}, timeout=10, check=True)
    if result.stdout.strip() != f"llmn-agent {record['agentVersion']}":
        raise RuntimeError(f'Bundled Agent version mismatch: {binary}')
    print(f"PASS: bundled Agent {record['agentVersion']} ({record['target']}) version/SHA256")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    source = parser.add_mutually_exclusive_group(required=True)
    source.add_argument('--deb', type=Path)
    source.add_argument('--binary', type=Path)
    source.add_argument('--appimage', type=Path)
    parser.add_argument('--agent-directory', type=Path,
                        help='Required with --binary: installed agent resource directory')
    parser.add_argument('--gui', action='store_true')
    args = parser.parse_args()
    if args.binary and not args.agent_directory:
        parser.error('--binary requires --agent-directory; the bundled Agent is mandatory')
    work = Path(tempfile.mkdtemp(prefix='llmn-linux-smoke-'))
    print(f'Isolated artifacts/logs retained for inspection and cleanup: {work}')
    if args.deb:
        package = work / 'package'
        subprocess.run(['dpkg-deb', '-x', str(args.deb.resolve()), str(package)], check=True)
        binary = package / 'usr/bin/LLM-Nest'
    elif args.appimage:
        subprocess.run([str(args.appimage.resolve()), '--appimage-extract'],
                       cwd=work, check=True, stdout=subprocess.DEVNULL)
        package = work / 'squashfs-root'
        binary = package / 'usr/bin/LLM-Nest'
    else:
        package = None
        binary = args.binary.resolve()
    if not binary.is_file():
        raise RuntimeError(f'Executable missing: {binary}')
    if package:
        records = list(package.rglob('agent-artifact.json'))
        if len(records) != 1 or records[0].parent.name != 'agent':
            raise RuntimeError(f'Expected exactly one packaged agent/agent-artifact.json: {records}')
        agent_directory = records[0].parent
    else:
        agent_directory = args.agent_directory.resolve(strict=True)
    verify_agent(agent_directory)
    dependency_env = dict(os.environ)
    if args.appimage:
        # Match AppRun's bundled library search rather than requiring host copies.
        libraries = [package / 'usr/lib', *sorted((package / 'usr/lib').glob('*-linux-gnu'))]
        dependency_env['LD_LIBRARY_PATH'] = ':'.join(str(path) for path in libraries if path.is_dir())
    dependencies = subprocess.run(['ldd', str(binary)], capture_output=True, text=True,
                                  env=dependency_env, check=True)
    if 'not found' in dependencies.stdout:
        raise RuntimeError('Desktop runtime dependencies missing:\n' + dependencies.stdout)
    print('PASS: native executable dependencies')
    if args.gui:
        for command in ('dbus-run-session', 'xvfb-run'):
            if not shutil.which(command):
                raise RuntimeError(f'{command} is required for --gui')
        home = work / 'home'
        home.mkdir()
        env = dict(os.environ, HOME=str(home), XDG_CONFIG_HOME=str(home / 'config'),
                   XDG_DATA_HOME=str(home / 'data'), XDG_CACHE_HOME=str(home / 'cache'),
                   LLMN_DATA_DIR=str(work / 'sessions'))
        env.pop('LLMN_CONFIG', None)
        log = work / 'startup.log'
        with log.open('wb') as output:
            launch = package / 'AppRun' if args.appimage else binary
            proc = subprocess.Popen(['dbus-run-session', '--', 'xvfb-run', '-a', str(launch)],
                                    env=env, stdout=output, stderr=subprocess.STDOUT, start_new_session=True)
            try:
                try:
                    proc.wait(timeout=8)
                except subprocess.TimeoutExpired:
                    pass
                else:
                    raise RuntimeError(f'GUI exited early ({proc.returncode}): ' + log.read_text(errors='replace'))
                if (work / 'sessions/.llmn.lock').exists():
                    raise RuntimeError('Cloud desktop unexpectedly initialized the retired host Runtime')
            finally:
                # Only this test's process group; never targets existing user apps.
                try:
                    os.killpg(proc.pid, signal.SIGTERM)
                except ProcessLookupError:
                    pass
                try:
                    proc.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    os.killpg(proc.pid, signal.SIGKILL)
                    proc.wait()
        print(log.read_text(errors='replace'))
        print('PASS: isolated Xvfb startup (not frontend IPC or desktop interaction acceptance)')


if __name__ == '__main__':
    main()
