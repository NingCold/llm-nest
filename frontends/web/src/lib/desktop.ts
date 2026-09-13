import { isTauri } from "@tauri-apps/api/core"

// Available before backend initialization, including on the startup error page.
export const IS_DESKTOP = isTauri()
// Linux uses the system's decorations; Windows keeps the verified custom frame.
export const CUSTOM_WINDOW_CHROME = IS_DESKTOP && __LLMN_CUSTOM_WINDOW_CHROME__
