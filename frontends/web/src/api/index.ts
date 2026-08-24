import type { ChatApi } from "./types"
import { useUiStore } from "@/store/ui"

let api: ChatApi | null = null

/**
 * 适配器选择：
 * 1. Tauri 壳（isTauri() 为真）→ tauriApi（真实 invoke）
 * 2. 浏览器且真实 HTTP 后端可用（web-server）→ httpApi（真实 ChatFeature）
 * 3. 兜底 → webApi（localStorage 演示适配器，纯前端演示）
 */
export async function getApi(): Promise<ChatApi> {
  if (api) return api

  // 1) Tauri 壳
  try {
    const { isTauri } = await import("@tauri-apps/api/core")
    if (isTauri()) {
      const { tauriApi } = await import("./tauri")
      api = tauriApi
      useUiStore.getState().setApiMode("tauri")
      return api
    }
  } catch {
    /* 非 Tauri 环境（web 包未装 @tauri-apps/api 时） */
  }

  // 2) 浏览器：优先真实 HTTP 后端
  const { httpApi, backendAvailable } = await import("./http")
  if (await backendAvailable()) {
    api = httpApi
    useUiStore.getState().setApiMode("http")
    return api
  }

  // 3) 兜底：演示适配器
  const { webApi } = await import("./web")
  api = webApi
  useUiStore.getState().setApiMode("demo")
  return api
}
