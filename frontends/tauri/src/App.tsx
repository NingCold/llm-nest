import { useInit } from "@/hooks/useInit"
import ErrorBoundary from "@/components/ErrorBoundary"
import { Sidebar } from "@/components/layout/Sidebar"
import { Toolbar } from "@/components/layout/Toolbar"
import { StatusBar } from "@/components/layout/StatusBar"
import { ChatView } from "@/components/chat/ChatView"

function App() {
  const { ready } = useInit()

  if (!ready) {
    return (
      <div className="h-screen w-screen flex items-center justify-center bg-zinc-950 text-white">
        Loading...
      </div>
    )
  }

  return (
    <ErrorBoundary>
      <div className="h-screen w-screen flex">
        <ErrorBoundary fallback={<div className="p-5 text-red-500">Sidebar Error</div>}>
          <Sidebar />
        </ErrorBoundary>
        <main className="flex-1 flex flex-col">
          <ErrorBoundary>
            <Toolbar />
          </ErrorBoundary>
          <ErrorBoundary>
            <ChatView />
          </ErrorBoundary>
          <ErrorBoundary>
            <StatusBar />
          </ErrorBoundary>
        </main>
      </div>
    </ErrorBoundary>
  )
}

export default App