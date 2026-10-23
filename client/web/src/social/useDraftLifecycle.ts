import { useEffect } from "react"
import { invoke } from "@tauri-apps/api/core"
import { listen } from "@tauri-apps/api/event"
import { IS_DESKTOP } from "@/lib/desktop"
import { flushDrafts, hasUnsavedDrafts, useLiveSocialStore } from "./live-store"

export function useDraftLifecycle() {
  useEffect(() => {
    const unloading = (event: BeforeUnloadEvent) => { if (hasUnsavedDrafts()) { event.preventDefault(); event.returnValue = "" } }
    window.addEventListener("beforeunload", unloading)
    let disposed = false
    const page = crypto.randomUUID()
    const setup = async () => {
      if (!IS_DESKTOP) return
      const unlisten = await listen<string>("drafts-prepare-close", async event => {
        useLiveSocialStore.setState({ draftsClosing: true })
        let error: string | null = null
        try { await flushDrafts() } catch (e) { error = String(e) }
        try { await invoke("desktop_finish_draft_close", { page, request: event.payload, error }) }
        catch (e) { useLiveSocialStore.setState({ draftsClosing: false, error: `关闭确认失败：${String(e)}` }) }
        if (error) useLiveSocialStore.setState({ draftsClosing: false })
      })
      if (disposed) { unlisten(); return }
      const failed = await listen("backend-shutdown-failed", () => useLiveSocialStore.setState({ draftsClosing: false }))
      const retained = await listen("desktop-close-retained", () => useLiveSocialStore.setState({ draftsClosing: false }))
      try { await invoke("desktop_register_draft_guard", { page }) }
      catch (e) { unlisten(); failed(); retained(); throw e }
      return () => { unlisten(); failed(); retained() }
    }
    const report = (e: unknown) => useLiveSocialStore.setState({ error: `关闭前的草稿保存检查未能完成：${String(e)}` })
    const listening = setup().catch(report)
    return () => {
      disposed = true; window.removeEventListener("beforeunload", unloading)
      void listening.then(async unlisten => { unlisten?.(); if (IS_DESKTOP) await invoke("desktop_unregister_draft_guard", { page }) }).catch(report)
    }
  }, [])
}
