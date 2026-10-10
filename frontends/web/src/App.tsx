import { lazy, Suspense, useEffect, useState } from "react"
import { listen } from "@tauri-apps/api/event"
import ErrorBoundary from "@/components/ErrorBoundary"
import { useUiStore } from "@/store/ui"
import { WindowControls } from "@/components/layout/WindowControls"
import { IS_SOCIAL_PREVIEW } from "@/lib/social-preview"
import { IS_DESKTOP } from "@/lib/desktop"
import { StartupSurface } from "@/components/StartupSurface"

const LegacyApp = lazy(() => import("./LegacyApp"))
const PreviewApp = lazy(() => import("./social/PreviewApp").then(module => ({ default: module.PreviewApp })))
const SocialApp = lazy(() => import("./social/App"))
const legacy = new URLSearchParams(location.search).get("legacy") === "1" || new URLSearchParams(location.search).get("demo") === "1"

function ShutdownError() {
  const [error, setError] = useState<string | null>(null)
  useEffect(() => {
    if (!IS_DESKTOP) return
    const listening = listen<string>("backend-shutdown-failed", event => setError(event.payload))
    return () => { void listening.then(unlisten => unlisten()) }
  }, [])
  return error ? <div role="alert" className="fixed bottom-4 left-4 right-4 z-[100] rounded-lg border border-destructive bg-background p-4 text-sm text-destructive">{error}</div> : null
}

function ThemeSync() {
  const theme = useUiStore(s => s.theme)
  const ready = useUiStore(s => s.desktopReady)
  useEffect(() => {
    const root = document.documentElement
    root.classList.toggle("dark", theme === "dark")
    if (IS_DESKTOP) {
      root.classList.toggle("desktop-starting", !ready)
      root.classList.toggle("desktop-ready", ready)
    }
  }, [theme, ready])
  return null
}

function NavigationSync() {
  useEffect(() => {
    const sync = () => useUiStore.getState().syncRoute()
    const shortcut = (event: KeyboardEvent) => {
      if ((event.ctrlKey || event.metaKey) && event.key === ",") {
        event.preventDefault(); useUiStore.getState().showSettings()
      }
    }
    window.addEventListener("hashchange", sync); window.addEventListener("keydown", shortcut)
    return () => { window.removeEventListener("hashchange", sync); window.removeEventListener("keydown", shortcut) }
  }, [])
  return null
}

export default function App() {
  return <><ThemeSync /><NavigationSync /><WindowControls /><ShutdownError /><ErrorBoundary>
    <Suspense fallback={<StartupSurface />}>
      {IS_SOCIAL_PREVIEW ? <PreviewApp /> : legacy ? <LegacyApp /> : <SocialApp />}
    </Suspense>
  </ErrorBoundary></>
}
