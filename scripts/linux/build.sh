#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/../.."

if [[ $(uname -s) != Linux ]]; then
  echo 'Run this script on Linux or inside scripts/linux/Dockerfile.' >&2
  exit 1
fi

pnpm --dir frontends/web install --frozen-lockfile
pnpm --dir frontends/tauri install --frozen-lockfile
pnpm --dir frontends/web typecheck
pnpm --dir frontends/web test
cargo test --workspace --locked
cargo build -p web-server --locked
python3 scripts/acceptance.py

# Use Tauri CLI: it builds the shared UI and embeds it with custom-protocol.
cd frontends/tauri
pnpm exec tauri build --bundles "${LLMN_LINUX_BUNDLES:-deb,appimage}" --ci -- --locked
