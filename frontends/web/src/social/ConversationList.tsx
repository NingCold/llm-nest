import { useEffect, useState } from "react"
import { CheckCheck, Mail, MoreHorizontal, Pin, PinOff } from "lucide-react"
import { Avatar, shortTime } from "./components/common"
import { socialClient } from "./client"
import { useLiveSocialStore } from "./live-store"
import { contactAvatar, conversationPreview } from "./live-ui"
import { conversationName, isUnread, orderConversations, searchExcerpt } from "./message-list"
import { navigate, navigateToMessage } from "./router"
import type { Conversation, MessageSearchPage, Snapshot } from "./live-types"

export function ConversationList({ query, selected }: { query: string; selected?: string }) {
  const state = useLiveSocialStore()
  const [unreadOnly, setUnreadOnly] = useState(false)
  const snapshot = state.snapshot
  if (!snapshot) return null
  const term = query.trim()
  const all = orderConversations(snapshot.conversations)
  const conversations = all.filter(c => (term || !unreadOnly || isUnread(c)) && (!term || `${conversationName(c, snapshot)} ${snapshot.accounts.filter(a => c.memberIds.includes(a.id)).map(a => a.name).join(" ")}`.toLocaleLowerCase().includes(term.toLocaleLowerCase())))
  const count = all.filter(isUnread).length
  return <>
    {!term && <div className="message-list-filters" aria-label="消息筛选"><button aria-pressed={!unreadOnly} onClick={() => setUnreadOnly(false)}>全部</button><button aria-pressed={unreadOnly} onClick={() => setUnreadOnly(true)}>未读{count > 0 && ` ${count}`}</button></div>}
    <div className="social-list">
      {term && <p className="search-section-label">聊天</p>}
      {conversations.map(c => <ConversationRow key={c.id} conversation={c} snapshot={snapshot} selected={selected === c.id} />)}
      {!conversations.length && <p className="list-empty">{term ? "没有同名聊天" : unreadOnly ? "消息都已读完" : "从认识一位朋友开始"}</p>}
      {term && <MessageSearch key={`${term}/${selected ?? ""}`} query={term} selected={selected} snapshot={snapshot} />}
    </div>
  </>
}

function ConversationRow({ conversation: c, snapshot, selected }: { conversation: Conversation; snapshot: Snapshot; selected: boolean }) {
  const state = useLiveSocialStore()
  const [busy, setBusy] = useState(false)
  const actor = snapshot.accounts.find(a => a.id !== snapshot.selfId && c.memberIds.includes(a.id))
  const name = conversationName(c, snapshot), unread = isUnread(c)
  async function change(action: import("./live-types").ConversationListAction) {
    if (action.kind === "unread" && selected) navigate("messages")
    setBusy(true)
    try { await state.action(() => state.updateList(c.id, action)) } finally { setBusy(false) }
  }
  return <div className={`conversation-row ${selected ? "selected" : ""} ${c.listState.pinned ? "pinned" : ""}`}>
    <button className="conversation-list-item" aria-current={selected ? "page" : undefined} onClick={() => navigate("messages", c.id)}>
      <Avatar contact={contactAvatar(actor)} group={c.kind === "group"} />
      <span className="conversation-list-text"><span><strong>{name}</strong><time>{c.latestMessage && shortTime(c.latestMessage.createdAt)}</time></span><span><small>{(state.drafts[c.id]?.text || state.drafts[c.id]?.images?.length) ? <><span className="list-draft-label">草稿</span>{state.drafts[c.id].text || "[图片]"}</> : <>{c.kind === "direct" && actor?.status === "archived" && "账号已停用 · "}{conversationPreview(c)}</>}</small>{unread && <span className={`unread-badge ${c.listState.unreadCount ? "" : "dot"}`} aria-label={c.listState.unreadCount ? `${c.listState.unreadCount} 条未读` : "已标记未读"}>{c.listState.unreadCount ? c.listState.unreadCount > 99 ? "99+" : c.listState.unreadCount : ""}</span>}</span></span>
    </button>
    <div className="conversation-row-tools" data-tauri-drag-region="false">{c.listState.pinned && <Pin className="pin-indicator" aria-label="已置顶" />}
      <details className="conversation-menu" onToggle={e => {
        const menu = e.currentTarget
        if (menu.open) { menu.dataset.up = "false"; menu.dataset.up = String(menu.querySelector("div")!.getBoundingClientRect().bottom > menu.closest(".social-list")!.getBoundingClientRect().bottom) }
      }} onKeyDown={e => { if (e.key === "Escape") { e.currentTarget.open = false; e.currentTarget.querySelector("summary")!.focus() } }} onBlur={e => { if (!e.currentTarget.contains(e.relatedTarget)) e.currentTarget.open = false }}>
        <summary aria-label={`${name} 的消息选项`}><MoreHorizontal /></summary>
        <div><button disabled={busy} onClick={e => { e.currentTarget.closest("details")!.open = false; void change({ kind: "pin", pinned: !c.listState.pinned }) }}>{c.listState.pinned ? <PinOff /> : <Pin />}{c.listState.pinned ? "取消置顶" : "置顶聊天"}</button>
          <button disabled={busy} onClick={e => { e.currentTarget.closest("details")!.open = false; void change(unread ? { kind: "read", through: c.listState.latestDelivery } : { kind: "unread" }) }}>{unread ? <CheckCheck /> : <Mail />}{unread ? "标为已读" : "标为未读"}</button></div>
      </details>
    </div>
  </div>
}

function MessageSearch({ query, selected, snapshot }: { query: string; selected?: string; snapshot: Snapshot }) {
  const [scope, setScope] = useState("")
  return <section className="message-search-results" aria-label="正文搜索结果">
    <div className="search-section-label"><span>聊天记录</span>{selected && <select aria-label="搜索范围" value={scope} onChange={e => setScope(e.target.value)}><option value="">全部聊天</option><option value={selected}>当前聊天</option></select>}</div>
    <SearchResults key={`${query}/${scope}/${snapshot.cursor}`} query={query} scope={scope} snapshot={snapshot} />
  </section>
}

function SearchResults({ query, scope, snapshot }: { query: string; scope: string; snapshot: Snapshot }) {
  const [page, setPage] = useState<MessageSearchPage | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [loading, setLoading] = useState(true)
  const [before, setBefore] = useState<string | null>(null)
  const [retry, setRetry] = useState(0)
  useEffect(() => {
    let active = true
    setError(null); setLoading(true)
    const timer = setTimeout(() => {
      socialClient.searchMessages({ query, conversationId: scope || null, before }).then(result => {
        if (active) setPage(old => ({ ...result, messages: before ? [...(old?.messages ?? []), ...result.messages] : result.messages }))
      }, err => { if (active) setError(String(err)) }).finally(() => { if (active) setLoading(false) })
    }, before ? 0 : 250)
    return () => { active = false; clearTimeout(timer) }
  }, [query, scope, before, retry])
  return <>
    {error && <p role="alert" className="list-empty">{error}<button onClick={() => setRetry(n => n + 1)}>重试搜索</button></p>}
    {page?.messages.map(m => <button key={m.id} className="message-search-hit" onClick={() => navigateToMessage(m.conversationId, m.id)}><span><strong>{snapshot.conversations.find(c => c.id === m.conversationId) && conversationName(snapshot.conversations.find(c => c.id === m.conversationId)!, snapshot)}</strong><time>{shortTime(m.createdAt)}</time></span><small>{m.senderName}</small><p>{searchExcerpt(m.text, query)}</p></button>)}
    {loading && <p className="list-empty" role="status">正在搜索…</p>}
    {!loading && !error && page && !page.messages.length && <p className="list-empty">没有找到包含这段文字的消息</p>}
    {page?.nextBefore && !error && <button className="social-button secondary search-more" disabled={loading} onClick={() => setBefore(page.nextBefore)}>更多结果</button>}
  </>
}
