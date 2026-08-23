import { useState, useRef } from "react"
import { Button } from "@/components/ui/button"
import { useChatStore } from "@/store/chat"

interface InputBoxProps {
  onSend: (input: string) => void
  onCancel: () => void
}

export function InputBox({ onSend, onCancel }: InputBoxProps) {
  const [input, setInput] = useState("")
  const isStreaming = useChatStore((s) => s.isStreaming)
  const textareaRef = useRef<HTMLTextAreaElement>(null)

  const handleSend = () => {
    const trimmed = input.trim()
    if (!trimmed || isStreaming) return
    onSend(trimmed)
    setInput("")
    if (textareaRef.current) {
      textareaRef.current.style.height = "auto"
    }
  }

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault()
      handleSend()
    }
  }

  const handleInput = (e: React.ChangeEvent<HTMLTextAreaElement>) => {
    setInput(e.target.value)
    if (textareaRef.current) {
      textareaRef.current.style.height = "auto"
      textareaRef.current.style.height = `${textareaRef.current.scrollHeight}px`
    }
  }

  return (
    <div className="border-t p-4">
      <div className="flex items-end gap-2 max-w-4xl mx-auto">
        <textarea
          ref={textareaRef}
          value={input}
          onChange={handleInput}
          onKeyDown={handleKeyDown}
          placeholder="Type a message... (Shift+Enter for new line)"
          rows={1}
          className="flex-1 resize-none rounded-lg border border-input bg-background px-3 py-2 text-sm ring-offset-background placeholder:text-muted-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 disabled:opacity-50 min-h-[40px] max-h-[200px]"
          disabled={isStreaming}
        />
        {isStreaming ? (
          <Button variant="destructive" size="sm" onClick={onCancel}>
            Stop
          </Button>
        ) : (
          <Button size="sm" onClick={handleSend} disabled={!input.trim()}>
            Send
          </Button>
        )}
      </div>
    </div>
  )
}