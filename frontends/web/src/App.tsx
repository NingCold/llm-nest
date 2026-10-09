import { lazy, Suspense, useEffect } from "react"
import ErrorBoundary from "@/components/ErrorBoundary"
import { useUiStore } from "@/store/ui"
import { WindowControls } from "@/components/layout/WindowControls"
import { IS_SOCIAL_PREVIEW } from "@/lib/social-preview"
import { IS_DESKTOP } from "@/lib/desktop"

const LegacyApp = lazy(() => import("./LegacyApp"))
const PreviewApp = lazy(() => import("./social/PreviewApp").then(module => ({ default: module.PreviewApp })))
const SocialApp = lazy(() => import("./social/App"))
const legacy = IS_DESKTOP || new URLSearchParams(location.search).get("legacy") === "1" || new URLSearchParams(location.search).get("demo") === "1"

function ThemeSync() {
  const theme = useUiStore(s => s.theme)
  useEffect(() => { document.documentElement.classList.toggle("dark", theme === "dark") }, [theme])
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
  return <><ThemeSync /><NavigationSync /><WindowControls /><ErrorBoundary>
    <Suspense fallback={<div className="flex h-screen items-center justify-center bg-background text-sm text-muted-foreground">正在打开 LLM Nest…</div>}>
      {IS_SOCIAL_PREVIEW ? <PreviewApp /> : legacy ? <LegacyApp /> : <SocialApp />}
    </Suspense>
  </ErrorBoundary></>
}
