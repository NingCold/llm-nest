"""Create a private CA and HTTPS certificate for an isolated test deployment."""
import argparse
import ipaddress
from pathlib import Path
import subprocess

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("directory", type=Path)
parser.add_argument("hosts", nargs="+")
args = parser.parse_args()
args.directory.mkdir(parents=True, exist_ok=True)
names = []
for host in args.hosts:
    try:
        names.append("IP:" + str(ipaddress.ip_address(host)))
    except ValueError:
        if not host or any(c not in "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789-." for c in host):
            parser.error("Invalid DNS name")
        names.append("DNS:" + host)


def openssl(*arguments):
    subprocess.run(["openssl", *arguments], cwd=args.directory, check=True,
                   stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)


openssl("req", "-x509", "-newkey", "rsa:2048", "-nodes", "-days", "90",
        "-subj", "/CN=LLM Nest internal test CA", "-keyout", "ca.key", "-out", "ca.crt",
        "-addext", "basicConstraints=critical,CA:TRUE,pathlen:0",
        "-addext", "keyUsage=critical,keyCertSign,cRLSign")
openssl("req", "-newkey", "rsa:2048", "-nodes", "-subj", "/CN=LLM Nest internal test",
        "-keyout", "server.key", "-out", "server.csr")
(args.directory / "server.ext").write_text(
    "subjectAltName=" + ",".join(names) + "\nbasicConstraints=critical,CA:FALSE\n"
    "keyUsage=critical,digitalSignature,keyEncipherment\nextendedKeyUsage=serverAuth\n")
openssl("x509", "-req", "-in", "server.csr", "-CA", "ca.crt", "-CAkey", "ca.key",
        "-CAcreateserial", "-days", "30", "-extfile", "server.ext", "-out", "server.crt")
for path in args.directory.glob("*.key"):
    path.chmod(0o600)
print("Test CA and HTTPS certificate created; keep the private keys on the server.")
