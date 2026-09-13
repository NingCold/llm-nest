import { useEffect, useState } from "react"
import { Copy, Minus, Square, X } from "lucide-react"
import { getCurrentWindow } from "@tauri-apps/api/window"
import { invoke } from "@tauri-apps/api/core"
import { IS_DESKTOP } from "@/lib/desktop"
import { useUiStore } from "@/store/ui"

interface FrameInfo {
  maximized: boolean
  fullscreen: boolean
  verticalDocked: boolean
  focused: boolean
  scaleFactor: number
  nativeBorder: boolean
  revision: number
}

/** Kept outside backend-dependent content so a failed startup is still closable. */
export function WindowControls() {
  const [frame, setFrame] = useState<FrameInfo>()
  const [error, setError] = useState("")
  const [fallbackFrame, setFallbackFrame] = useState(false)
  const theme = useUiStore(s => s.theme)

  useEffect(() => {
    if (!IS_DESKTOP) return
    // Keep the native surface exposed during resize in sync with the opaque CSS page.
    // Use Window (not WebviewWindow) so the WebView2 composition surface stays transparent.
    let active = true
    let unlisten: (() => void) | undefined
    const accept = (next: FrameInfo) => {
      if (active) setFrame(current => !current || next.revision >= current.revision ? next : current)
    }
    void (async () => {
      const win = getCurrentWindow()
      // Subscribe before requesting a snapshot; revisions discard late IPC replies.
      const stop = await win.listen<FrameInfo>("window-frame-changed", event => accept(event.payload))
      if (!active) { stop(); return }
      unlisten = stop
      await win.setBackgroundColor(theme === "dark" ? "#0c0c0e" : "#ffffff")
      if (!active) return
      accept(await invoke<FrameInfo>("set_window_appearance", { dark: theme === "dark" }))
      if (active) setFallbackFrame(false)
    })().catch(e => {
      if (active) { setFallbackFrame(true); setError(`窗口外观更新失败：${String(e)}`) }
    })
    return () => { active = false; unlisten?.() }
  }, [theme])

  if (!IS_DESKTOP) return null
  const maximized = frame?.maximized ?? false
  const fullscreen = frame?.fullscreen ?? false
  const clientFrame = fallbackFrame || (frame && (!frame.nativeBorder || frame.verticalDocked))
  const action = async (name: "minimize" | "toggleMaximize" | "close") => {
    setError("")
    try { await getCurrentWindow()[name]() }
    catch (e) { setError(`窗口操作失败：${String(e)}`) }
  }
  const button = "flex h-14 w-11 items-center justify-center text-muted-foreground transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-ring"
  return <>
    {clientFrame && !maximized && !fullscreen && <div
      aria-hidden="true"
      className="desktop-frame-fallback"
      data-focused={frame?.focused ?? true}
      style={{ borderWidth: 1 / (frame?.scaleFactor || 1) }}
    />}
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
  </>
}
