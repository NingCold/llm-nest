import { cn } from "@/lib/utils"

interface MessageBubbleProps {
  role: "user" | "assistant"
  content: string
  status: "pending" | "streaming" | "done" | "error" | "cancelled"
  error?: string
}

export function MessageBubble({ role, content, status, error }: MessageBubbleProps) {
  const isUser = role === "user"

  return (
    <div className={cn("flex w-full", isUser ? "justify-end" : "justify-start")}>
      <div
        className={cn(
          "max-w-[80%] rounded-lg px-4 py-2 whitespace-pre-wrap break-words",
          isUser
            ? "bg-primary text-primary-foreground"
            : "bg-muted text-muted-foreground",
          status === "error" && "bg-destructive/10 text-destructive border border-destructive/30",
          status === "cancelled" && "opacity-60",
        )}
      >
        {content}
        {status === "streaming" && (
          <span className="inline-block w-1.5 h-4 ml-0.5 bg-current animate-pulse" />
        )}
        {status === "error" && error && (
          <div className="text-xs mt-1 opacity-70">{error}</div>
        )}
        {status === "cancelled" && (
          <span className="text-xs ml-2 opacity-50">(cancelled)</span>
        )}
      </div>
    </div>
  )
}