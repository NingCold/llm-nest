import { useEffect, useMemo, useRef, useState } from "react"
import { ArrowUp, FileText, Image as ImageIcon, Paperclip, Sparkles, Square, X } from "lucide-react"
import { PROMPT_TEMPLATES } from "@/lib/prompts"
import { useConfigStore } from "@/store/config"
import { useSessionStore } from "@/store/session"
import { useChatStore } from "@/store/chat"
import { useUiStore } from "@/store/ui"
import {
  cacheHitRate,
  formatThinking,
  formatTokenSpeed,
} from "@/lib/format"
import { cn } from "@/lib/utils"
import type { GuiAttachment } from "@/api/types"

/** 稳定空数组引用：避免 selector 每次返回新引用触发无限重渲染 */
const EMPTY_MESSAGES: never[] = []

function fmtSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`
}

/** 状态栏分段分隔符 */
function Sep() {
  return <span className="text-muted-foreground/25">|</span>
}

/** 读文件为 Base64 dataUrl（多模态请求体用） */
function readAsDataUrl(file: File): Promise<GuiAttachment> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader()
    reader.onload = () =>
      resolve({
        id: `${file.name}-${file.size}-${Date.now()}-${Math.random()
          .toString(36)
          .slice(2, 6)}`,
        name: file.name,
        mime: file.type || "application/octet-stream",
        size: file.size,
        dataUrl: String(reader.result),
      })
    reader.onerror = () => reject(new Error(`读取文件失败：${file.name}`))
    reader.readAsDataURL(file)
  })
}

interface InputBarProps {
  onSend: (text: string, attachments: GuiAttachment[]) => void
  onStop: () => void
  isStreaming: boolean
  disabled?: boolean
}

export function InputBar({ onSend, onStop, isStreaming, disabled }: InputBarProps) {
  const [value, setValue] = useState("")
  const [attachments, setAttachments] = useState<GuiAttachment[]>([])
  const [promptsOpen, setPromptsOpen] = useState(false)
  const [attachOpen, setAttachOpen] = useState(false)
  const [reading, setReading] = useState(false)
  const taRef = useRef<HTMLTextAreaElement>(null)
  const fileRef = useRef<HTMLInputElement>(null)

  const reasoningEffort = useUiStore((s) => s.reasoningEffort)
  const webSearchEnabled = useUiStore((s) => s.webSearchEnabled)
  const config = useConfigStore((s) => s.config)
  const providers = useConfigStore((s) => s.providers)

  // —— 会话级统计（输入框底部状态栏，DSH 风格）——
  const currentSessionId = useSessionStore((s) => s.currentSessionId)
  const messagesBySession = useChatStore((s) => s.messagesBySession)
  const sessionMessages = currentSessionId
    ? (messagesBySession[currentSessionId] ?? EMPTY_MESSAGES)
    : EMPTY_MESSAGES

  const sessionStats = useMemo(() => {
    const rounds = sessionMessages.filter((m) => m.role === "user").length
    let llmMs = 0
    let toolMs = 0
    let hasToolTime = false
    let ttftSum = 0
    let ttftCount = 0
    let inTok = 0
    let outTok = 0
    let cached = 0
    let hasUsage = false
    let steps = 0
    for (const m of sessionMessages) {
      if (m.role === "tool" || m.tools?.some((t) => t.kind === "call")) steps++
      // 工具用时 = 各工具结果块的执行耗时之和（0ms 也是真实耗时）
      const dur = m.tools?.find((t) => t.kind === "result")?.durationMs
      if (dur != null) {
        toolMs += dur
        hasToolTime = true
      }
      if (m.timings?.totalMs) llmMs += m.timings.totalMs
      if (m.timings?.ttftMs != null) {
        ttftSum += m.timings.ttftMs
        ttftCount++
      }
      if (m.usage) {
        hasUsage = true
        inTok += m.usage.promptTokens ?? 0
        outTok += m.usage.completionTokens ?? 0
        cached += m.usage.cachedTokens ?? 0
      }
    }
    const avgTtft = ttftCount > 0 ? formatThinking(ttftSum / ttftCount) : "-"
    const speed =
      outTok > 0 && llmMs > 0 ? formatTokenSpeed(outTok, llmMs) : "-"
    const rate = inTok > 0 ? cacheHitRate(cached || undefined, inTok) : "-"
    return {
      rounds,
      steps,
      llmMs,
      toolMs,
      hasToolTime,
      avgTtft,
      speed,
      inTok,
      outTok,
      rate,
      // 没有任何一条消息带 usage（provider 未返回/演示数据缺失）时用 "-"，
      // 避免把"没有数据"误显示成"消耗了 0 token"
      hasUsage,
      hasData: sessionMessages.length > 0,
    }
  }, [sessionMessages])

  const modelName = (() => {
    const m = config?.currentModel
    if (!m) return ""
    const p = providers.find((x) => x.id === m.provider)
    const model = p?.models.find((x) => x.id === m.model)
    return model?.displayName ?? `${m.provider}/${m.model}`
  })()

  const resize = () => {
    const el = taRef.current
    if (!el) return
    el.style.height = "auto"
    el.style.height = Math.min(el.scrollHeight, 208) + "px"
  }
  useEffect(resize, [value])

  /* Escape 关闭弹层 */
  useEffect(() => {
    if (!promptsOpen && !attachOpen) return
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        setPromptsOpen(false)
        setAttachOpen(false)
      }
    }
    window.addEventListener("keydown", onKey)
    return () => window.removeEventListener("keydown", onKey)
  }, [promptsOpen, attachOpen])

  const submit = () => {
    const text = value.trim()
    if (!text || isStreaming || disabled) return
    onSend(text, attachments)
    setValue("")
    setAttachments([])
    requestAnimationFrame(resize)
  }

  const pickPrompt = (prompt: string) => {
    setValue((prev) => (prev ? prev + "\n" + prompt : prompt))
    setPromptsOpen(false)
    taRef.current?.focus()
  }

  const onFiles = async (files: FileList | null) => {
    if (!files) return
    const list = Array.from(files)
    setReading(true)
    try {
      const next = await Promise.all(list.map((f) => readAsDataUrl(f)))
      setAttachments((prev) => [...prev, ...next])
    } catch (err) {
      console.error("[InputBar] read attachment failed:", err)
    } finally {
      setReading(false)
      setAttachOpen(false)
    }
  }

  const canSend = value.trim().length > 0 && !disabled

  return (
    <div className="shrink-0 px-4 pb-4 pt-2">
      <div className="relative mx-auto max-w-3xl">
        {/* popover backdrops */}
        {(promptsOpen || attachOpen) && (
          <div
            className="fixed inset-0 z-40"
            onClick={() => {
              setPromptsOpen(false)
              setAttachOpen(false)
            }}
          />
        )}

        {/* Prompt templates popover */}
        {promptsOpen && (
          <div className="absolute bottom-full left-0 z-50 mb-2 w-72 rounded-xl border border-border bg-popover p-1.5 shadow-xl animate-fade-up">
            <p className="px-2.5 pb-1 pt-1.5 text-xs font-medium text-muted-foreground">
              快捷模板
            </p>
            <div className="max-h-72 overflow-y-auto">
              {PROMPT_TEMPLATES.map((t) => {
                const Icon = t.icon
                return (
                  <button
                    key={t.id}
                    type="button"
                    onClick={() => pickPrompt(t.prompt)}
                    className="flex w-full items-center gap-2.5 rounded-lg px-2.5 py-2 text-left text-sm transition-colors hover:bg-accent"
                  >
                    <Icon className="h-4 w-4 shrink-0 text-muted-foreground" />
                    <span className="min-w-0">
                      <span className="block font-medium leading-tight">{t.title}</span>
                      <span className="block truncate text-xs text-muted-foreground">
                        {t.description}
                      </span>
                    </span>
                  </button>
                )
              })}
            </div>
          </div>
        )}

        {/* Attach popover */}
        {attachOpen && (
          <div className="absolute bottom-full left-0 z-50 mb-2 w-44 rounded-xl border border-border bg-popover p-1.5 shadow-xl animate-fade-up">
            <button
              type="button"
              onClick={() => {
                if (fileRef.current) {
                  fileRef.current.accept = "image/*"
                  fileRef.current.click()
                }
              }}
              className="flex w-full items-center gap-2.5 rounded-lg px-2.5 py-2 text-sm transition-colors hover:bg-accent"
            >
              <ImageIcon className="h-4 w-4 text-muted-foreground" />
              上传图片
            </button>
            <button
              type="button"
              onClick={() => {
                if (fileRef.current) {
                  fileRef.current.accept = ""
                  fileRef.current.click()
                }
              }}
              className="flex w-full items-center gap-2.5 rounded-lg px-2.5 py-2 text-sm transition-colors hover:bg-accent"
            >
              <FileText className="h-4 w-4 text-muted-foreground" />
              上传文件
            </button>
          </div>
        )}

        {/* Floating input card */}
        <div
          className={cn(
            "rounded-2xl border border-border bg-card shadow-xl shadow-zinc-900/[0.07] transition-all duration-200 dark:shadow-black/50",
            "focus-within:border-zinc-400 focus-within:shadow-zinc-900/[0.12] focus-within:ring-2 focus-within:ring-ring/40 dark:focus-within:border-zinc-500",
          )}
        >
          {/* Attachment chips */}
          {(attachments.length > 0 || reading) && (
            <div className="flex flex-wrap gap-2 px-3.5 pt-3">
              {attachments.map((a) => (
                <span
                  key={a.id}
                  className="flex items-center gap-1.5 rounded-lg border border-border bg-muted px-2.5 py-1.5 text-xs text-foreground"
                >
                  {a.mime.startsWith("image/") ? (
                    <img
                      src={a.dataUrl}
                      alt={a.name}
                      className="h-5 w-5 rounded object-cover"
                    />
                  ) : (
                    <FileText className="h-3.5 w-3.5 text-muted-foreground" />
                  )}
                  <span className="max-w-40 truncate">{a.name}</span>
                  <span className="text-muted-foreground">{fmtSize(a.size)}</span>
                  <button
                    type="button"
                    onClick={() =>
                      setAttachments((prev) => prev.filter((x) => x.id !== a.id))
                    }
                    className="ml-0.5 rounded p-0.5 text-muted-foreground transition-colors hover:bg-accent hover:text-foreground"
                    aria-label="移除附件"
                  >
                    <X className="h-3 w-3" />
                  </button>
                </span>
              ))}
              {reading && (
                <span className="flex items-center gap-1.5 rounded-lg border border-border bg-muted px-2.5 py-1.5 text-xs text-muted-foreground">
                  <span className="h-1.5 w-1.5 rounded-full bg-muted-foreground/70 animate-pulse-dot" />
                  读取中…
                </span>
              )}
            </div>
          )}

          <div className="flex items-end gap-1.5 p-2.5">
            {/* Attach */}
            <button
              type="button"
              onClick={() => {
                setAttachOpen((v) => !v)
                setPromptsOpen(false)
              }}
              className={cn(
                "flex h-9 w-9 shrink-0 items-center justify-center rounded-xl text-muted-foreground transition-colors hover:bg-accent hover:text-foreground",
                attachOpen && "bg-accent text-foreground",
              )}
              aria-label="添加附件"
            >
              <Paperclip className="h-4.5 w-4.5" />
            </button>

            {/* Prompt templates */}
            <button
              type="button"
              onClick={() => {
                setPromptsOpen((v) => !v)
                setAttachOpen(false)
              }}
              className={cn(
                "flex h-9 w-9 shrink-0 items-center justify-center rounded-xl text-muted-foreground transition-colors hover:bg-accent hover:text-foreground",
                promptsOpen && "bg-accent text-foreground",
              )}
              aria-label="快捷模板"
            >
              <Sparkles className="h-4.5 w-4.5" />
            </button>

            {/* Auto-resize textarea */}
            <textarea
              ref={taRef}
              rows={1}
              value={value}
              disabled={disabled}
              onChange={(e) => setValue(e.target.value)}
              onKeyDown={(e) => {
                if (
                  e.key === "Enter" &&
                  !e.shiftKey &&
                  !e.nativeEvent.isComposing
                ) {
                  e.preventDefault()
                  submit()
                }
              }}
              placeholder={disabled ? "先创建一个对话…" : "给 LLM Nest 发送消息…"}
              className="max-h-52 min-h-[38px] flex-1 resize-none bg-transparent px-1.5 py-2 text-[15px] leading-relaxed outline-none placeholder:text-muted-foreground disabled:cursor-not-allowed"
            />

            {/* Send / Stop */}
            {isStreaming ? (
              <button
                type="button"
                onClick={onStop}
                title="停止生成"
                aria-label="停止生成"
                className="flex h-9 w-9 shrink-0 items-center justify-center rounded-full bg-zinc-900 text-white transition-colors hover:bg-red-600 dark:bg-zinc-100 dark:text-zinc-900 dark:hover:bg-red-500 dark:hover:text-white"
              >
                <span className="flex animate-spin-slow">
                  <Square className="h-3.5 w-3.5 fill-current" />
                </span>
              </button>
            ) : (
              <button
                type="button"
                onClick={submit}
                disabled={!canSend}
                aria-label="发送"
                title="发送"
                className={cn(
                  "flex h-9 w-9 shrink-0 items-center justify-center rounded-full transition-all duration-200",
                  canSend
                    ? "bg-zinc-900 text-white hover:bg-zinc-700 dark:bg-zinc-100 dark:text-zinc-900 dark:hover:bg-zinc-300"
                    : "cursor-not-allowed bg-muted text-muted-foreground/50",
                )}
              >
                <ArrowUp className="h-4.5 w-4.5" />
              </button>
            )}
          </div>
        </div>

        {/* Caption */}
        <p className="mt-2.5 flex flex-wrap items-center justify-center gap-x-1.5 text-center text-xs text-muted-foreground">
          <span>LLM Nest 可能会犯错，请核查重要信息。</span>
          {modelName && (
            <>
              <span className="text-muted-foreground/50">·</span>
              <span>{modelName}</span>
            </>
          )}
          {reasoningEffort !== "off" && (
            <>
              <span className="text-muted-foreground/50">·</span>
              <span>思考 · {reasoningEffort}</span>
            </>
          )}
          {webSearchEnabled && (
            <>
              <span className="text-muted-foreground/50">·</span>
              <span>联网搜索</span>
            </>
          )}
        </p>

        {/* 会话级统计（轮数·步数 | LLM用时·工具用时 | 首token·速度 | token·缓存） */}
        {sessionStats.hasData && (
          <p className="mt-1 flex flex-wrap items-center justify-center gap-x-2 text-center font-mono text-[11px] text-muted-foreground/60">
            <span>
              {sessionStats.rounds} 轮 · {sessionStats.steps} 步
            </span>
            <Sep />
            <span>
              LLM {formatThinking(sessionStats.llmMs)} ·{" "}
              <span className="text-muted-foreground/35">
                工具{" "}
                {sessionStats.hasToolTime
                  ? formatThinking(sessionStats.toolMs)
                  : "-"}
              </span>
            </span>
            <Sep />
            <span>
              首token {sessionStats.avgTtft} · {sessionStats.speed}
            </span>
            <Sep />
            <span
              className="cursor-help border-b border-dotted border-muted-foreground/30"
              title={
                sessionStats.hasUsage
                  ? `输入 ${sessionStats.inTok} · 输出 ${sessionStats.outTok} · 缓存命中 ${sessionStats.rate}`
                  : "后端未返回 token 用量"
              }
            >
              输入 {sessionStats.hasUsage ? sessionStats.inTok : "-"} · 输出{" "}
              {sessionStats.hasUsage ? sessionStats.outTok : "-"} · 缓存{" "}
              {sessionStats.rate}
            </span>
          </p>
        )}
      </div>

      {/* Hidden file input (demo: 附件仅作展示) */}
      <input
        ref={fileRef}
        type="file"
        multiple
        className="hidden"
        onChange={(e) => {
          onFiles(e.target.files)
          e.target.value = ""
        }}
      />
    </div>
  )
}
