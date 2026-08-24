import { useState } from "react"
import {
  Check,
  CircleAlert,
  FileText,
  Loader2,
  Pencil,
  Sparkles,
  Wrench,
  X,
} from "lucide-react"
import type { GuiToolBlock } from "@/api/types"
import type { Message } from "@/store/chat"
import { MarkdownView } from "@/components/chat/MarkdownView"
import { ThinkingBlock } from "@/components/chat/ThinkingBlock"
import { MessageActions } from "@/components/chat/MessageActions"
import { initials, formatThinking } from "@/lib/format"

interface MessageItemProps {
  sessionId: string
  message: Message
  isStreaming: boolean
  onRegenerate: (messageId: string) => void
  onEditResend: (messageId: string, newContent: string) => void
}

function Skeleton() {
  return (
    <div className="space-y-2.5 py-1" aria-hidden>
      <div className="h-3.5 w-3/4 rounded-full bg-muted animate-breathe" />
      <div className="h-3.5 w-full rounded-full bg-muted animate-breathe [animation-delay:150ms]" />
      <div className="h-3.5 w-2/3 rounded-full bg-muted animate-breathe [animation-delay:300ms]" />
    </div>
  )
}

function UserMessage({
  message,
  isStreaming,
  onEditResend,
}: {
  message: Message
  isStreaming: boolean
  onEditResend: (messageId: string, newContent: string) => void
}) {
  const [editing, setEditing] = useState(false)
  const [draft, setDraft] = useState(message.content)

  const save = () => {
    setEditing(false)
    const next = draft.trim()
    if (next && next !== message.content) {
      onEditResend(message.id, next)
    }
  }

  return (
    <div className="group flex items-start justify-end gap-3 animate-fade-up">
      <div className="flex max-w-[82%] flex-col items-end">
        {editing ? (
          <div className="w-full min-w-[300px] rounded-2xl rounded-br-md border border-input bg-card p-3 shadow-sm">
            <textarea
              autoFocus
              rows={4}
              value={draft}
              onChange={(e) => setDraft(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter" && !e.shiftKey) {
                  e.preventDefault()
                  save()
                }
              }}
              className="w-full resize-none bg-transparent text-[15px] leading-relaxed outline-none"
            />
            <div className="mt-2 flex justify-end gap-2">
              <button
                type="button"
                onClick={() => setEditing(false)}
                className="flex h-8 items-center gap-1 rounded-lg px-3 text-sm text-muted-foreground transition-colors hover:bg-accent hover:text-foreground"
              >
                <X className="h-3.5 w-3.5" />
                取消
              </button>
              <button
                type="button"
                onClick={save}
                className="flex h-8 items-center gap-1 rounded-lg bg-zinc-900 px-3 text-sm font-medium text-white transition-colors hover:bg-zinc-700 dark:bg-zinc-100 dark:text-zinc-900 dark:hover:bg-zinc-300"
              >
                <Check className="h-3.5 w-3.5" />
                保存并重新生成
              </button>
            </div>
          </div>
        ) : (
          <>
            {/* 附件：图片缩略图 / 文件 chip */}
            {message.attachments && message.attachments.length > 0 && (
              <div className="mb-2 flex max-w-full flex-wrap justify-end gap-2">
                {message.attachments.map((a) =>
                  a.mime.startsWith("image/") ? (
                    <img
                      key={a.id}
                      src={a.dataUrl}
                      alt={a.name}
                      title={a.name}
                      className="h-36 w-36 rounded-xl border border-border object-cover shadow-sm"
                    />
                  ) : (
                    <span
                      key={a.id}
                      className="flex items-center gap-1.5 rounded-lg border border-border bg-muted px-2.5 py-1.5 text-xs text-foreground"
                    >
                      <FileText className="h-3.5 w-3.5 text-muted-foreground" />
                      <span className="max-w-40 truncate">{a.name}</span>
                    </span>
                  ),
                )}
              </div>
            )}
            <div className="whitespace-pre-wrap rounded-2xl rounded-br-md bg-zinc-900 px-4 py-2.5 text-[15px] leading-relaxed text-zinc-50 shadow-sm dark:bg-zinc-100 dark:text-zinc-900">
              {message.content}
            </div>
            {!isStreaming && (
              <button
                type="button"
                onClick={() => {
                  setDraft(message.content)
                  setEditing(true)
                }}
                className="mt-1 flex items-center gap-1 rounded-md px-2 py-1 text-xs text-muted-foreground opacity-0 transition-all hover:bg-accent hover:text-foreground group-hover:opacity-100"
              >
                <Pencil className="h-3 w-3" />
                编辑
              </button>
            )}
          </>
        )}
        {message.status === "error" && (
          <p className="mt-1 text-xs text-red-500">{message.error}</p>
        )}
      </div>
      <div className="flex h-8 w-8 shrink-0 items-center justify-center rounded-full bg-gradient-to-br from-zinc-500 to-zinc-800 text-[11px] font-semibold text-white">
        {initials("Demo User")}
      </div>
    </div>
  )
}

function AssistantMessage({
  sessionId,
  message,
  onRegenerate,
}: {
  sessionId: string
  message: Message
  onRegenerate: (messageId: string) => void
}) {
  const streaming = message.status === "streaming"
  const showThinking =
    Boolean(message.reasoning) || (streaming && !message.content)
  const showSkeleton = streaming && !message.content && !message.reasoning

  return (
    <div className="group flex items-start gap-3 animate-fade-up">
      <div className="flex h-8 w-8 shrink-0 items-center justify-center rounded-lg bg-gradient-to-br from-zinc-700 to-zinc-950 text-white shadow-sm dark:from-zinc-200 dark:to-zinc-500 dark:text-zinc-900">
        <Sparkles className="h-4 w-4" />
      </div>

      <div className="min-w-0 flex-1 pt-0.5">
        {showThinking && (
          <ThinkingBlock
            reasoning={message.reasoning ?? ""}
            thinkingMs={message.thinkingMs}
            isStreaming={streaming}
          />
        )}

        {message.tools?.filter((t) => t.kind === "call").length ? (
          <div className="mb-2 space-y-1.5">
            {message.tools
              .filter((t) => t.kind === "call")
              .map((t) => (
                <ToolCallCard key={t.id} block={t} />
              ))}
          </div>
        ) : null}

        {showSkeleton ? (
          <Skeleton />
        ) : message.content ? (
          <div className="relative">
            <MarkdownView content={message.content} />
            {streaming && (
              <span className="ml-0.5 inline-block h-4 w-[2px] translate-y-0.5 animate-caret-blink bg-foreground/70" />
            )}
          </div>
        ) : null}

        {message.status === "error" && (
          <p className="mt-2 rounded-lg border border-red-500/30 bg-red-500/5 px-3 py-2 text-sm text-red-500">
            生成失败：{message.error}
          </p>
        )}
        {message.status === "cancelled" && (
          <p className="mt-2 text-xs text-muted-foreground">已停止生成</p>
        )}

        {message.status === "done" && message.content && (
          <MessageActions
            sessionId={sessionId}
            message={message}
            onRegenerate={() => onRegenerate(message.id)}
          />
        )}
      </div>
    </div>
  )
}

export function MessageItem(props: MessageItemProps) {
  if (props.message.role === "user") {
    return (
      <UserMessage
        message={props.message}
        isStreaming={props.isStreaming}
        onEditResend={props.onEditResend}
      />
    )
  }
  if (props.message.role === "tool") {
    return <ToolMessage message={props.message} />
  }
  return (
    <AssistantMessage
      sessionId={props.sessionId}
      message={props.message}
      onRegenerate={props.onRegenerate}
    />
  )
}

/** 工具调用卡片（assistant 消息内） */
function ToolCallCard({ block }: { block: GuiToolBlock }) {
  let args = block.arguments
  try {
    if (args) args = JSON.stringify(JSON.parse(args), null, 2)
  } catch {
    /* keep raw */
  }
  return (
    <div className="flex items-start gap-2 rounded-lg border border-border bg-muted/40 px-3 py-2 text-[13px]">
      <Wrench className="mt-0.5 h-3.5 w-3.5 shrink-0 text-muted-foreground" />
      <div className="min-w-0 flex-1">
        <p className="font-mono text-xs font-medium text-foreground">{block.name}</p>
        {args ? (
          <pre className="mt-1 overflow-x-auto whitespace-pre-wrap font-mono text-[11px] leading-relaxed text-muted-foreground">
            {args}
          </pre>
        ) : null}
      </div>
    </div>
  )
}

/** 工具执行结果消息（role=tool） */
function ToolMessage({ message }: { message: Message }) {
  const block = message.tools?.[0]
  return (
    <div className="flex items-start gap-3 animate-fade-up">
      <div className="flex h-8 w-8 shrink-0 items-center justify-center rounded-lg bg-muted text-muted-foreground">
        <Wrench className="h-4 w-4" />
      </div>
      <div className="min-w-0 flex-1 pt-0.5">
        <div className="inline-flex items-center gap-2 rounded-lg border border-border bg-muted/40 px-3 py-2 text-[13px]">
          {message.toolPending ? (
            <>
              <Loader2 className="h-3.5 w-3.5 animate-spin text-muted-foreground" />
              <span className="font-mono text-xs">正在调用 {block?.name} …</span>
            </>
          ) : block?.isError ? (
            <>
              <CircleAlert className="h-3.5 w-3.5 shrink-0 text-red-500" />
              <span className="font-mono text-xs text-red-500">
                {block.name} 失败：{message.content}
              </span>
            </>
          ) : (
            <span className="font-mono text-xs text-muted-foreground">
              {block?.name} → {block?.content ?? message.content}
              {block?.durationMs != null && (
                <span className="ml-1.5 text-muted-foreground/60">
                  · {formatThinking(block.durationMs)}
                </span>
              )}
            </span>
          )}
        </div>
      </div>
    </div>
  )
}
