import type { ChatApi } from "./types"
import { useUiStore } from "@/store/ui"
import { IS_SOCIAL_PREVIEW } from "@/lib/social-preview"

let api: ChatApi | null = null

/**
 * 适配器选择：
 * 1. Tauri 壳（isTauri() 为真）→ tauriApi（真实 invoke）
 * 2. 显式社交预览或 ?demo=1 → localStorage 演示（预览仅复用设置）
 * 3. 普通浏览器 → HTTP；连接失败不会生成演示回答
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

  if (IS_SOCIAL_PREVIEW || new URLSearchParams(location.search).get("demo") === "1") {
    const { webApi } = await import("./web")
    api = webApi
    useUiStore.getState().setApiMode("demo")
  } else {
    const { httpApi } = await import("./http")
    api = httpApi
    useUiStore.getState().setApiMode("http")
  }
  return api
}
