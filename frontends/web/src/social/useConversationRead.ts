import { useEffect, useState } from "react"
import { useLiveSocialStore } from "./live-store"
import { canAutoRead } from "./message-list"
import type { Conversation } from "./live-types"

export function useConversationRead(conversation: Conversation, atLatest: boolean) {
  const state = useLiveSocialStore()
  const [focused, setFocused] = useState(() => document.visibilityState === "visible" && document.hasFocus())
  useEffect(() => {
    const update = () => setFocused(document.visibilityState === "visible" && document.hasFocus())
    document.addEventListener("visibilitychange", update); window.addEventListener("focus", update); window.addEventListener("blur", update)
    return () => { document.removeEventListener("visibilitychange", update); window.removeEventListener("focus", update); window.removeEventListener("blur", update) }
  }, [])
  const through = state.historyReadThrough[conversation.id]
  const loading = state.loadingHistory[conversation.id]
  useEffect(() => {
    if (!canAutoRead(focused, atLatest, loading, through, conversation)) return
    const timer = setTimeout(() => { void state.action(() => state.updateList(conversation.id, { kind: "read", through: through! })) }, 500)
    return () => clearTimeout(timer)
  }, [focused, atLatest, loading, through, conversation.id, conversation.listState.revision, state.action, state.updateList])
}
