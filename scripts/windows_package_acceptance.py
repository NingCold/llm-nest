"""Verify installed/extracted Windows payloads against the Tauri release EXE.

Tauri patches a three-byte bundle marker (UNK/NSS/MSI) per installer, then
restores the original EXE. Compare every other byte and verify each bundled
Agent against its fixed version/SHA256 record. Does not install or read user data.
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


def verify_agent(directory, expected=None):
    record_path = directory / "agent-artifact.json"
    record = json.loads(record_path.read_text(encoding="utf-8"))
    if not record.get("agentVersion") or "windows" not in record.get("target", ""):
        raise ValueError(f"Invalid Windows Agent record: {record_path}")
    if expected is not None and record != expected:
        raise ValueError(f"Bundled Agent record differs from build: {record_path}")
    binary = directory / "llmn-agent.exe"
    digest = hashlib.sha256(binary.read_bytes()).hexdigest()
    if digest != record.get("sha256"):
        raise ValueError(f"Bundled Agent SHA256 mismatch: {binary}")
    result = subprocess.run([str(binary.resolve()), "--version"], capture_output=True,
        text=True, env={}, timeout=10, check=True,
        creationflags=subprocess.CREATE_NO_WINDOW if os.name == "nt" else 0)
    if result.stdout.strip() != f"llmn-agent {record['agentVersion']}":
        raise ValueError(f"Bundled Agent version mismatch: {binary}")
    return record


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--build", type=Path, required=True)
    parser.add_argument("--installed-nsis", type=Path, required=True)
    parser.add_argument("--msi-extracted", type=Path, required=True)
    parser.add_argument("--agent-directory", type=Path,
        default=Path(__file__).resolve().parents[1] / "desktop/src-tauri/resources",
        help="Prepared build Agent and agent-artifact.json directory")
    args = parser.parse_args()
    agent_record = verify_agent(args.agent_directory.resolve(strict=True))
    reference, _ = normalized(args.build.resolve(strict=True), b"UNK")
    for label, path, marker in (("NSIS installed", args.installed_nsis, b"NSS"),
                                ("MSI extracted", args.msi_extracted, b"MSI")):
        path = path.resolve(strict=True)
        content, digest = normalized(path, marker)
        assert content == reference, f"{label}: EXE differs beyond its expected bundle marker"
        verify_agent(path.parent / "agent", agent_record)
        print(f"PASS: {label} payload and bundled Agent version/SHA256; EXE SHA256={digest}")


if __name__ == "__main__":
    main()
