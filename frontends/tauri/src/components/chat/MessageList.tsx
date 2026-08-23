import { useRef, useEffect } from "react"
import { useChatStore } from "@/store/chat"
import { useSessionStore } from "@/store/session"
import { MessageBubble } from "./MessageBubble"

export function MessageList() {
  const currentSessionId = useSessionStore((s) => s.currentSessionId)
  const getMessages = useChatStore((s) => s.getMessages)
  const scrollRef = useRef<HTMLDivElement>(null)

  const messages = currentSessionId ? getMessages(currentSessionId) : []

  useEffect(() => {
    if (scrollRef.current) {
      scrollRef.current.scrollTop = scrollRef.current.scrollHeight
    }
  }, [messages])

  if (!currentSessionId) {
    return (
      <div className="flex-1 flex items-center justify-center text-muted-foreground">
        Select or create a session to start chatting
      </div>
    )
  }

  return (
    <div className="flex-1 overflow-y-auto px-4 py-4" ref={scrollRef}>
      <div className="space-y-4">
        {messages.map((msg) => (
          <MessageBubble
            key={msg.id}
            role={msg.role}
            content={msg.content}
            status={msg.status}
            error={msg.error}
          />
        ))}
      </div>
    </div>
  )
}