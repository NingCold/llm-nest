import { useCallback, useMemo } from "react"
import { useChatStore } from "@/store/chat"
import { useSessionStore } from "@/store/session"
import { useConfigStore } from "@/store/config"
import { useUiStore, clampEffort } from "@/store/ui"
import type { GuiAttachment } from "@/api/types"

async function getApi() {
  const mod = await import("@/api")
  return mod.getApi()
}

export function useChat() {
  const isStreaming = useChatStore((s) => s.isStreaming)
  const setStreaming = useChatStore((s) => s.setStreaming)
  const startAssistantMessage = useChatStore((s) => s.startAssistantMessage)
  const appendDelta = useChatStore((s) => s.appendDelta)
  const appendReasoningDelta = useChatStore((s) => s.appendReasoningDelta)
  const finishMessage = useChatStore((s) => s.finishMessage)
  const failMessage = useChatStore((s) => s.failMessage)
  const cancelMessage = useChatStore((s) => s.cancelMessage)
  const addUserMessage = useChatStore((s) => s.addUserMessage)
  const editUserMessage = useChatStore((s) => s.editUserMessage)
  const truncateFrom = useChatStore((s) => s.truncateFrom)
  const setThinkingTime = useChatStore((s) => s.setThinkingTime)
  const setUsageTimings = useChatStore((s) => s.setUsageTimings)
  const addToolCall = useChatStore((s) => s.addToolCall)
  const updateToolResult = useChatStore((s) => s.updateToolResult)

  const currentSessionId = useSessionStore((s) => s.currentSessionId)
  const config = useConfigStore((s) => s.config)
  const providers = useConfigStore((s) => s.providers)
  const reasoningEffort = useUiStore((s) => s.reasoningEffort)

  // 当前模型支持的思考级别（模型快照/配置），钳制 effort 防止切模型后失效
  const currentLevels = useMemo(() => {
    const p = providers.find((x) => x.id === config?.currentModel.provider)
    return p?.models.find((m) => m.id === config?.currentModel.model)
      ?.reasoningLevels
  }, [providers, config])

  /** 把用户选中的思考强度（clamp 到模型能力）放进模型选择 */
  const buildModel = useCallback(() => {
    const base = {
      provider: config?.currentModel.provider ?? "",
      model: config?.currentModel.model ?? "",
    }
    return { ...base, reasoningEffort: clampEffort(reasoningEffort, currentLevels) }
  }, [config, reasoningEffort, currentLevels])

  /** Stream one assistant reply into a fresh assistant message. */
  const streamReply = useCallback(
    async (input: string, attachments?: GuiAttachment[]) => {
      const sessionId = currentSessionId
      if (!sessionId || !config) return

      const messageId = `ai-${Date.now()}-${Math.random().toString(36).slice(2, 7)}`
      startAssistantMessage(sessionId, messageId)
      setStreaming(true)

      const streamStart = Date.now()
      let reasoningSeen = false
      let contentSeen = false

      try {
        const a = await getApi()
        await a.chat(
          {
            sessionId,
            messageId,
            input,
            model: buildModel(),
            temperature: config.temperature,
            maxTokens: config.maxTokens,
            ...(attachments && attachments.length > 0 ? { attachments } : {}),
          },
          (event) => {
            switch (event.type) {
              case "reasoning_delta":
                reasoningSeen = true
                appendReasoningDelta(sessionId, messageId, event.content)
                break
              case "delta":
                if (reasoningSeen && !contentSeen) {
                  // 思考结束、正文开始的时刻即"思考耗时"
                  setThinkingTime(sessionId, messageId, Date.now() - streamStart)
                }
                contentSeen = true
                appendDelta(sessionId, messageId, event.content)
                break
              case "finished":
                // 后端精确计时优先（timings.reasoningMs 覆盖前端估算的思考耗时）
                if (event.timings?.reasoningMs != null) {
                  setThinkingTime(sessionId, messageId, event.timings.reasoningMs)
                } else if (reasoningSeen) {
                  setThinkingTime(
                    sessionId,
                    messageId,
                    Date.now() - streamStart,
                  )
                }
                setUsageTimings(sessionId, messageId, event.usage, event.timings)
                finishMessage(sessionId, messageId)
                // 统一消息 id 为 m-{index}（与后端历史加载一致；反馈持久化等
                // 依赖 m-{idx} 格式的索引）
                if (!messageId.startsWith("m-")) {
                  const msgs = useChatStore.getState().getMessages(sessionId)
                  const idx = msgs.findIndex((m) => m.id === messageId)
                  if (idx >= 0) {
                    useChatStore
                      .getState()
                      .renameMessageId(sessionId, messageId, `m-${idx}`)
                  }
                }
                useSessionStore.getState().refreshSessions()
                break
              case "error":
                failMessage(sessionId, messageId, event.error ?? "unknown error")
                break
              case "tool_call":
                addToolCall(sessionId, event.toolId, event.toolName, event.toolArguments)
                break
              case "tool_result":
                updateToolResult(
                  sessionId,
                  event.toolId,
                  event.toolContent,
                  event.isError,
                  event.durationMs,
                )
                break
              case "cancelled":
                cancelMessage(sessionId, messageId)
                break
            }
          },
        )
      } catch (err) {
        failMessage(
          sessionId,
          messageId,
          err instanceof Error ? err.message : String(err),
        )
      }
    },
    [
      currentSessionId,
      config,
      buildModel,
      setStreaming,
      startAssistantMessage,
      appendDelta,
      appendReasoningDelta,
      setThinkingTime,
      setUsageTimings,
      addToolCall,
      updateToolResult,
      finishMessage,
      failMessage,
      cancelMessage,
    ],
  )

  /** Send a brand-new user message. */
  const send = useCallback(
    async (input: string, attachments?: GuiAttachment[]) => {
      if (!currentSessionId || !config || isStreaming) return
      addUserMessage(currentSessionId, input, attachments)
      await streamReply(input, attachments)
    },
    [currentSessionId, config, isStreaming, addUserMessage, streamReply],
  )

  /** Regenerate the reply for the user message preceding an assistant message. */
  const regenerate = useCallback(
    async (assistantMessageId: string) => {
      const sessionId = currentSessionId
      if (!sessionId || !config || isStreaming) return
      const messages = useChatStore.getState().getMessages(sessionId)
      const idx = messages.findIndex((m) => m.id === assistantMessageId)
      if (idx < 1) return
      const userMsg = messages[idx - 1]
      if (userMsg.role !== "user") return
      truncateFrom(sessionId, userMsg.id)
      await streamReply(userMsg.content)
    },
    [currentSessionId, config, isStreaming, truncateFrom, streamReply],
  )

  /** Edit a user message and re-stream the answer. */
  const editAndResend = useCallback(
    async (messageId: string, newContent: string) => {
      const sessionId = currentSessionId
      if (!sessionId || !config || isStreaming) return
      editUserMessage(sessionId, messageId, newContent)
      await streamReply(newContent)
    },
    [currentSessionId, config, isStreaming, editUserMessage, streamReply],
  )

  const cancel = useCallback(async () => {
    if (!currentSessionId) return
    const a = await getApi()
    await a.cancelChat(currentSessionId)
  }, [currentSessionId])

  return { send, regenerate, editAndResend, cancel, isStreaming }
}
