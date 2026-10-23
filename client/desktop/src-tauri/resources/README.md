# Packaged Agent

`pnpm --dir client/desktop build:agent` builds the independent `llmn-agent` executable here. Tauri bundles it under `agent/`; generated executables are ignored. Development builds resolve the adjacent Cargo target executable as well.
