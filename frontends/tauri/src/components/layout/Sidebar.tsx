import { useSessionStore } from "@/store/session"
import { SessionList } from "@/components/session/SessionList"

export function Sidebar() {
  return (
    <aside className="w-64 border-r bg-card flex flex-col h-full">
      <div className="p-3 border-b">
        <h1 className="font-semibold text-lg">LLM Nest</h1>
      </div>
      <SessionList />
    </aside>
  )
}