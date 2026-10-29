#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/../.."

if [[ $(uname -s) != Linux ]]; then
  echo 'Run this script on Linux or inside scripts/linux/Dockerfile.' >&2
  exit 1
fi

pnpm --dir web install --frozen-lockfile
pnpm --dir desktop install --frozen-lockfile
pnpm --dir web typecheck
pnpm --dir web test
: "${LLMN_AGENT_MANIFEST:?Set exact Agent manifest}"
: "${LLMN_AGENT_VERSION:?Set exact Agent version}"
: "${LLMN_AGENT_TARGET:?Set target triple}"
agent_target="$LLMN_AGENT_TARGET"
pnpm --dir desktop build:agent
cargo test --workspace --locked

# Use Tauri CLI: it builds the shared UI and embeds it with custom-protocol.
cd desktop
pnpm exec tauri build --target "$agent_target" --bundles "${LLMN_LINUX_BUNDLES:-deb,appimage}" --ci -- --locked
