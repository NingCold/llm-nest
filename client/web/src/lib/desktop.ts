import { isTauri } from "@tauri-apps/api/core"

// Available before backend initialization, including on the startup error page.
export const IS_DESKTOP = isTauri()
// Desktop controls live in the content header; the OS window has no title bar.
export const CUSTOM_WINDOW_CHROME = IS_DESKTOP && __LLMN_CUSTOM_WINDOW_CHROME__
