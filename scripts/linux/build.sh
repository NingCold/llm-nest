#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/../.."

if [[ $(uname -s) != Linux ]]; then
  echo 'Run this script on Linux or inside scripts/linux/Dockerfile.' >&2
  exit 1
fi

pnpm --dir client/web install --frozen-lockfile
pnpm --dir client/desktop install --frozen-lockfile
pnpm --dir client/web typecheck
pnpm --dir client/web test
# This monorepo step explicitly produces the versioned Agent artifact first.
# After splitting, pass the same manifest/file inputs from the Agent build.
agent_target="${LLMN_AGENT_TARGET:-$(rustc -vV | sed -n 's/^host: //p')}"
agent_version="$(python3 -c 'import tomllib; print(tomllib.load(open("Cargo.toml", "rb"))["workspace"]["package"]["version"])')"
agent_output="$(mktemp -d "${TMPDIR:-/tmp}/llmn-agent-package.XXXXXX")"
cargo build -p agent-node --release --target "$agent_target" --locked
agent_binary="$(python3 agent/scripts/package-agent.py \
  --binary "${CARGO_TARGET_DIR:-target}/$agent_target/release/llmn-agent" \
  --output "$agent_output" --version "$agent_version" --target "$agent_target")"
export LLMN_AGENT_FILE="$agent_binary"
export LLMN_AGENT_MANIFEST="$agent_output/agent-manifest.json"
export LLMN_AGENT_VERSION="$agent_version"
export LLMN_AGENT_TARGET="$agent_target"
printf 'Agent artifact retained for verification/cleanup: %s\n' "$agent_output"
pnpm --dir client/desktop build:agent
cargo test --workspace --locked

# Use Tauri CLI: it builds the shared UI and embeds it with custom-protocol.
cd client/desktop
pnpm exec tauri build --target "$agent_target" --bundles "${LLMN_LINUX_BUNDLES:-deb,appimage}" --ci -- --locked
