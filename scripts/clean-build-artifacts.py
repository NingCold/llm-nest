#!/usr/bin/env python3
"""Preview or remove only Cargo incremental caches in this repository."""

import argparse
import csv
from dataclasses import dataclass
import io
import os
from pathlib import Path
import re
import stat
import subprocess
import sys


# Conservatively recognize desktop triples; unknown/custom target directories stay intact.
DESKTOP_TARGET = re.compile(
    r"(?:x86_64|i686|aarch64)-(?:pc-windows-(?:msvc|gnu|gnullvm)"
    r"|unknown-linux-(?:gnu|musl)|apple-darwin)"
)


class CleanupError(RuntimeError):
    pass


@dataclass(frozen=True)
class Candidate:
    path: Path
    size: int
    files: tuple[Path, ...]
    directories: tuple[Path, ...]


def reject_link(path: Path) -> None:
    info = path.lstat()
    if stat.S_ISLNK(info.st_mode) or (
        getattr(info, "st_file_attributes", 0)
        & getattr(stat, "FILE_ATTRIBUTE_REPARSE_POINT", 0)
    ):
        raise CleanupError(f"Refusing link or reparse point: {path}")


def active_build_processes() -> list[str]:
    if os.name == "nt":
        output = subprocess.check_output(
            ["tasklist", "/FO", "CSV", "/NH"], text=True
        )
        names = [row[0] for row in csv.reader(io.StringIO(output)) if row]
    else:
        output = subprocess.check_output(["ps", "-A", "-o", "comm="], text=True)
        names = output.splitlines()
    return sorted(
        {name.strip() for name in names
         if Path(name.strip()).name.lower() in {"cargo", "cargo.exe", "rustc", "rustc.exe"}}
    )


def ensure_idle() -> None:
    processes = active_build_processes()
    if processes:
        raise CleanupError("Build processes are active: " + ", ".join(processes))


def inspect_candidate(path: Path) -> Candidate:
    """Read and validate a fixed absolute inventory before any deletion."""
    reject_link(path)
    size = 0
    file_paths = []
    directory_paths = [path]
    for root, dirs, files in os.walk(path, followlinks=False, onerror=raise_walk_error):
        for name in dirs:
            entry = Path(root) / name
            reject_link(entry)
            directory_paths.append(entry)
        for name in files:
            entry = Path(root) / name
            reject_link(entry)
            size += entry.stat().st_size
            file_paths.append(entry)
    return Candidate(
        path, size, tuple(sorted(file_paths)),
        tuple(sorted(directory_paths, key=lambda entry: (-len(entry.parts), str(entry)))),
    )


def raise_walk_error(error: OSError) -> None:
    raise error


def plan_cleanup(repository: Path, target: Path | None = None) -> list[Candidate]:
    repository = repository.resolve(strict=True)
    expected = repository / "target"
    target = expected if target is None else target.absolute()
    if target != expected:
        raise CleanupError(f"Only the repository target directory is allowed: {expected}")
    if not target.exists() and not target.is_symlink():
        return []
    reject_link(target)
    profiles = [target / "debug", target / "release"]
    for child in target.iterdir():
        # Do not descend into tool, work, backup, or arbitrary data directories.
        if DESKTOP_TARGET.fullmatch(child.name):
            reject_link(child)
            if child.is_dir():
                profiles.extend([child / "debug", child / "release"])
    candidates = []
    for profile in profiles:
        if not profile.exists() and not profile.is_symlink():
            continue
        reject_link(profile)
        incremental = profile / "incremental"
        if not incremental.exists() and not incremental.is_symlink():
            continue
        reject_link(incremental)
        if not incremental.is_dir():
            raise CleanupError(f"Expected an incremental directory: {incremental}")
        candidates.append(inspect_candidate(incremental))
    return sorted(candidates, key=lambda candidate: str(candidate.path))


def cleanup(repository: Path, apply: bool = False) -> list[Candidate]:
    ensure_idle()
    candidates = plan_cleanup(repository)
    for candidate in candidates:
        print(f"{'REMOVE' if apply else 'PREVIEW'} {candidate.path} ({candidate.size:,} bytes)")
        for path in candidate.files:
            print(f"  FILE {path}")
        for path in candidate.directories:
            print(f"  DIRECTORY {path}")
    print(f"Total: {sum(candidate.size for candidate in candidates):,} bytes")
    if apply:
        # Recheck the whole plan before the first mutation, then each target.
        ensure_idle()
        if plan_cleanup(repository) != candidates:
            raise CleanupError("Incremental caches changed after preview; run again when idle")
        for candidate in candidates:
            ensure_idle()
            if inspect_candidate(candidate.path) != candidate:
                raise CleanupError("Incremental cache changed after preview; run again when idle")
            for path in candidate.files:
                path.unlink()
            for path in candidate.directories:
                if any(path.iterdir()):
                    raise CleanupError(f"Directory is not empty: {path}")
                path.rmdir()
            if candidate.path.exists() or candidate.path.is_symlink():
                raise CleanupError(f"Removal verification failed: {candidate.path}")
        print(f"Removed and verified {len(candidates)} incremental cache directories.")
    else:
        print("Preview only. Pass --apply to remove these directories after all builds finish.")
    return candidates


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--apply", action="store_true", help="remove the previewed incremental caches")
    args = parser.parse_args()
    try:
        cleanup(Path(__file__).resolve().parent.parent, apply=args.apply)
    except (CleanupError, OSError, subprocess.SubprocessError) as error:
        print(f"Cleanup stopped: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
