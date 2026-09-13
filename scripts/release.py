"""Validate release versions and collect only expected installer artifacts.

No network calls, credentials or publishing. Use with Python 3.11+.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import tomllib

ROOT = Path(__file__).resolve().parents[1]


def validate(root=ROOT, tag=""):
    version = tomllib.loads((root / "Cargo.toml").read_text(encoding="utf-8"))["workspace"]["package"]["version"]
    for name in ("frontends/tauri/src-tauri/tauri.conf.json",
                 "frontends/tauri/package.json", "frontends/web/package.json"):
        actual = json.loads((root / name).read_text(encoding="utf-8"))["version"]
        if actual != version:
            raise ValueError(f"{name}: version {actual!r} does not match Cargo {version!r}")
    if tag and (not re.fullmatch(r"v\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?", tag) or tag != f"v{version}"):
        raise ValueError(f"Tag {tag!r} must match v{version}")
    return version


def checksums(directory):
    files = sorted(p for p in directory.iterdir()
                   if p.is_file() and not p.name.startswith("SHA256SUMS"))
    if not files:
        raise ValueError("No release files found")
    return "".join(f"{hashlib.sha256(p.read_bytes()).hexdigest()}  {p.name}\n" for p in files)


def collect(bundle_dir, output, platform, version, appimage=False):
    expected = [("nsis", ".exe"), ("msi", ".msi")] if platform == "windows-x86_64" else [("deb", ".deb")]
    if appimage:
        if platform != "linux-x86_64":
            raise ValueError("AppImage is only supported for linux-x86_64")
        expected.append(("appimage", ".AppImage"))
    selected = []
    for folder, suffix in expected:
        found = [p for p in (bundle_dir / folder).glob(f"*{suffix}")
                 if f"_{version}_" in p.name and p.is_file()]
        if len(found) != 1:
            raise ValueError(f"Expected one {folder} artifact for {version}, found {len(found)}")
        selected.append(found[0])
    output.mkdir(parents=True, exist_ok=True)
    if any(output.iterdir()):
        raise ValueError(f"Output must be empty: {output}")
    for source in selected:
        shutil.copy2(source, output / source.name)
    (output / f"SHA256SUMS-{platform}.txt").write_text(checksums(output), encoding="utf-8")
    return selected


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)
    v = sub.add_parser("validate")
    v.add_argument("--tag", default=os.environ.get("RELEASE_TAG", ""))
    c = sub.add_parser("collect")
    c.add_argument("--bundle-dir", type=Path, required=True)
    c.add_argument("--output", type=Path, required=True)
    c.add_argument("--platform", choices=("windows-x86_64", "linux-x86_64"), required=True)
    c.add_argument("--appimage", action="store_true")
    s = sub.add_parser("checksums")
    s.add_argument("directory", type=Path)
    args = parser.parse_args()
    if args.command == "validate":
        print(f"Release version: {validate(tag=args.tag)}")
    elif args.command == "collect":
        for p in collect(args.bundle_dir, args.output, args.platform, validate(), args.appimage):
            print(p.name)
    else:
        (args.directory / "SHA256SUMS.txt").write_text(checksums(args.directory), encoding="utf-8")


if __name__ == "__main__":
    main()
