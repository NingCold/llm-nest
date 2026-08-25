import { useEffect, useRef, useState } from "react"
import { Brain, ChevronDown } from "lucide-react"
import { formatThinking } from "@/lib/format"
import { cn } from "@/lib/utils"

interface ThinkingBlockProps {
  reasoning: string
  thinkingMs?: number
  isStreaming?: boolean
}

/**
 * 可折叠的「思考过程」卡片：浅灰背景、展开/折叠箭头、思考耗时。
 * 自动跟随生成阶段：思考流式输出期间展开（live 预览），正文开始输出
 * （thinkingMs 被置上）或消息完成后自动收起；用户手动切换过一次后不再
 * 自动干预（尊重用户选择）。标题行始终在，可随时手动展开/折叠。
 */
export function ThinkingBlock({ reasoning, thinkingMs, isStreaming }: ThinkingBlockProps) {
  const [open, setOpen] = useState(false)
  const userToggled = useRef(false)

  // 阶段跟随：思考中（流式且无正文耗时、有思考内容）→ 展开；正文输出/完成 → 收起
  useEffect(() => {
    if (userToggled.current) return
    const thinkingNow = isStreaming === true && thinkingMs == null && reasoning.length > 0
    setOpen(thinkingNow)
  }, [isStreaming, thinkingMs, reasoning])

  const live = isStreaming && !thinkingMs

  return (
    <div className="mb-2.5 overflow-hidden rounded-xl border border-border bg-muted/50">
      <button
        type="button"
        onClick={() => {
          userToggled.current = true
          setOpen((v) => !v)
        }}
        className="flex w-full items-center gap-2 px-3.5 py-2.5 text-left text-sm transition-colors hover:bg-accent/50"
      >
        <Brain className="h-4 w-4 shrink-0 text-muted-foreground" />
        <span className="font-medium">思考过程</span>
        <span className="flex-1" />
        {live ? (
          <span className="flex items-center gap-1.5 text-xs text-muted-foreground">
            <span className="h-1.5 w-1.5 rounded-full bg-muted-foreground/70 animate-pulse-dot" />
            思考中…
          </span>
        ) : thinkingMs ? (
          <span className="text-xs tabular-nums text-muted-foreground">
            {formatThinking(thinkingMs)}
          </span>
        ) : null}
        <ChevronDown
          className={cn(
            "h-4 w-4 shrink-0 text-muted-foreground transition-transform duration-200",
            open && "rotate-180",
          )}
        />
      </button>

      {/* Smooth expand/collapse via grid-rows trick */}
      <div
        className={cn(
          "grid transition-[grid-template-rows,opacity] duration-300 ease-in-out",
          open ? "grid-rows-[1fr] opacity-100" : "grid-rows-[0fr] opacity-0",
        )}
      >
        <div className="overflow-hidden">
          <div className="max-h-64 overflow-y-auto whitespace-pre-wrap border-t border-border/70 px-4 py-3 text-[13px] leading-relaxed text-muted-foreground">
            {reasoning}
            {isStreaming && (
              <span className="ml-0.5 inline-block h-3.5 w-[2px] translate-y-0.5 animate-caret-blink bg-muted-foreground/80" />
            )}
          </div>
        </div>
      </div>
    </div>
  )
}
