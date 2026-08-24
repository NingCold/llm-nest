import { useEffect, useRef } from "react"
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

export function ChatView() {
  const currentSessionId = useSessionStore((s) => s.currentSessionId)
  const messagesBySession = useChatStore((s) => s.messagesBySession)
  const messages = currentSessionId
    ? (messagesBySession[currentSessionId] ?? EMPTY_MESSAGES)
    : EMPTY_MESSAGES
  const hydrateSession = useChatStore((s) => s.hydrateSession)

  const { send, regenerate, editAndResend, cancel, isStreaming } = useChat()

  const scrollRef = useRef<HTMLDivElement>(null)
  const stickToBottom = useRef(true)

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
    const existing = useChatStore.getState().getMessages(currentSessionId)
    if (existing.length > 0) return
    let cancelled = false
    getApi()
      .then((a) => a.getMessages(currentSessionId))
      .then((msgs) => {
        if (!cancelled && msgs.length > 0) {
          hydrateSession(currentSessionId, msgs)
        }
      })
      .catch(() => {
        /* demo adapter 不会失败；静默 */
      })
    return () => {
      cancelled = true
    }
  }, [currentSessionId, hydrateSession])

  /* 自动滚动（用户未上翻时跟随最新内容；rAF 等虚拟化测量落定） */
  useEffect(() => {
    if (!stickToBottom.current || messages.length === 0) return
    const el = scrollRef.current
    if (!el) return
    const raf = requestAnimationFrame(() => {
      el.scrollTop = el.scrollHeight
    })
    return () => cancelAnimationFrame(raf)
  }, [messages])

  /* 切换会话：回到吸底跟随 */
  useEffect(() => {
    stickToBottom.current = true
  }, [currentSessionId])

  const handleScroll = () => {
    const el = scrollRef.current
    if (!el) return
    stickToBottom.current =
      el.scrollHeight - el.scrollTop - el.clientHeight < 120
  }

  const handlePick = (prompt: string) => {
    void send(prompt)
  }

  const handleSend = (text: string, attachments: GuiAttachment[]) => {
    void send(text, attachments)
  }

  return (
    <div className="relative flex min-h-0 flex-1 flex-col">
      {/* Message feed */}
      <div ref={scrollRef} onScroll={handleScroll} className="min-h-0 flex-1 overflow-y-auto">
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
                      onRegenerate={(id) => void regenerate(id)}
                      onEditResend={(id, content) => void editAndResend(id, content)}
                    />
                  </div>
                </div>
              )
            })}
          </div>
        )}
      </div>

      {/* Floating input bar */}
      <InputBar
        onSend={handleSend}
        onStop={() => void cancel()}
        isStreaming={isStreaming}
        disabled={!currentSessionId}
      />
    </div>
  )
}
