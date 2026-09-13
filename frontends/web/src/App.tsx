import { useSessionStore } from "@/store/session"
import { useConfigStore } from "@/store/config"
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
  const { ready, error, retry } = useInit()
  const saveError = useConfigStore(s => s.saveError)
  const retrySave = useConfigStore(s => s.retrySave)
  const sessionError = useSessionStore(s => s.error)
  const refreshSessions = useSessionStore(s => s.refreshSessions)
  const sessionLoading = useSessionStore(s => s.loading)
  const saving = useConfigStore(s => s.saving)

  if (error) return <div className="flex h-screen flex-col items-center justify-center gap-4 p-8 bg-background text-foreground">
    <h1 className="text-lg font-medium">无法启动 LLM Nest</h1>
    <p role="alert" className="max-w-xl whitespace-pre-wrap break-words text-sm text-red-500">{error}</p>
    <p className="text-sm text-muted-foreground">请检查后端连接、配置文件和数据目录占用情况后重试。</p>
    <button className="rounded border px-4 py-2" onClick={retry}>重新连接</button>
  </div>

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
          {sessionError && <div role="alert" className="border-b p-3 text-sm text-red-500">{sessionError} <button disabled={sessionLoading} onClick={() => void refreshSessions()} className="underline">重试刷新</button></div>}
          {saveError && <div role="alert" className="border-b p-3 text-sm text-red-500">{saveError} <button disabled={saving} onClick={retrySave} className="underline">重试保存</button></div>}
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
