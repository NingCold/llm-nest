import { IS_DESKTOP, CUSTOM_WINDOW_CHROME } from "@/lib/desktop"

/** The native backdrop stays empty until startup succeeds or reports an error. */
export function StartupSurface() {
  return <div className={IS_DESKTOP ? "desktop-startup-surface" : "web-startup-surface"}
    data-tauri-drag-region={CUSTOM_WINDOW_CHROME || undefined} role="status" aria-label="正在打开 LLM Nest">
    <span className={IS_DESKTOP ? "sr-only" : undefined}>正在打开 LLM Nest…</span>
  </div>
}
