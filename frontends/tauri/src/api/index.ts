import type { ChatApi } from "./types"

let api: ChatApi

export async function getApi(): Promise<ChatApi> {
  if (api) return api

  try {
    const { isTauri } = await import("@tauri-apps/api/core")
    if (isTauri()) {
      const { tauriApi } = await import("./tauri")
      api = tauriApi
    } else {
      const { webApi } = await import("./web")
      api = webApi
    }
  } catch {
    const { webApi } = await import("./web")
    api = webApi
  }

  return api
}