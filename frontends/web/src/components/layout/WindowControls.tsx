import { useEffect, useState } from "react"
import { Copy, Minus, Square, X } from "lucide-react"
import { getCurrentWindow } from "@tauri-apps/api/window"
import { IS_DESKTOP } from "@/lib/desktop"
import { useUiStore } from "@/store/ui"

/** Kept outside backend-dependent content so a failed startup is still closable. */
export function WindowControls() {
  const [maximized, setMaximized] = useState(false)
  const [error, setError] = useState("")
  const theme = useUiStore(s => s.theme)

  useEffect(() => {
    if (!IS_DESKTOP) return
    // Keep the native surface exposed during resize in sync with the opaque CSS page.
    // Use Window (not WebviewWindow) so the WebView2 composition surface stays transparent.
    let active = true
    void getCurrentWindow().setBackgroundColor(theme === "dark" ? "#0c0c0e" : "#ffffff")
      .catch(e => { if (active) setError(`窗口背景更新失败：${String(e)}`) })
    return () => { active = false }
  }, [theme])

  useEffect(() => {
    if (!IS_DESKTOP) return
    let active = true
    let unlisten: (() => void) | undefined
    const win = getCurrentWindow()
    const refresh = () => {
      void win.isMaximized().then(value => { if (active) setMaximized(value) })
        .catch(e => { if (active) setError(String(e)) })
    }
    refresh()
    void win.onResized(refresh).then(stop => {
      if (active) unlisten = stop
      else stop()
    }).catch(e => { if (active) setError(String(e)) })
    return () => { active = false; unlisten?.() }
  }, [])

  if (!IS_DESKTOP) return null
  const action = async (name: "minimize" | "toggleMaximize" | "close") => {
    setError("")
    try { await getCurrentWindow()[name]() }
    catch (e) { setError(`窗口操作失败：${String(e)}`) }
  }
  const button = "flex h-14 w-11 items-center justify-center text-muted-foreground transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-ring"
  return (
    <div className="fixed right-0 top-0 z-50 flex select-none" aria-label="窗口控制">
      <button type="button" aria-label="最小化窗口" title="最小化" className={`${button} hover:bg-accent hover:text-foreground`} onClick={() => void action("minimize")}><Minus className="h-4 w-4" /></button>
      <button type="button" aria-label={maximized ? "还原窗口" : "最大化窗口"} title={maximized ? "还原" : "最大化"} className={`${button} hover:bg-accent hover:text-foreground`} onClick={() => void action("toggleMaximize")}>
        {maximized ? <Copy className="h-3.5 w-3.5" /> : <Square className="h-3.5 w-3.5" />}
      </button>
      <button type="button" aria-label="关闭窗口" title="关闭" className={`${button} hover:bg-red-600 hover:text-white`} onClick={() => void action("close")}><X className="h-4.5 w-4.5" /></button>
      {error && <div role="alert" className="absolute right-2 top-14 w-72 rounded-lg border border-border bg-background p-3 text-sm shadow-lg">
        <p className="break-words text-red-500">{error}</p><p className="mt-1 text-muted-foreground">也可按 Alt+F4 关闭窗口。</p><button onClick={() => setError("")} className="mt-2 underline">知道了</button>
      </div>}
    </div>
  )
}
