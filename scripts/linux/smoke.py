"""Smoke-test a packaged Linux executable without model credentials or user data.

python3 scripts/linux/smoke.py --deb <package.deb> [--gui]
python3 scripts/linux/smoke.py --binary <installed-executable> [--gui]
python3 scripts/linux/smoke.py --appimage <package.AppImage> [--gui]
--gui uses Xvfb to check startup only, not GNOME/Wayland interaction.
"""
import argparse
import json
import os
from pathlib import Path
import shutil
import signal
import subprocess
import tempfile


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    source = parser.add_mutually_exclusive_group(required=True)
    source.add_argument('--deb', type=Path)
    source.add_argument('--binary', type=Path)
    source.add_argument('--appimage', type=Path)
    parser.add_argument('--gui', action='store_true')
    args = parser.parse_args()
    with tempfile.TemporaryDirectory(prefix='llmn-linux-smoke-') as directory:
        work = Path(directory)
        if args.deb:
            subprocess.run(['dpkg-deb', '-x', str(args.deb.resolve()), str(work / 'package')], check=True)
            binary = work / 'package/usr/bin/LLM-Nest'
        elif args.appimage:
            subprocess.run([str(args.appimage.resolve()), '--appimage-extract'],
                           cwd=work, check=True, stdout=subprocess.DEVNULL)
            binary = work / 'squashfs-root/usr/bin/LLM-Nest'
        else:
            binary = args.binary.resolve()
        if not binary.is_file():
            raise RuntimeError(f'Executable missing: {binary}')
        # Same empty environment as ProcessTool: catches missing runtime libraries.
        result = subprocess.run([str(binary), '--llmn-tool-worker', 'add'],
                                input=b'{"a":2,"b":3}', capture_output=True, env={}, timeout=10, check=True)
        assert json.loads(result.stdout) == {'result': {'sum': 5}}, result.stdout
        denied = subprocess.run([str(binary), '--llmn-tool-worker', 'shell'],
                                input=b'{}', capture_output=True, env={}, timeout=10, check=True)
        assert 'tool not allowed' in json.loads(denied.stdout)['error']
        print('PASS: packaged worker, cleared environment, fixed allowlist')
        if args.gui:
            for command in ('dbus-run-session', 'xvfb-run'):
                if not shutil.which(command):
                    raise RuntimeError(f'{command} is required for --gui')
            home = work / 'home'
            home.mkdir()
            config = work / 'llmn.toml'
            config.write_text('[providers]\n', encoding='utf-8')
            env = dict(os.environ, HOME=str(home), XDG_CONFIG_HOME=str(home / 'config'),
                       XDG_DATA_HOME=str(home / 'data'), XDG_CACHE_HOME=str(home / 'cache'),
                       LLMN_DATA_DIR=str(work / 'sessions'), LLMN_CONFIG=str(config))
            log = work / 'startup.log'
            with log.open('wb') as output:
                launch = work / 'squashfs-root/AppRun' if args.appimage else binary
                proc = subprocess.Popen(['dbus-run-session', '--', 'xvfb-run', '-a', str(launch)],
                                        env=env, stdout=output, stderr=subprocess.STDOUT, start_new_session=True)
                try:
                    try:
                        proc.wait(timeout=8)
                    except subprocess.TimeoutExpired:
                        pass
                    else:
                        raise RuntimeError(f'GUI exited early ({proc.returncode}): ' + log.read_text(errors='replace'))
                    if not (work / 'sessions/.llmn.lock').is_file():
                        raise RuntimeError('Frontend did not initialize Runtime through IPC: ' + log.read_text(errors='replace'))
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
            print('PASS: isolated Xvfb startup and frontend/Runtime IPC (not desktop interaction acceptance)')


if __name__ == '__main__':
    main()
