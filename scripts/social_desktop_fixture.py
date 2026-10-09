"""Prepare isolated desktop QA data with a loopback mock provider, then keep it alive.

python scripts/social_desktop_fixture.py --server-binary PATH --manifest PATH --stop-file PATH
After 'READY', launch the Tauri CLI build with LLMN_CONFIG/LLMN_DATA_DIR from the manifest.
The seeding HTTP server exits before desktop startup so the data directory has one owner.
Create the stop file to stop the provider; the fixture data remains available for inspection.
"""
import argparse
import http.server
import json
import os
from pathlib import Path
import socket
import subprocess
import tempfile
import threading
import time
import urllib.request
import uuid

from social_acceptance import Provider, ROOT


class DesktopProvider(Provider):
    # Keep native refresh/close QA observable across several UI actions.
    stream_delay = .25


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--server-binary', required=True, type=Path)
    parser.add_argument('--manifest', required=True, type=Path)
    parser.add_argument('--stop-file', required=True, type=Path)
    args = parser.parse_args()
    if args.stop_file.exists():
        raise ValueError('Use a fresh stop file path')
    work = Path(tempfile.mkdtemp(prefix='llmn-desktop-qa-'))
    provider = http.server.ThreadingHTTPServer(('127.0.0.1', 0), DesktopProvider)
    threading.Thread(target=provider.serve_forever, daemon=True).start()
    config = work / 'llmn.toml'
    config.write_text(f'[providers.test]\nprotocol="openai"\nbase_url="http://127.0.0.1:{provider.server_port}/v1"\napi_key="fixture-only"\n[providers.test.models.chat]\nmodel="chat-wire"\ncontext_window=8192\nmax_tokens=1024\n', encoding='utf-8')
    with socket.socket() as sock:
        sock.bind(('127.0.0.1', 0))
        port = sock.getsockname()[1]
    env = os.environ.copy()
    env.update(LLMN_CONFIG=str(config), LLMN_DATA_DIR=str(work / 'data'), LLMN_PORT=str(port))
    base = f'http://127.0.0.1:{port}/api/social'

    def api(path, data=None):
        body = json.dumps(data).encode() if data is not None else None
        with urllib.request.urlopen(urllib.request.Request(base + path, data=body, headers={'X-LLMN-Client': '1', 'Content-Type': 'application/json'}), timeout=10) as response:
            return json.load(response)

    try:
        with (work / 'seed.log').open('w', encoding='utf-8') as log:
            server = subprocess.Popen([str(args.server_binary.resolve())], cwd=ROOT, env=env, stdout=log, stderr=log, creationflags=subprocess.CREATE_NO_WINDOW if os.name == 'nt' else 0)
            try:
                for _ in range(100):
                    if server.poll() is not None:
                        raise RuntimeError('Seed server exited; inspect seed.log')
                    try:
                        api('/init')
                        break
                    except OSError:
                        time.sleep(.1)
                else:
                    raise TimeoutError('Seed server did not start')
                actors = [api('/contacts', {'requestId': str(uuid.uuid4()), 'name': name, 'persona': '桌面验收角色', 'binding': {'providerId': 'test', 'modelId': 'chat', 'reasoningEffort': None}})['id'] for name in ['桌面甲', '桌面乙', '桌面丙']]
                direct = api(f'/contacts/{actors[0]}/direct', {'requestId': str(uuid.uuid4())})
                group = api('/groups', {'requestId': str(uuid.uuid4()), 'title': 'IPC 验收群', 'memberIds': actors})
            finally:
                server.terminate()
                server.wait(timeout=10)
        args.manifest.parent.mkdir(parents=True, exist_ok=True)
        args.manifest.write_text(json.dumps({'config': str(config), 'data': str(work / 'data'), 'webview': str(work / 'webview'), 'direct': direct, 'group': group, 'actors': actors}, ensure_ascii=False, indent=2), encoding='utf-8')
        print(f'READY: {args.manifest.resolve()}', flush=True)
        while not args.stop_file.exists():
            time.sleep(.5)
    finally:
        provider.shutdown()
        provider.server_close()


if __name__ == '__main__':
    main()
