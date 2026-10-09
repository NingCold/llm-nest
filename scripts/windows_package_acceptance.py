"""Verify installed/extracted Windows payloads against the Tauri release EXE.

Tauri patches a three-byte bundle marker (UNK/NSS/MSI) per installer, then
restores the original EXE. Compare every other byte and execute each payload's
worker in an empty environment. Does not install, publish or read user data.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess


def normalized(path, expected):
    data = path.read_bytes()
    prefix = b"__TAURI_BUNDLE_TYPE_VAR_"
    if data.count(prefix) != 1:
        raise ValueError(f"{path.name}: expected exactly one Tauri bundle marker")
    offset = data.index(prefix) + len(prefix)
    marker = data[offset:offset + 3]
    if marker != expected:
        raise ValueError(f"{path.name}: unexpected bundle marker {marker!r}")
    return data[:offset] + b"UNK" + data[offset + 3:], hashlib.sha256(data).hexdigest()


def worker(path, tool, body):
    result = subprocess.run([str(path), "--llmn-tool-worker", tool], input=json.dumps(body).encode(),
        capture_output=True, env={}, timeout=10, check=True,
        creationflags=subprocess.CREATE_NO_WINDOW if os.name == "nt" else 0)
    return json.loads(result.stdout)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--build", type=Path, required=True)
    parser.add_argument("--installed-nsis", type=Path, required=True)
    parser.add_argument("--msi-extracted", type=Path, required=True)
    args = parser.parse_args()
    reference, _ = normalized(args.build.resolve(strict=True), b"UNK")
    for label, path, marker in (("NSIS installed", args.installed_nsis, b"NSS"),
                                ("MSI extracted", args.msi_extracted, b"MSI")):
        path = path.resolve(strict=True)
        content, digest = normalized(path, marker)
        assert content == reference, f"{label}: EXE differs beyond its expected bundle marker"
        assert worker(path, "add", {"a": 2, "b": 3}) == {"result": {"sum": 5}}
        assert "tool not allowed" in worker(path, "shell", {})["error"]
        print(f"PASS: {label} payload, empty-environment worker and fixed allowlist; SHA256={digest}")


if __name__ == "__main__":
    main()
