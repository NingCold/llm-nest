import { useCallback } from "react"
import { useChatStore } from "@/store/chat"
import { useSessionStore } from "@/store/session"
import { useConfigStore } from "@/store/config"

async function getApi() {
  const mod = await import("@/api")
  return mod.getApi()
}

export function useChat() {
  const isStreaming = useChatStore((s) => s.isStreaming)
  const setStreaming = useChatStore((s) => s.setStreaming)
  const startAssistantMessage = useChatStore((s) => s.startAssistantMessage)
  const appendDelta = useChatStore((s) => s.appendDelta)
  const finishMessage = useChatStore((s) => s.finishMessage)
  const failMessage = useChatStore((s) => s.failMessage)
  const cancelMessage = useChatStore((s) => s.cancelMessage)
  const addUserMessage = useChatStore((s) => s.addUserMessage)

  const currentSessionId = useSessionStore((s) => s.currentSessionId)
  const config = useConfigStore((s) => s.config)

  const send = useCallback(
    async (input: string) => {
      if (!currentSessionId || !config || isStreaming) return

      addUserMessage(currentSessionId, input)
      setStreaming(true)

      const messageId = `ai-${Date.now()}`
      startAssistantMessage(currentSessionId, messageId)

      try {
        const a = await getApi()
        await a.chat(
          {
            sessionId: currentSessionId,
            input,
            model: config.currentModel,
            temperature: config.temperature,
            maxTokens: config.maxTokens,
          },
          (event) => {
            switch (event.type) {
              case "delta":
                appendDelta(currentSessionId, messageId, event.content ?? "")
                break
              case "finished":
                finishMessage(currentSessionId, messageId)
                break
              case "error":
                failMessage(currentSessionId, messageId, event.error ?? "unknown error")
                break
              case "cancelled":
                cancelMessage(currentSessionId, messageId)
                break
            }
          },
        )
      } catch (err) {
        failMessage(
          currentSessionId,
          messageId,
          err instanceof Error ? err.message : String(err),
        )
      }
    },
    [
      currentSessionId,
      config,
      isStreaming,
      addUserMessage,
      setStreaming,
      startAssistantMessage,
      appendDelta,
      finishMessage,
      failMessage,
      cancelMessage,
    ],
  )

  const cancel = useCallback(async () => {
    if (!currentSessionId) return
    const a = await getApi()
    await a.cancelChat(currentSessionId)
  }, [currentSessionId])

  return { send, cancel, isStreaming }
}