import { Archive, Brain, MessageCircle, Pin, Users, X } from "lucide-react"
import { navigate } from "../router"
import { useSocialStore } from "../store"
import type { Conversation } from "../types"
import type { Editor } from "./EditorDialogs"
import { Avatar, IconButton } from "./common"
import { MemoryPanel } from "./MemoryPanel"

export function DetailsPanel({ conversation, memories, edit }: { conversation: Conversation; memories: boolean; edit(editor: Editor): void }) {
  const snapshot = useSocialStore(s => s.snapshot!)
  const action = useSocialStore(s => s.action)
  const members = snapshot.contacts.filter(c => conversation.memberIds.includes(c.id))
  return <aside className="details-panel" aria-label="聊天详情">
    <header><strong>聊天详情</strong><IconButton label="关闭聊天详情" onClick={() => navigate("messages", conversation.id)}><X /></IconButton></header>
    <div className="details-tabs"><button className={!memories ? "selected" : ""} onClick={() => navigate("messages", conversation.id, true)}>聊天</button><button className={memories ? "selected" : ""} onClick={() => navigate("messages", conversation.id, true, true)}><Brain />意识与记忆</button></div>
    <div className="details-scroll">{memories ? <MemoryPanel conversation={conversation} edit={edit} /> : <>
      <div className="details-hero"><Avatar contact={conversation.kind === "direct" ? members[0] : undefined} group={conversation.kind === "group"} size="large" /><h2>{conversation.title}</h2><p>{conversation.kind === "group" ? "一个独立的聊天空间" : "一个持续积累的长期私聊"}</p></div>
      <div className="section-title"><span><Users />{conversation.kind === "group" ? "群成员" : "聊天成员"}</span><small>{conversation.memberIds.length} 位</small></div>
      <div className="member-list"><div><Avatar self size="small" /><span>我</span><small>本地用户</small></div>{members.map(member => <button key={member.id} onClick={() => navigate("contacts", member.id)}><Avatar contact={member} size="small" /><span>{member.name}</span><span className="ai-badge">AI</span></button>)}</div>
      {conversation.kind === "group" && <div className="details-note"><MessageCircle /><p>AI 只在被 @、回复或邀请时接话。群聊内容可以引用到成员私聊中继续，后续私聊保留在私聊窗口。</p></div>}
      <div className="details-controls"><label><span><Pin />置顶聊天</span><input type="checkbox" checked={conversation.pinned} onChange={e => void action(api => api.updateConversation(conversation.id, { pinned: e.target.checked }))} /></label><label><span>暂停 AI 接话</span><input type="checkbox" checked={conversation.paused} disabled={conversation.status === "archived"} onChange={e => void action(api => api.setPaused(conversation.id, e.target.checked))} /></label></div>
      <button className="details-memory-link" onClick={() => navigate("messages", conversation.id, true, true)}><Brain /><span>管理意识与记忆<small>共同记忆、历史摘要与来源</small></span><span>→</span></button>
      <button className="social-button archive-action" onClick={() => {
        if (conversation.status === "archived") void action(api => api.updateConversation(conversation.id, { status: "active" }))
        else edit({ kind: "confirm", title: "归档这个聊天？", description: "聊天将变为只读，当前回复会停止。历史、记忆与窗口身份保留，恢复后可以继续使用。", confirm: async () => !!await action(async api => { await api.updateConversation(conversation.id, { status: "archived" }); return true }) })
      }}><Archive />{conversation.status === "archived" ? "恢复聊天" : "归档聊天"}</button>
    </> }</div>
  </aside>
}
