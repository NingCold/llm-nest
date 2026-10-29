"""Check client, cloud and Agent source boundaries using Cargo's dependency graph."""
import json
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[1]


def main():
    metadata = json.loads(subprocess.check_output(
        ["cargo", "metadata", "--no-deps", "--format-version", "1", "--locked"],
        cwd=ROOT, text=True, encoding="utf-8",
    ))
    packages = {p["name"]: p for p in metadata["packages"]}
    retired = {"runtime", "storage", "tools", "social-feature", "web-server"}
    if packages.keys() & retired:
        raise SystemExit("Retired host packages remain in the workspace")
    forbidden = {
        "connector": {"runtime", "storage", "ai-client", "builtin-agent", "tools",
                      "social-feature", "web-server", "cloud-server", "tauri-frontend", "llmn-crypto"},
        "builtin-agent": {"runtime", "storage", "connector", "social-feature",
                          "web-server", "cloud-server", "tauri-frontend"},
        "cloud-server": {"runtime", "storage", "ai-client", "builtin-agent", "tools",
                         "social-feature", "web-server", "tauri-frontend", "llmn-crypto"},
        "llmn-crypto": {"runtime", "storage", "connector", "ai-client", "builtin-agent",
                        "tools", "social-feature", "web-server", "cloud-server", "tauri-frontend"},
        "agent-node": {"runtime", "storage", "social-feature", "web-server",
                       "cloud-server", "tauri-frontend"},
        "tauri-frontend": {"runtime", "storage", "ai-client", "builtin-agent", "tools",
                           "social-feature", "web-server", "cloud-server", "llmn-crypto", "common"},
    }
    for start, excluded in forbidden.items():
        pending = [(start, [start])]
        visited = set()
        while pending:
            name, chain = pending.pop()
            if name in excluded:
                raise SystemExit("Forbidden dependency: " + " -> ".join(chain))
            if name in visited:
                continue
            visited.add(name)
            for dependency in packages[name]["dependencies"]:
                target = dependency["name"]
                if dependency.get("path") and target in packages:
                    pending.append((target, chain + [target]))
        print(f"{start}: dependency boundary OK")

    prepare = (ROOT / "client/desktop/scripts/prepare-agent.mjs").read_text(encoding="utf-8")
    if any(token in prepare for token in ("execFile", "spawn(", "cargo build", "target/debug", "target/release")):
        raise SystemExit("Desktop artifact preparation must not build or locate Agent source output")
    docker = (ROOT / "server/cloud/Dockerfile").read_text(encoding="utf-8")
    for line in docker.splitlines():
        if line.startswith("COPY ") and any(token in line for token in ("client/", "agent/", "crates/runtime", "crates/storage")):
            raise SystemExit("Cloud Docker build includes another runtime or client source")
    print("Desktop Agent artifact and cloud build-context boundaries OK")


if __name__ == "__main__":
    main()
