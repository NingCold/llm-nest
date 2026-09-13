import { isTauri } from "@tauri-apps/api/core"

// Available before backend initialization, including on the startup error page.
export const IS_DESKTOP = isTauri()
