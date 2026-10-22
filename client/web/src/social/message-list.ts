import type { Conversation, Snapshot } from "./live-types"

export function isUnread(c: Conversation) { return c.listState.markedUnread || c.listState.unreadCount > 0 }
export function conversationName(c: Conversation, snapshot: Snapshot) {
  return c.kind === "group" ? c.title : snapshot.accounts.find(a => a.id !== snapshot.selfId && c.memberIds.includes(a.id))?.name ?? c.title
}
export function orderConversations(conversations: Conversation[]) {
  return [...conversations].sort((a, b) => Number(b.listState.pinned) - Number(a.listState.pinned)
    || (BigInt(a.listState.latestDelivery) === BigInt(b.listState.latestDelivery) ? 0 : BigInt(a.listState.latestDelivery) > BigInt(b.listState.latestDelivery) ? -1 : 1))
}
export function canAutoRead(focused: boolean, atLatest: boolean, loading: boolean, through: string | undefined, c: Conversation) {
  return focused && atLatest && !loading && through !== undefined
    && (c.listState.markedUnread || BigInt(through) > BigInt(c.listState.readThrough))
}
export function searchExcerpt(text: string, query: string) {
  const index = text.toLocaleLowerCase().indexOf(query.toLocaleLowerCase())
  const from = Math.max(0, index - 28)
  return `${from ? "…" : ""}${text.slice(from, from + 150)}${text.length > from + 150 ? "…" : ""}`
}
