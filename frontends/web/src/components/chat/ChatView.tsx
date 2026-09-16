import { useConfigStore } from "@/store/config"
import { useEffect, useRef, useState } from "react"
import { ArrowDown } from "lucide-react"
import { useVirtualizer } from "@tanstack/react-virtual"
import { useChat } from "@/hooks/useChat"
import { useChatStore } from "@/store/chat"
import { useSessionStore } from "@/store/session"
import { MessageItem } from "@/components/chat/MessageItem"
import { InputBar } from "@/components/chat/InputBar"
import { EmptyState } from "@/components/chat/EmptyState"
import type { GuiAttachment } from "@/api/types"

async function getApi() {
  const mod = await import("@/api")
  return mod.getApi()
}

/** 稳定空数组引用：避免 zustand selector 每次返回新引用导致无限重渲染 */
const EMPTY_MESSAGES: never[] = []

/**
 * 「吸底跟随」阈值（px，约一段对话的高度）：
 * - 距底部 < 阈值 → 视为接近底端：跟随生成内容自动下滑
 * - 距底部 ≥ 阈值 → 停止跟随，右下角显示置底按钮（点击回到底部并恢复跟随）
 */
const FOLLOW_THRESHOLD = 160

export function ChatView() {
  const hasModel = useConfigStore(s => Boolean(s.config?.currentModel.provider && s.config?.currentModel.model))
  const currentSessionId = useSessionStore((s) => s.currentSessionId)
  const messagesBySession = useChatStore((s) => s.messagesBySession)
  const messages = currentSessionId
    ? (messagesBySession[currentSessionId] ?? EMPTY_MESSAGES)
    : EMPTY_MESSAGES
  const hydrateSession = useChatStore((s) => s.hydrateSession)

  const { send, regenerate, editAndResend, cancel, isStreaming } = useChat()

  const [actionError, setActionError] = useState<string | null>(null)
  const [history, setHistory] = useState<{ id: string | null; error?: string }>({ id: null })
  const [historyAttempt, retryHistory] = useState(0)
  const historyReady = !currentSessionId || history.id === currentSessionId

  const scrollRef = useRef<HTMLDivElement>(null)
  const stickToBottom = useRef(true)
  const draggingScrollbar = useRef(false)
  const touchY = useRef<number | null>(null)
  /** 距底部超过阈值（不跟随）时显示右下角置底按钮 */
  const [showJump, setShowJump] = useState(false)

  /* 虚拟滚动：长会话只渲染可视区附近的节点 */
  const virtualizer = useVirtualizer({
    count: messages.length,
    getScrollElement: () => scrollRef.current,
    estimateSize: (index) => (messages[index]?.role === "assistant" ? 320 : 90),
    overscan: 8,
    getItemKey: (index) => messages[index].id,
  })

  /* 切换会话时从 API 载入历史消息（仅当内存中还没有该会话） */
  useEffect(() => {
    if (!currentSessionId) return
    if (Object.prototype.hasOwnProperty.call(useChatStore.getState().messagesBySession, currentSessionId)) {
      setHistory({ id: currentSessionId })
      return
    }
    let cancelled = false
    setHistory({ id: null })
    getApi().then(a => a.getMessages(currentSessionId)).then(msgs => {
      if (!cancelled) { hydrateSession(currentSessionId, msgs); setHistory({ id: currentSessionId }) }
    }).catch(error => {
      if (!cancelled) setHistory({ id: null, error: `历史记录加载失败：${String(error)}` })
    })
    return () => { cancelled = true }
  }, [currentSessionId, hydrateSession, historyAttempt])

  /* 自动滚动（用户未上翻时跟随最新内容；rAF 等虚拟化测量落定）：
     依赖 messages 与 virtualizer 总高度——思考卡片展开/收起、markdown 渲染、
     虚拟化测量变化等"布局变化但不改消息"的场景也会重新吸底 */
  useEffect(() => {
    if (!stickToBottom.current || messages.length === 0) return
    const el = scrollRef.current
    if (!el) return
    const raf = requestAnimationFrame(() => {
      if (stickToBottom.current) el.scrollTop = el.scrollHeight
    })
    return () => cancelAnimationFrame(raf)
  }, [messages, virtualizer.getTotalSize()])

  /* 切换会话：回到吸底跟随 */
  useEffect(() => {
    stickToBottom.current = true
    setShowJump(false)
  }, [currentSessionId])

  useEffect(() => {
    const release = () => { draggingScrollbar.current = false }
    window.addEventListener("pointerup", release)
    window.addEventListener("pointercancel", release)
    return () => {
      window.removeEventListener("pointerup", release)
      window.removeEventListener("pointercancel", release)
    }
  }, [])

  const handleScroll = () => {
    const el = scrollRef.current
    if (!el) return
    const dist = el.scrollHeight - el.scrollTop - el.clientHeight
    // Virtualizer corrections can move scrollTop in either direction. A
    // scroll event alone is not evidence that the user stopped following.
    if (draggingScrollbar.current) stickToBottom.current = dist < FOLLOW_THRESHOLD
    else if (dist < FOLLOW_THRESHOLD) stickToBottom.current = true
    setShowJump(!stickToBottom.current)
  }

  const resumeFollowing = () => {
    stickToBottom.current = true
    setShowJump(false)
  }

  const pauseFollowing = () => {
    stickToBottom.current = false
    setShowJump(true)
  }

  const jumpToBottom = () => {
    const el = scrollRef.current
    if (!el) return
    resumeFollowing()
    el.scrollTo({ top: el.scrollHeight, behavior: "smooth" })
  }

  const handlePick = (prompt: string) => {
    if (historyReady && hasModel) { resumeFollowing(); void send(prompt) }
  }

  const handleSend = (text: string, attachments: GuiAttachment[]) => {
    if (historyReady && hasModel) { resumeFollowing(); void send(text, attachments) }
  }

  return (
    <div className="relative flex min-h-0 flex-1 flex-col">
      {!hasModel && <p className="p-3 text-sm text-muted-foreground">尚未配置模型，请在设置中添加供应商。</p>}
      {actionError && <p role="alert" className="p-3 text-sm text-red-500">{actionError}</p>}
      {!historyReady && <div className="p-4 text-sm" role="status">{history.error ?? "正在加载历史记录…"}
        {history.error && <button onClick={() => retryHistory(n => n + 1)} className="ml-3 underline">重试加载</button>}
      </div>}
      {/* Message feed（外层 relative：置底按钮相对滚动区定位，不随内容滚动） */}
      <div className="relative flex min-h-0 flex-1 flex-col">
        <div
          ref={scrollRef}
          onScroll={handleScroll}
          onWheel={e => { if (e.deltaY < 0) pauseFollowing() }}
          onPointerDown={e => { draggingScrollbar.current = e.target === e.currentTarget }}
          onTouchStart={e => { touchY.current = e.touches[0]?.clientY ?? null }}
          onTouchMove={e => {
            const y = e.touches[0]?.clientY ?? null
            if (y !== null && touchY.current !== null && y > touchY.current) pauseFollowing()
            touchY.current = y
          }}
          onKeyDown={e => { if (["ArrowUp", "PageUp", "Home"].includes(e.key)) pauseFollowing() }}
          className="overflow-anchor-none min-h-0 flex-1 overflow-y-auto"
        >
          {messages.length === 0 ? (
            <div className="mx-auto flex h-full w-full max-w-3xl flex-col px-4 sm:px-6">
              <EmptyState onPick={handlePick} />
            </div>
          ) : (
            <div
              className="relative mx-auto w-full max-w-3xl"
              style={{ height: virtualizer.getTotalSize() }}
            >
              {virtualizer.getVirtualItems().map((vi) => {
                const m = messages[vi.index]
                return (
                  <div
                    key={vi.key}
                    data-index={vi.index}
                    ref={virtualizer.measureElement}
                    className="absolute left-0 top-0 w-full px-4 sm:px-6"
                    style={{ transform: `translateY(${vi.start}px)` }}
                  >
                    <div className="py-3.5">
                      <MessageItem
                        sessionId={currentSessionId ?? ""}
                        message={m}
                        isStreaming={isStreaming}
                        onRegenerate={(id) => { resumeFollowing(); void regenerate(id) }}
                        onEditResend={(id, content) => { resumeFollowing(); void editAndResend(id, content) }}
                      />
                    </div>
                  </div>
                )
              })}
            </div>
          )}
        </div>

        {/* 置底按钮：用户上翻（距底部 ≥ 阈值）时出现，点击平滑回到底部并恢复跟随 */}
        {showJump && messages.length > 0 && (
          <button
            type="button"
            onClick={jumpToBottom}
            aria-label="滚动到底部"
            title="滚动到底部"
            className="absolute bottom-4 right-4 z-20 flex h-10 w-10 items-center justify-center rounded-full border border-border bg-card/95 text-muted-foreground shadow-lg backdrop-blur transition-colors animate-fade-up hover:bg-accent hover:text-foreground"
          >
            <ArrowDown className="h-4 w-4" />
          </button>
        )}
      </div>

      {/* Floating input bar */}
      <InputBar
        onSend={handleSend}
        onStop={() => { setActionError(null); void cancel().catch(e => setActionError(`停止失败，请重试：${String(e)}`)) }}
        isStreaming={isStreaming}
        disabled={!currentSessionId || !historyReady || !hasModel}
      />
    </div>
  )
}
