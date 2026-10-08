import { useEffect, useRef, useState } from "react"
import { ArrowLeft, ArrowUp, Check, ChevronDown, CircleStop, Info, MessageCircle, MoreHorizontal, Pause, Pencil, Play, Reply, Sparkles, Trash2, X, Brain } from "lucide-react"
import { useSocialStore } from "../store"
import { navigate } from "../router"
import { memoryScope } from "../types"
import type { Conversation, SocialMessage } from "../types"
import type { Editor } from "./EditorDialogs"
import { Avatar, IconButton, shortTime } from "./common"

export function ConversationView({ conversation, details, edit }: { conversation: Conversation; details: boolean; edit(editor: Editor): void }) {
  const snapshot = useSocialStore(s => s.snapshot!)
  const action = useSocialStore(s => s.action)
  const draft = useSocialStore(s => s.drafts[conversation.id])
  const sending = useSocialStore(s => !!s.sending[conversation.id])
  const updateDraft = useSocialStore(s => s.updateDraft)
  const send = useSocialStore(s => s.send)
  const members = snapshot.contacts.filter(c => conversation.memberIds.includes(c.id))
  const contact = conversation.kind === "direct" ? members[0] : undefined
  const messages = snapshot.messages.filter(m => m.conversationId === conversation.id)
  const run = snapshot.runs.find(r => r.conversationId === conversation.id && r.status === "running")
  const [menu, setMenu] = useState<string | null>(null)
  const [showMentions, setShowMentions] = useState(false)
  const [nearBottom, setNearBottom] = useState(true)
  const scroll = useRef<HTMLDivElement>(null)
  const textarea = useRef<HTMLTextAreaElement>(null)
  const stick = useRef(true)
  const archived = conversation.status === "archived"

  useEffect(() => {
    if (conversation.unread) void action(api => api.markRead(conversation.id))
  }, [conversation.id, conversation.unread, action])
  useEffect(() => {
    stick.current = true; setNearBottom(true); setMenu(null); setShowMentions(false)
    if (scroll.current) scroll.current.scrollTop = scroll.current.scrollHeight
  }, [conversation.id])
  useEffect(() => {
    if (stick.current && scroll.current) scroll.current.scrollTop = scroll.current.scrollHeight
  }, [messages.length, messages[messages.length - 1]?.text, conversation.id])
  useEffect(() => {
    if (textarea.current) { textarea.current.style.height = "auto"; textarea.current.style.height = `${Math.min(144, textarea.current.scrollHeight)}px` }
  }, [draft?.text, conversation.id])
  useEffect(() => {
    const close = (event: KeyboardEvent) => { if (event.key === "Escape") { setMenu(null); setShowMentions(false) } }
    window.addEventListener("keydown", close)
    return () => window.removeEventListener("keydown", close)
  }, [])

  const sourceText = (message?: SocialMessage) => message?.status === "complete" ? message.text : "来源已变化或不可用"
  const reply = (message: SocialMessage) => {
    const mentions = conversation.kind === "group" && message.senderId !== snapshot.self.id ? [...new Set([...(draft?.mentions ?? []), message.senderId])] : draft?.mentions ?? []
    updateDraft(conversation.id, { replyTo: message.id, mentions }); setMenu(null); textarea.current?.focus()
  }
  const continuePrivate = async (message: SocialMessage, actorId: string) => {
    const direct = await action(api => api.getDirectConversation(actorId))
    if (!direct) return
    const previous = useSocialStore.getState().drafts[direct.id]
    updateDraft(direct.id, { contextReferences: [{ conversationId: conversation.id, messageId: message.id, messageRevision: message.revision, actorId }], text: previous?.text || "我们接着群里这个话题聊聊。" })
    setMenu(null); navigate("messages", direct.id)
  }
  const sendDraft = () => { stick.current = true; setNearBottom(true); void send(conversation.id); textarea.current?.focus() }

  return <section className="conversation-view" aria-label={`${conversation.title}聊天`}>
    <header className="conversation-header">
      <IconButton label="返回消息列表" className="mobile-back" onClick={() => navigate("messages")}><ArrowLeft /></IconButton>
      <button className="conversation-heading" onClick={() => contact ? navigate("contacts", contact.id) : navigate("messages", conversation.id, true)}>
        <Avatar contact={contact} group={conversation.kind === "group"} />
        <span><strong>{conversation.title}{conversation.kind === "direct" && <span className="ai-badge">AI</span>}</strong><small>{archived ? "已归档 · 只读" : conversation.paused ? "已暂停接话" : conversation.kind === "group" ? `${conversation.memberIds.length} 位成员 · @ 或回复时接话` : contact?.execution === "preview" ? "长期私聊 · 模拟回复" : "长期私聊 · 尚未配置回复"}</small></span>
      </button>
      <div className="header-actions">
        {!archived && <IconButton label={conversation.paused ? "恢复 AI 接话" : "暂停 AI 接话"} onClick={() => void action(api => api.setPaused(conversation.id, !conversation.paused))}>{conversation.paused ? <Play /> : <Pause />}</IconButton>}
        <IconButton label="聊天详情" aria-pressed={details} onClick={() => navigate("messages", conversation.id, !details)}><Info /></IconButton>
      </div>
    </header>
    <div className="chat-timeline" ref={scroll} onScroll={() => {
      const el = scroll.current!; const near = el.scrollHeight - el.scrollTop - el.clientHeight < 100
      stick.current = near; setNearBottom(near)
    }} onClick={event => { if (!(event.target as HTMLElement).closest(".message-menu, .message-options")) setMenu(null) }}>
      <div className="conversation-start"><span><MessageCircle />{conversation.kind === "direct" ? "每次相遇，都在同一个窗口" : "共同的话题，从这里开始"}</span><p>{conversation.kind === "direct" ? "聊天记录持续保存，记忆可在详情中管理。" : "每个群都有独立的聊天记录与共同记忆。"}</p></div>
      {messages.map((message, index) => {
        const own = message.senderId === snapshot.self.id
        const actor = snapshot.contacts.find(c => c.id === message.senderId)
        const previous = messages[index - 1]
        const day = new Date(message.createdAt).toLocaleDateString("zh-CN", { month: "long", day: "numeric" })
        const showDay = !previous || new Date(previous.createdAt).toDateString() !== new Date(message.createdAt).toDateString()
        const replyMessage = message.replyTo ? snapshot.messages.find(m => m.id === message.replyTo) : undefined
        return <div key={message.id} id={`message-${message.id}`}>
          {showDay && <div className="day-divider"><span>{day}</span></div>}
          <article className={`message-row ${own ? "own" : ""}`}>
            <button className="message-avatar" aria-label={own ? "我的头像" : `查看${actor?.name ?? message.senderName}的资料`} disabled={own} onClick={() => actor && navigate("contacts", actor.id)}><Avatar contact={actor} self={own} size="small" /></button>
            <div className="message-body">
              <div className="message-meta"><span>{own ? "我" : message.senderName}</span>{!own && <span className="ai-badge">AI</span>}<time dateTime={message.createdAt}>{shortTime(message.createdAt)}</time></div>
              {message.status === "redacted" ? <div className="redacted-message">这条消息已撤回</div> : <div className={`message-bubble ${message.status === "generating" ? "generating" : ""}`}>
                {own && replyMessage && <button className="reply-quote" onClick={() => document.getElementById(`message-${replyMessage.id}`)?.scrollIntoView({ block: "center", behavior: "smooth" })}><Reply /><span><strong>{replyMessage.senderName}</strong>{sourceText(replyMessage).slice(0, 90)}</span></button>}
                {message.contextReferences.map(ref => {
                  const source = snapshot.messages.find(m => m.id === ref.messageId)
                  const valid = source?.revision === ref.messageRevision && source.status === "complete"
                  return <button className="reply-quote" key={ref.messageId} onClick={() => navigate("messages", ref.conversationId)}><MessageCircle /><span><strong>来自 {snapshot.conversations.find(c => c.id === ref.conversationId)?.title}</strong>{valid ? source.text.slice(0, 90) : "引用来源已变化"}</span></button>
                })}
                {!!message.mentions.length && <div className="message-mentions">{message.mentions.map(id => <span key={id}>@{snapshot.contacts.find(c => c.id === id)?.name}</span>)}</div>}
                <p>{message.text || (message.status === "generating" ? "正在组织回复…" : "回复尚未生成")}</p>
              </div>}
              {message.status === "cancelled" || message.status === "interrupted" ? <small className="message-state">{message.status === "cancelled" ? "回复已停止" : "回复已中断"} · 保留已生成部分</small> : message.status === "generating" ? <small className="message-state"><span className="typing-dot" />模拟回复中</small> : own && message.status === "complete" ? <small className="message-state"><Check />已保存{message.revision !== "1" && " · 已编辑"}</small> : null}
            </div>
            {!archived && message.status === "complete" && <div className="message-options"><IconButton label={`操作${own ? "我" : message.senderName}的消息`} aria-expanded={menu === message.id} onClick={() => setMenu(menu === message.id ? null : message.id)}><MoreHorizontal /></IconButton>
              {menu === message.id && <div className={`message-menu ${index >= messages.length - 2 ? "menu-up" : ""}`}>
                <button onClick={() => reply(message)}><Reply />回复</button>
                <button onClick={() => { edit({ kind: "memory", scope: memoryScope(conversation), source: message }); setMenu(null) }}><Brain />记住</button>
                {own && <><button onClick={() => { edit({ kind: "message", message }); setMenu(null) }}><Pencil />编辑</button><button className="danger-text" onClick={() => { setMenu(null); edit({ kind: "confirm", title: "撤回这条消息？", description: "移除消息原文，并将依赖它的记忆和摘要标记为待更新。", confirm: async () => !!await action(async api => { await api.redactMessage(message.id, message.revision); return true }) }) }}><Trash2 />撤回</button></>}
                {conversation.kind === "group" && <><span className="menu-label">在私聊中继续</span>{members.map(member => <button key={member.id} onClick={() => void continuePrivate(message, member.id)}><Avatar contact={member} size="small" />与 {member.name} 继续</button>)}</>}
              </div>}
            </div>}
          </article>
        </div>
      })}
    </div>
    {!nearBottom && <button className="new-message-button" onClick={() => { stick.current = true; setNearBottom(true); scroll.current?.scrollTo({ top: scroll.current.scrollHeight, behavior: "smooth" }) }}><ChevronDown />回到最新消息</button>}
    {archived ? <div className="archive-footer"><p>聊天已归档，记录和记忆仍然保留。</p><button className="social-button primary" onClick={() => void action(api => api.updateConversation(conversation.id, { status: "active" }))}>恢复这个聊天</button></div> : <div className="composer-area">
      {(conversation.paused || conversation.bout === "ended" || contact?.execution === "unconfigured") && <div className="chat-status-notice">{conversation.paused ? "AI 已暂停接话，仍可以发送和保存消息。恢复后从新消息开始。" : contact?.execution === "unconfigured" ? "该 AI 尚未配置回复，发送的消息仍会保存。可在联系人资料中调整回复配置。" : "这轮聊天已结束。发送新消息可以开启下一轮。"}</div>}
      <div className="composer">
        {draft?.replyTo && <div className="draft-reference"><Reply /><span>回复 {snapshot.messages.find(m => m.id === draft.replyTo)?.senderName}：{sourceText(snapshot.messages.find(m => m.id === draft.replyTo)).slice(0, 60)}</span><IconButton label="取消引用回复" onClick={() => updateDraft(conversation.id, { replyTo: null })}><X /></IconButton></div>}
        {draft?.contextReferences.map(ref => <div className="draft-reference" key={ref.messageId}><MessageCircle /><span>接续「{snapshot.conversations.find(c => c.id === ref.conversationId)?.title}」的话题 · 仅引用共同可见消息</span><IconButton label="移除群聊引用" onClick={() => updateDraft(conversation.id, { contextReferences: [] })}><X /></IconButton></div>)}
        <textarea ref={textarea} aria-label="消息输入框" rows={2} maxLength={20_000} placeholder={conversation.kind === "group" ? "聊点什么，@ 一位朋友来接话…" : `给 ${conversation.title} 发消息…`} value={draft?.text ?? ""} onChange={event => updateDraft(conversation.id, { text: event.target.value })} onKeyDown={event => {
          if (event.key === "Enter" && !event.shiftKey && !event.nativeEvent.isComposing && event.keyCode !== 229) { event.preventDefault(); if (!sending && draft?.text.trim()) sendDraft() }
        }} />
        <div className="composer-bottom"><div className="composer-tools">
          {conversation.kind === "group" ? <div className="mention-control"><button className="text-button" aria-expanded={showMentions} onClick={() => setShowMentions(!showMentions)}>＠ 邀请接话</button>{showMentions && <div className="mention-menu"><p>选择接话的朋友</p>{members.map(member => <button key={member.id} aria-pressed={draft?.mentions.includes(member.id) ?? false} onClick={() => {
            const mentions = draft?.mentions ?? []; updateDraft(conversation.id, { mentions: mentions.includes(member.id) ? mentions.filter(id => id !== member.id) : [...mentions, member.id] })
          }}><Avatar contact={member} size="small" /><span>{member.name}</span>{draft?.mentions.includes(member.id) && <Check />}</button>)}</div>}</div> : <span className="composer-hint"><Sparkles />模拟回复</span>}
          {draft?.mentions.map(id => <button key={id} className="mention-chip" onClick={() => updateDraft(conversation.id, { mentions: draft.mentions.filter(item => item !== id) })}>@{snapshot.contacts.find(c => c.id === id)?.name}<X /></button>)}
        </div><button className="send-button" aria-label="发送消息" disabled={sending || !draft?.text.trim()} onClick={sendDraft}><ArrowUp /></button></div>
      </div>
      <div className="composer-caption"><span>Enter 发送 · Shift + Enter 换行</span><div>{run && <button onClick={() => void action(api => api.cancelRun(run.id))}><CircleStop />停止这次回复</button>}<button disabled={conversation.bout === "ended"} onClick={() => void action(api => api.endBout(conversation.id))}>结束这轮聊天</button></div></div>
    </div>}
  </section>
}
