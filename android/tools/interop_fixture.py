"""Start a disposable HTTPS server for Android interop tests using an existing cloud-server binary.
Requires local initdb/pg_ctl, OpenSSL and Python. Never connects to an existing database.
Pass a NEW output directory. Create its STOP file to shut down the fixture.
"""
import argparse
import asyncio
import http.cookiejar
import json
import os
from pathlib import Path
import re
import secrets
import socket
import ssl
import subprocess
import threading
import time
import urllib.request


def port():
    with socket.socket() as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


def run(*args, **kwargs):
    return subprocess.run(list(args), check=True, stdin=subprocess.DEVNULL, **kwargs)


async def proxy(reader, writer, upstream):
    try:
        other, sink = await asyncio.open_connection("127.0.0.1", upstream)
    except OSError:
        writer.close()
        return

    async def copy(src, dest):
        while chunk := await src.read(65536):
            dest.write(chunk)
            await dest.drain()
    tasks = [asyncio.create_task(copy(reader, sink)), asyncio.create_task(copy(other, writer))]
    await asyncio.wait(tasks, return_when=asyncio.FIRST_COMPLETED)
    for task in tasks:
        task.cancel()
    await asyncio.gather(*tasks, return_exceptions=True)
    sink.close()
    writer.close()


def setup(args, root, tls_port):
    server_exe = str(Path(args.cloud_server).resolve(strict=True))
    for sub in ("site", "outbox", "res/raw", "res/xml"):
        (root / sub).mkdir(parents=True)
    (root / "site/index.html").write_text("Android integration fixture", encoding="utf-8")
    run("openssl", "req", "-x509", "-newkey", "rsa:2048", "-nodes", "-days", "2",
        "-subj", "/CN=LLM Nest local Android test", "-addext", "subjectAltName=IP:127.0.0.1",
        "-keyout", str(root / "tls.key"), "-out", str(root / "res/raw/android_test_ca.pem"),
        stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    (root / "res/xml/network_security_config.xml").write_text(
        '<network-security-config><base-config cleartextTrafficPermitted="false"><trust-anchors>'
        '<certificates src="system"/></trust-anchors></base-config><debug-overrides><trust-anchors>'
        '<certificates src="@raw/android_test_ca"/></trust-anchors></debug-overrides></network-security-config>', encoding="utf-8")
    password = secrets.token_urlsafe(25)
    (root / "pg-password").write_text(password + "\n", encoding="ascii")
    pg_port = port()
    pg = str(root / "pg")
    run("initdb", "-D", pg, "-U", "android_test", "-A", "scram-sha-256",
        "--pwfile", str(root / "pg-password"), "-E", "UTF8", "--locale=C", stdout=subprocess.DEVNULL)
    run("pg_ctl", "-D", pg, "-l", str(root / "pg.log"), "-o", f"-p {pg_port} -h 127.0.0.1", "-w", "start")
    try:
        upstream = port()
        origin = f"https://127.0.0.1:{tls_port}"
        env = {**os.environ, "CLOUD_PUBLIC_URL": origin, "CLOUD_BIND": f"127.0.0.1:{upstream}",
               "CLOUD_DATABASE_URL": f"postgresql://android_test:{password}@127.0.0.1:{pg_port}/postgres",
               "CLOUD_ATTACHMENTS_DIR": str(root / "attachments"), "CLOUD_MAIL_OUTBOX": str(root / "outbox"),
               "CLOUD_REQUIRE_ENCRYPTION": "false", "CLOUD_WEB_DIR": str(root / "site")}
        with (root / "cloud.log").open("w", encoding="utf-8") as log, subprocess.Popen(
                [server_exe], env=env, stdout=log, stderr=log, stdin=subprocess.DEVNULL) as child:
            try:
                for _ in range(120):
                    if child.poll() is not None:
                        raise RuntimeError("Fixture failed to start; see cloud.log")
                    try:
                        with socket.create_connection(("127.0.0.1", upstream), .3):
                            break
                    except OSError:
                        time.sleep(.25)
                loop = asyncio.new_event_loop()
                context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
                context.load_cert_chain(root / "res/raw/android_test_ca.pem", root / "tls.key")

                def serve():
                    asyncio.set_event_loop(loop)
                    server = loop.run_until_complete(asyncio.start_server(lambda r, w: proxy(r, w, upstream), "127.0.0.1", tls_port, ssl=context))
                    try:
                        loop.run_forever()
                    finally:
                        server.close()
                        loop.run_until_complete(server.wait_closed())

                threading.Thread(target=serve, daemon=True).start()
                identities = {"origin": origin}
                # Fixture-only registration. No invitations, emails or data leave localhost.
                def new_user(suffix):
                    client = urllib.request.build_opener(urllib.request.HTTPCookieProcessor(http.cookiejar.CookieJar()),
                        urllib.request.HTTPSHandler(context=ssl.create_default_context(cafile=str(root / "res/raw/android_test_ca.pem"))))
                    def post(path, body):
                        request = urllib.request.Request(origin + "/api" + path, json.dumps(body).encode(),
                            {"Content-Type": "application/json", "Origin": origin, "X-LLMN-Client": "1"})
                        with client.open(request, timeout=15) as response:
                            return json.load(response)
                    invite = run(server_exe, "invite", env=env, capture_output=True, text=True).stdout.strip()
                    email, pwd = f"{suffix.lower()}@android-test.invalid", secrets.token_urlsafe(18)
                    post("/auth/register", {"invite": invite, "email": email, "password": pwd, "name": "Android " + suffix})
                    mail = next(row for file in (root / "outbox").glob("*.json")
                                if (row := json.loads(file.read_text(encoding="utf-8")))["to"] == email)
                    token = re.search(r"token=([0-9a-f]+)", mail["body"]).group(1)
                    account = post("/auth/verify", {"token": token})["account"]
                    identities.update({f"email{suffix}": email, f"password{suffix}": pwd})
                    return post, account["id"]
                post_a, aid = new_user("A")
                post_b, bid = new_user("B")
                new_user("Recovery")  # Its first identity is separate from the two interop peers.
                for post, peer in ((post_a, bid), (post_b, aid)):
                    post("/chat/contacts", {"accountId": peer, "listed": True})
                (root / "fixture.json").write_text(json.dumps(identities), encoding="utf-8")
                print("Fixture ready. Private emulator arguments: fixture.json", flush=True)
                while not (root / "STOP").is_file():
                    if child.poll() is not None:
                        raise RuntimeError("Fixture server stopped")
                    time.sleep(.5)
                loop.call_soon_threadsafe(loop.stop)
            finally:
                child.terminate()
    finally:
        run("pg_ctl", "-D", pg, "-w", "-m", "fast", "stop")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--cloud-server", required=True)
    parser.add_argument("--output", required=True)
    options = parser.parse_args()
    destination = Path(options.output).resolve()
    destination.mkdir(mode=0o700, parents=False, exist_ok=False)
    setup(options, destination, port())
