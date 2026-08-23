import { useSessionStore } from "@/store/session"
import { SessionItem } from "./SessionItem"
import { Button } from "@/components/ui/button"
import { ScrollArea } from "@/components/ui/scroll-area"

export function SessionList() {
  const sessions = useSessionStore((s) => s.sessions)
  const currentSessionId = useSessionStore((s) => s.currentSessionId)
  const setCurrentSession = useSessionStore((s) => s.setCurrentSession)
  const createSession = useSessionStore((s) => s.createSession)

  return (
    <div className="flex flex-col h-full">
      <div className="p-3 border-b">
        <Button
          variant="outline"
          size="sm"
          className="w-full"
          onClick={() => createSession()}
        >
          + New Session
        </Button>
      </div>
      <ScrollArea className="flex-1 p-2">
        <div className="space-y-1">
          {sessions.map((session) => (
            <SessionItem
              key={session.id}
              title={session.title}
              messageCount={session.messageCount}
              isActive={session.id === currentSessionId}
              onClick={() => setCurrentSession(session.id)}
            />
          ))}
          {sessions.length === 0 && (
            <div className="text-sm text-muted-foreground text-center py-8">
              No sessions yet
            </div>
          )}
        </div>
      </ScrollArea>
    </div>
  )
}