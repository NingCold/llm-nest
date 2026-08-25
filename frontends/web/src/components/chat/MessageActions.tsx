import { useState } from "react"
import { Check, CircleHelp, Copy, RotateCcw, ThumbsDown, ThumbsUp } from "lucide-react"
import { useChatStore, type Feedback, type Message } from "@/store/chat"
import {
  cacheHitRate,
  copyText,
  formatMessageTime,
  formatThinking,
  formatTokenSpeed,
} from "@/lib/format"
import { cn } from "@/lib/utils"

interface MessageActionsProps {
  sessionId: string
  message: Message
  onRegenerate?: () => void
}

async function getApi() {
  const mod = await import("@/api")
  return mod.getApi()
}

export function MessageActions({ sessionId, message, onRegenerate }: MessageActionsProps) {
  const setFeedback = useChatStore((s) => s.setFeedback)
  const [copied, setCopied] = useState(false)

  const handleCopy = async () => {
    const ok = await copyText(message.content)
    if (ok) {
      setCopied(true)
      setTimeout(() => setCopied(false), 1800)
    }
  }

  /** 乐观更新本地反馈并持久化到后端（失败静默，刷新后以服务端为准） */
  const toggleFeedback = (fb: Feedback) => {
    const next = message.feedback === fb ? null : fb
    setFeedback(sessionId, message.id, next)
    void getApi()
      .then((a) => a.setMessageFeedback(sessionId, message.id, next))
      .catch(() => {
        /* 持久化失败不打断交互 */
      })
  }

  const btn =
    "flex h-8 w-8 items-center justify-center rounded-lg text-zinc-500 transition-colors hover:bg-accent hover:text-zinc-900 dark:text-zinc-400 dark:hover:text-zinc-100"

  // —— hover 统计（assistant 消息带 usage/timings 时显示）——
  const { usage, timings, createdAt } = message
  const hasStats = !!usage || !!timings
  const totalMs = timings?.totalMs
  // 消耗量 = 输入 + 输出（本次回复的总 token 数）
  const tokensTotal =
    usage && (usage.promptTokens || usage.completionTokens)
      ? usage.promptTokens + usage.completionTokens
      : 0
  const hasTokens = !!usage

  return (
    <div className="group/actions relative">
      <div className="mt-2 flex items-center gap-0.5">
        <button type="button" onClick={handleCopy} className={btn} title="复制全文">
          {copied ? <Check className="h-4 w-4 text-emerald-500" /> : <Copy className="h-4 w-4" />}
        </button>
        {onRegenerate && (
          <button type="button" onClick={onRegenerate} className={btn} title="重新生成">
            <RotateCcw className="h-4 w-4" />
          </button>
        )}
        <button
          type="button"
          onClick={() => toggleFeedback("up")}
          className={cn(btn, message.feedback === "up" && "text-emerald-500 hover:text-emerald-500")}
          title="有帮助"
        >
          <ThumbsUp className="h-4 w-4" />
        </button>
        <button
          type="button"
          onClick={() => toggleFeedback("down")}
          className={cn(btn, message.feedback === "down" && "text-red-500 hover:text-red-500")}
          title="没帮助"
        >
          <ThumbsDown className="h-4 w-4" />
        </button>
      </div>

      {hasStats && (
        <div className="mt-1 flex flex-wrap items-center gap-x-2 gap-y-0.5 text-[11px] leading-tight text-muted-foreground/60 opacity-0 transition-opacity duration-150 group-hover/actions:opacity-100">
          {createdAt ? <span>{formatMessageTime(createdAt)}</span> : null}
          {totalMs ? <span>用时 {formatThinking(totalMs)}</span> : null}
          {timings?.ttftMs != null ? (
            <span>首token {formatThinking(timings.ttftMs)}</span>
          ) : null}
          {usage && totalMs ? (
            <span>{formatTokenSpeed(usage.completionTokens, totalMs)}</span>
          ) : null}
          {hasTokens ? (
            <span className="inline-flex items-center gap-1">
              <span>消耗 {tokensTotal} tokens</span>
              {/* 小圆圈问号：悬停查看本次回复的输入/输出/缓存命中 */}
              <span className="group/help relative inline-flex">
                <CircleHelp
                  aria-label="查看本次回复的 token 明细"
                  className="h-3.5 w-3.5 cursor-help text-muted-foreground/40 transition-colors hover:text-muted-foreground/70"
                />
                <span
                  role="tooltip"
                  className="pointer-events-none absolute bottom-full left-1/2 z-30 mb-1.5 -translate-x-1/2 whitespace-nowrap rounded-md border border-border bg-popover px-2 py-1 text-[11px] font-normal text-popover-foreground shadow-md opacity-0 transition-opacity duration-150 group-hover/help:opacity-100"
                >
                  输入 {usage.promptTokens} · 输出 {usage.completionTokens} · 缓存命中{" "}
                  {cacheHitRate(usage.cachedTokens, usage.promptTokens)}
                </span>
              </span>
            </span>
          ) : null}
        </div>
      )}
    </div>
  )
}
