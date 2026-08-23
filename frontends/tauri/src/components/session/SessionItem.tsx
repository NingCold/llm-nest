import { cn } from "@/lib/utils"

interface SessionItemProps {
  title: string
  messageCount: number
  isActive: boolean
  onClick: () => void
}

export function SessionItem({ title, messageCount, isActive, onClick }: SessionItemProps) {
  return (
    <button
      onClick={onClick}
      className={cn(
        "w-full text-left px-3 py-2 rounded-md text-sm transition-colors",
        "hover:bg-accent hover:text-accent-foreground",
        isActive && "bg-accent text-accent-foreground font-medium",
      )}
    >
      <div className="truncate">{title || "Untitled"}</div>
      <div className="text-xs text-muted-foreground mt-0.5">
        {messageCount} messages
      </div>
    </button>
  )
}