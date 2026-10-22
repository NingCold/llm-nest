"""Exercise an isolated cloud deployment, with a second client over SSH.

Creates disposable invited accounts. Private mail and credentials stay in the
test environment and the explicitly supplied output file; no model is called.
"""
import argparse
import base64
import http.cookiejar
import io
import json
from pathlib import Path
import re
import secrets
import shlex
import ssl
import struct
import subprocess
import time
import urllib.error
import urllib.parse
import urllib.request
import uuid
import zipfile
import zlib


def remote(host, args, input=None):
    return subprocess.check_output(["ssh", "-o", "BatchMode=yes", host,
                                    " ".join(shlex.quote(str(a)) for a in args)], input=input)


class Client:
    def __init__(self, origin, ca):
        self.origin = origin.rstrip("/")
        self.cookies = http.cookiejar.CookieJar()
        self.opener = urllib.request.build_opener(
            urllib.request.HTTPCookieProcessor(self.cookies),
            urllib.request.HTTPSHandler(context=ssl.create_default_context(cafile=ca)))

    def request(self, path, method="GET", body=None, status=200, headers=None):
        h = {"X-LLMN-Client": "1", "Origin": self.origin, **(headers or {})}
        if body is not None:
            h["Content-Type"] = "application/json"
        req = urllib.request.Request(self.origin + path,
            None if body is None else json.dumps(body).encode(), h, method=method)
        try:
            response = self.opener.open(req, timeout=30)
        except urllib.error.HTTPError as error:
            response = error
        payload = response.read()
        assert response.status == status, (method, path, response.status, payload[:200])
        if "application/json" in response.headers.get("content-type", ""):
            return json.loads(payload)
        return payload


def png():
    def chunk(kind, data):
        return struct.pack("!I", len(data)) + kind + data + struct.pack("!I", zlib.crc32(kind+data))
    return b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack("!2I5B", 1, 1, 8, 6, 0, 0, 0)) + chunk(b"IDAT", zlib.compress(b"\0\x42\x88\xcc\xff")) + chunk(b"IEND", b"")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--url", required=True)
    parser.add_argument("--ca", required=True)
    parser.add_argument("--server-host", required=True)
    parser.add_argument("--server-root", required=True, help="Absolute directory containing src/, private/, data/")
    parser.add_argument("--client-host", required=True)
    parser.add_argument("--output", required=True, help="Ignored private JSON file for subsequent manual tests")
    args = parser.parse_args()
    root = args.server_root.rstrip("/")
    compose = ["sudo", "-n", "docker", "compose", "--env-file", root+"/private/cloud.env", "-f", root+"/src/server/cloud/deploy/compose.yaml"]
    clients = [Client(args.url, args.ca) for _ in range(3)]
    accounts = []
    suffix = secrets.token_hex(5)
    mail_script = "import pathlib,json,sys; rows=[json.loads(p.read_text()) for p in pathlib.Path(sys.argv[1]).glob('*.json')]; print(next(r['body'] for r in rows if r['to']==sys.argv[2]))"
    for index, client in enumerate(clients):
        invite = remote(args.server_host, compose+["exec", "-T", "app", "cloud-server", "invite"]).decode().strip()
        email = f"cloud-{suffix}-{index}@example.invalid"
        password = secrets.token_urlsafe(20)
        name = ["云端甲", "云端乙", "隔离检查"][index] + "-" + suffix
        client.request("/api/auth/register", "POST", {"invite":invite,"email":email,"password":password,"name":name})
        mail = remote(args.server_host, ["sudo", "-n", "python3", "-c", mail_script, root+"/data/outbox", email]).decode()
        token = re.search(r"token=([a-f0-9]+)", mail).group(1)
        result = client.request("/api/auth/verify", "POST", {"token":token})
        accounts.append({"email":email,"password":password,"account":result["account"]})
    a,b,c = clients
    a.request("/api/me", "PATCH", {"name":"伪造","bio":""}, status=403, headers={"Origin":"https://unrelated.invalid"})
    a.request("/api/chat/contacts", "POST", {"accountId":accounts[1]["account"]["id"],"listed":True})
    conv = a.request("/api/chat/direct", "POST", {"accountId":accounts[1]["account"]["id"]})
    cid = conv["id"]
    assert b.request("/api/chat/direct", "POST", {"accountId":accounts[0]["account"]["id"]})["id"] == cid
    path = f"/api/chat/conversations/{cid}/messages"
    command = {"requestId":str(uuid.uuid4()),"text":"跨设备中文 🐈\n第二行", "images":[{"mime":"image/png","data":base64.b64encode(png()).decode()}], "files":[{"name":"测试 文档.txt","mime":"text/plain","data":base64.b64encode("测试附件 🐈".encode()).decode()}]}
    sent = a.request(path,"POST",command)
    assert a.request(path,"POST",command)["id"] == sent["id"]
    assert b.request(path)["messages"][-1]["id"] == sent["id"]
    c.request(path,status=404)
    c.request(f"/api/chat/conversations/{cid}/export?format=json",status=404)
    assert c.request("/api/chat/search?query="+urllib.parse.quote("跨设备"))["messages"] == []
    attachment = f"/api/chat/messages/{sent['id']}/attachments/1?revision={sent['revision']}"
    assert b.request(attachment) == "测试附件 🐈".encode()
    c.request(attachment,status=404)
    exported = b.request(f"/api/chat/conversations/{cid}/export?format=json&includeImages=true&includeFiles=true")
    with zipfile.ZipFile(io.BytesIO(exported)) as archive:
        assert "chat.json" in archive.namelist()
        doc = json.loads(archive.read("chat.json"))
        assert archive.read(doc["messages"][0]["attachments"][0]["filePath"]) == png()
    # Ubuntu performs its own TLS verification and login; it receives no cookie
    # from the first client. The JSON is sent on stdin, not in a process argument.
    second = r'''import urllib.request,http.cookiejar,ssl,json,sys,uuid
v=json.load(sys.stdin)
ctx=ssl.create_default_context(cadata=v['ca'])
op=urllib.request.build_opener(urllib.request.HTTPSHandler(context=ctx),urllib.request.HTTPCookieProcessor(http.cookiejar.CookieJar()))
def call(path,body=None):
 req=urllib.request.Request(v['url']+path,None if body is None else json.dumps(body).encode(),{'Content-Type':'application/json','X-LLMN-Client':'1'})
 with op.open(req,timeout=30) as r:return json.load(r)
call('/api/auth/login',{'email':v['email'],'password':v['password']})
assert call(v['path'])['messages'][-1]['id']==v['messageId']
result=call(v['path'],{'requestId':str(uuid.uuid4()),'text':'来自第二台客户端的回复 🐧'})
print(json.dumps({'id':result['id']}))
'''
    payload = {**accounts[1],"url":args.url,"ca":Path(args.ca).read_text(),"path":path,"messageId":sent["id"]}
    reply = json.loads(remote(args.client_host,["python3","-c",second],json.dumps(payload).encode()))
    assert a.request(path)["messages"][-1]["id"] == reply["id"]
    a.request("/api/auth/logout","POST",status=204)
    a.request("/api/me",status=401)
    a.request("/api/auth/login","POST",{k:accounts[0][k] for k in ("email","password")})
    assert len(a.request(path)["messages"]) == 2
    # Restart only the application container and verify persisted history/files.
    remote(args.server_host,compose+["restart","app"])
    for attempt in range(20):
        try:
            a.request("/health")
            break
        except (urllib.error.URLError, AssertionError):
            time.sleep(.5)
    assert len(a.request(path)["messages"]) == 2
    assert a.request(attachment) == "测试附件 🐈".encode()
    output = Path(args.output)
    output.parent.mkdir(parents=True,exist_ok=True)
    output.write_text(json.dumps({"url":args.url,"conversationId":cid,"accounts":accounts},ensure_ascii=False,indent=2),encoding="utf-8")
    print("PASS: invited accounts, verified TLS, cross-device chat, attachments/ZIP, idempotency, third-account isolation, logout/relogin, container restart persistence")


if __name__ == "__main__":
    main()
