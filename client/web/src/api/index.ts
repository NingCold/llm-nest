import type { SettingsApi } from "./types"
import { IS_DESKTOP } from "@/lib/desktop"

let api: SettingsApi | null = null
export async function getApi(): Promise<SettingsApi> {
  if (!api) api = IS_DESKTOP ? (await import("./tauri")).tauriApi : (await import("./http")).httpApi
  return api
}
