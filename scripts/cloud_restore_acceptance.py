"""Verify backup/restore on the explicitly selected isolated cloud Compose service.

Pauses only app while snapshotting. Restores PostgreSQL into a new disposable
clone, never over the source. Keeps the dump, attachment ZIP and manifest in
private/backup-restore-<UUID>/ on the server. Does not run model calls.
"""
import argparse
import json
import shlex
import subprocess
import sys


REMOTE = r'''
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import uuid
import zipfile

root = Path(sys.argv[1])
if not root.is_absolute() or root.resolve() != root:
    raise SystemExit("FAIL: server-root must be an absolute resolved path")
compose = ["docker", "compose", "--env-file", str(root / "private/cloud.env"),
           "-f", str(root / "src/server/cloud/deploy/compose.yaml")]

def run(args, *, data=None, output=None, label="remote command"):
    result = subprocess.run(args, input=data, stdout=output or subprocess.PIPE,
                            stderr=subprocess.PIPE)
    if result.returncode:
        if "backup" in globals():
            with (backup / "error.log").open("ab") as log:
                log.write((label + "\n").encode())
                log.write(result.stderr)
        raise RuntimeError(label + " failed (details retained in private error.log)")
    return result.stdout

def pg(args, **kwargs):
    return run(compose + ["exec", "-T", "postgres"] + args, **kwargs)

def sql(database, query):
    return pg(["psql", "-X", "-q", "-A", "-t", "-v", "ON_ERROR_STOP=1",
               "-U", "llmn", "-d", database], data=query.encode(), label="database query")

def active():
    # The current schema calls AI execution records cloud_node_tasks.
    return int(sql("llmn", "SELECT count(*) FROM cloud_node_tasks WHERE state='running';")) + int(
        sql("llmn", "SELECT count(*) FROM cloud_summary_calls WHERE state='running';"))

def table_hashes(database):
    names = sql(database, "SELECT tablename FROM pg_tables WHERE schemaname='public' ORDER BY tablename COLLATE \"C\";").decode().splitlines()
    result = {}
    for name in names:
        quoted = '"' + name.replace('"', '""') + '"'
        query = ('COPY (SELECT to_jsonb(t)::text AS body FROM public.' + quoted +
                 ' t ORDER BY to_jsonb(t)::text COLLATE "C") TO STDOUT;')
        error_log = (backup / "error.log").open("ab")
        process = subprocess.Popen(compose + ["exec", "-T", "postgres", "psql", "-X", "-q",
                                   "-v", "ON_ERROR_STOP=1", "-U", "llmn", "-d", database],
                                   stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                   stderr=error_log)
        digest = hashlib.sha256()
        rows = 0
        try:
            process.stdin.write(query.encode())
            process.stdin.close()
            while chunk := process.stdout.read(1024 * 1024):
                digest.update(chunk)
                rows += chunk.count(b"\n")
            if process.wait():
                raise RuntimeError("table hashing failed (private output suppressed)")
        finally:
            process.stdout.close()
            error_log.close()
            if process.poll() is None:
                process.kill()
                process.wait()
        result[name] = {"rows": rows, "sha256": digest.hexdigest()}
    required = {"accounts", "cloud_messages", "cloud_members", "cloud_list", "cloud_import_reads",
                "cloud_imports", "cloud_receipts", "cloud_events", "_sqlx_migrations"}
    if not required <= result.keys():
        raise RuntimeError("required migration tables missing; upgrade isolated service first")
    return result

def sha_file(path):
    digest = hashlib.sha256()
    with path.open("rb") as source:
        while chunk := source.read(1024 * 1024):
            digest.update(chunk)
    return digest.hexdigest()

os.umask(0o077)
private = root / "private"
if not private.is_dir() or private.is_symlink():
    raise SystemExit("FAIL: expected private directory is missing or symlinked")
app_id = run(compose + ["ps", "-q", "app"], label="locate app").decode().strip()
if not app_id:
    raise SystemExit("FAIL: app must already be running")
state = json.loads(run(["docker", "inspect", "--format", "{{json .State}}", app_id], label="inspect app"))
if not state["Running"] or state["Paused"]:
    raise SystemExit("FAIL: app must already be running and unpaused")
mounts = json.loads(run(["docker", "inspect", "--format", "{{json .Mounts}}", app_id], label="inspect attachment mount"))
attachment_mounts = [m for m in mounts if m["Destination"] == "/data/attachments" and m["Type"] == "bind"]
attachments = root / "data/attachments"
if not attachments.is_dir() or len(attachment_mounts) != 1 or Path(attachment_mounts[0]["Source"]).resolve() != attachments or attachments.resolve() != attachments:
    raise SystemExit("FAIL: expected root/data/attachments bind mount required")
if active():
    raise SystemExit("FAIL: running AI or summary calls; wait for completion before backup")
identifier = uuid.uuid4().hex
backup_name = "backup-restore-" + identifier
backup = private / backup_name
backup.mkdir(mode=0o700)
clone = "llmn_restore_" + identifier
assert re.fullmatch(r"llmn_restore_[0-9a-f]{32}", clone)
paused = False
clone_created = False
try:
    # Mark before invocation so an interrupted pause command still attempts unpause.
    paused = True
    run(compose + ["pause", "app"], label="pause app")
    if active():
        raise RuntimeError("AI call started during pause; no backup taken")
    original = table_hashes("llmn")
    with (backup / "database.dump").open("xb") as dump:
        pg(["pg_dump", "-U", "llmn", "-d", "llmn", "--format=custom", "--no-owner", "--no-acl"],
           output=dump, label="database dump")
    files = []
    with zipfile.ZipFile(backup / "attachments.zip", "x", compression=zipfile.ZIP_DEFLATED) as archive:
        for directory, directories, names in os.walk(attachments, followlinks=False):
            for name in directories:
                if (Path(directory) / name).is_symlink():
                    raise RuntimeError("symlink in attachment directory")
            for name in sorted(names):
                path = Path(directory) / name
                if path.is_symlink() or not path.is_file():
                    raise RuntimeError("unsupported attachment entry")
                relative = path.relative_to(attachments).as_posix()
                files.append({"path": relative, "bytes": path.stat().st_size, "sha256": sha_file(path)})
                archive.write(path, relative)
    files.sort(key=lambda item: item["path"])
    manifest = {"tables": original, "attachments": files,
                "databaseDumpSha256": sha_file(backup / "database.dump")}
    (backup / "manifest.json").write_text(json.dumps(manifest, ensure_ascii=False, indent=2), encoding="utf-8")
finally:
    if paused:
        run(compose + ["unpause", "app"], label="resume app")

try:
    pg(["createdb", "-U", "llmn", "--template=template0", clone], label="create isolated restore database")
    clone_created = True
    with (backup / "database.dump").open("rb") as dump:
        # Binary dump travels only on stdin to the database container.
        result = subprocess.run(compose + ["exec", "-T", "postgres", "pg_restore", "-U", "llmn",
                                "--dbname=" + clone, "--exit-on-error", "--no-owner", "--no-acl"],
                                stdin=dump, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
        if result.returncode:
            with (backup / "error.log").open("ab") as log:
                log.write(b"database restore\n" + result.stderr)
            raise RuntimeError("database restore failed (details retained in private error.log)")
    if table_hashes(clone) != original:
        raise RuntimeError("restored database rows differ from snapshot")
    restored = backup / "restored-attachments"
    restored.mkdir(mode=0o700)
    with zipfile.ZipFile(backup / "attachments.zip") as archive:
        expected = {item["path"] for item in files}
        if set(archive.namelist()) != expected or len(archive.namelist()) != len(expected):
            raise RuntimeError("attachment archive manifest mismatch")
        for item in files:
            target = restored / item["path"]
            if not target.resolve().is_relative_to(restored):
                raise RuntimeError("unsafe attachment path")
            target.parent.mkdir(parents=True, exist_ok=True)
            with archive.open(item["path"]) as source, target.open("xb") as output:
                shutil.copyfileobj(source, output)
            if target.stat().st_size != item["bytes"] or sha_file(target) != item["sha256"]:
                raise RuntimeError("restored attachment differs from snapshot")
    print(json.dumps({"status": "PASS", "backup": "private/" + backup_name,
                      "tables": len(original), "rows": sum(t["rows"] for t in original.values()),
                      "attachments": len(files), "emptyFiles": sum(f["bytes"] == 0 for f in files)}))
finally:
    if clone_created:
        # This exact UUID database is the only database this script ever drops.
        pg(["dropdb", "-U", "llmn", "--force", clone], label="drop isolated restore database")
'''


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--server-host", required=True, help="SSH destination for the authorized isolated server")
    parser.add_argument("--server-root", required=True, help="Absolute resolved directory containing src/, private/, data/")
    args = parser.parse_args()
    command = " ".join(shlex.quote(a) for a in ["sudo", "-n", "python3", "-", args.server_root])
    wrapped = "try:\n" + "".join("    " + line + "\n" for line in REMOTE.splitlines()) + """
except (RuntimeError, SystemExit) as error:
    if "backup" in globals():
        import traceback
        with (backup / "error.log").open("a", encoding="utf-8") as log:
            traceback.print_exc(file=log)
    print(json.dumps({"status": "FAIL", "reason": str(error), "backup": "private/" + backup_name if "backup_name" in globals() else None}))
    sys.exit(1)
except Exception:
    if "backup" in globals():
        import traceback
        with (backup / "error.log").open("a", encoding="utf-8") as log:
            traceback.print_exc(file=log)
    print(json.dumps({"status": "FAIL", "reason": "backup/restore failed; details retained in private error.log", "backup": "private/" + backup_name if "backup_name" in globals() else None}))
    sys.exit(1)
"""
    result = subprocess.run(["ssh", "-o", "BatchMode=yes", args.server_host, command],
                            input=wrapped.encode(), stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    if result.returncode:
        # Only the remote wrapper's deliberately sanitized JSON is relayed.
        try:
            summary = json.loads(result.stdout)
        except (ValueError, UnicodeDecodeError):
            summary = {"status": "FAIL", "reason": "SSH or remote Python failed; private output suppressed"}
        print(json.dumps(summary))
        return 1
    summary = json.loads(result.stdout)
    print(json.dumps(summary))
    return 0


if __name__ == "__main__":
    sys.exit(main())
