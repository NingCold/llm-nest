"""Check the extracted Agent core and connector using Cargo's actual dependency graph."""
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
    forbidden = {
        "connector": {"runtime", "storage", "ai-client", "builtin-agent", "tools",
                      "social-feature", "web-server", "tauri-frontend"},
        "builtin-agent": {"runtime", "storage", "connector", "social-feature",
                          "web-server", "tauri-frontend"},
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


if __name__ == "__main__":
    main()
