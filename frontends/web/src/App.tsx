import { useEffect } from "react"
import { useInit } from "@/hooks/useInit"
import ErrorBoundary from "@/components/ErrorBoundary"
import { Sidebar } from "@/components/layout/Sidebar"
import { Header } from "@/components/layout/Header"
import { ChatView } from "@/components/chat/ChatView"
import { useUiStore } from "@/store/ui"

/** 把主题应用到 <html class="dark">，并监听切换 */
function ThemeSync() {
  const theme = useUiStore((s) => s.theme)
  useEffect(() => {
    document.documentElement.classList.toggle("dark", theme === "dark")
  }, [theme])
  return null
}

function App() {
  const { ready } = useInit()

  if (!ready) {
    return (
      <div className="flex h-screen w-screen items-center justify-center bg-background text-foreground">
        <div className="flex items-center gap-2 text-sm text-muted-foreground">
          <span className="h-2 w-2 rounded-full bg-zinc-400 animate-pulse-dot" />
          正在启动…
        </div>
      </div>
    )
  }

  return (
    <ErrorBoundary>
      <ThemeSync />
      <div className="flex h-screen w-screen overflow-hidden bg-background text-foreground">
        <ErrorBoundary fallback={<div className="p-5 text-red-500">Sidebar Error</div>}>
          <Sidebar />
        </ErrorBoundary>
        <main className="flex min-w-0 flex-1 flex-col">
          <ErrorBoundary>
            <Header />
          </ErrorBoundary>
          <ErrorBoundary>
            <ChatView />
          </ErrorBoundary>
        </main>
      </div>
    </ErrorBoundary>
  )
}

export default App
