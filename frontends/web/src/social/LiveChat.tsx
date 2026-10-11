import { useEffect, useRef, useState } from "react"
import { ArrowLeft, ArrowUp, Brain, CircleStop, Info, MessageCircle, Reply, X } from "lucide-react"
import { MarkdownView } from "@/components/chat/MarkdownView"
import { Avatar, IconButton, shortTime } from "./components/common"
import { socialClient } from "./client"
import { GroupMembers, MentionChoices } from "./GroupControls"
import { ConversationControls, OwnMessageControls } from "./LiveControls"
import { useLiveSocialStore } from "./live-store"
import { contactAvatar as avatar } from "./live-ui"
import { navigate } from "./router"
import type { Account, Conversation, ParticipationPlan } from "./live-types"
import { CUSTOM_WINDOW_CHROME } from "@/lib/desktop"
import { MemoryPanel, RememberMessage } from "./LiveMemory"

export function ConsciousnessInfo({ conversation }: { conversation?: Conversation }) {
  return conversation ? <MemoryPanel key={conversation.id} conversation={conversation} /> : <section className="live-consciousness"><Brain /><h2>意识与记忆</h2><p>先点击“发消息”打开这个 AI 的唯一私聊，再管理你们之间的记忆。</p></section>
}

export function Chat({ conversation, contact, details, memories }: { conversation: Conversation; contact?: Account; details: boolean; memories: boolean }) {
  const state = useLiveSocialStore()
  const snapshot = state.snapshot!
  const messages = state.histories[conversation.id]
  const draft = state.drafts[conversation.id]
  const text = draft?.text ?? ""
  const group = conversation.kind === "group"
  const title = group ? conversation.title : contact?.name ?? conversation.title
  const run = conversation.activeRun
  const timeline = useRef<HTMLDivElement>(null)
  const stick = useRef(true)
  const [plans, setPlans] = useState<ParticipationPlan[]>([])
  const contactArchived = contact?.status === "archived"
  const readOnly = conversation.status === "archived" || conversation.legacy
  useEffect(() => { stick.current = true; void state.action(() => state.loadHistory(conversation.id)) }, [conversation.id, state.action, state.loadHistory])
  useEffect(() => { if (stick.current && timeline.current) timeline.current.scrollTop = timeline.current.scrollHeight }, [messages, state.generations])
  useEffect(() => {
    if (!run) return
    const tick = setInterval(() => {
      const store = useLiveSocialStore.getState()
      if (!store.generations[run.messageId]) void store.action(async () => { await store.loadHistory(conversation.id); await store.loadDetails(run.id) })
    }, 500)
    return () => clearInterval(tick)
  }, [run?.id, conversation.id])
  useEffect(() => {
    if (!details || memories) return
    let current = true
    void state.action(async () => { const result = await socialClient.plans(conversation.id); if (current) setPlans(result) })
    return () => { current = false }
  }, [details, memories, conversation.pendingPlans, conversation.id, state.action])
  const reply = draft?.replyTo ? messages?.find(m => m.id === draft.replyTo) : undefined
  const name = (id: string) => snapshot.accounts.find(c => c.id === id)?.name ?? "AI"
  return <><section className="conversation-view" aria-label={`${title}聊天`}>
    <header className="conversation-header" data-tauri-drag-region={CUSTOM_WINDOW_CHROME ? "deep" : undefined}><IconButton label="返回消息列表" className="mobile-back" onClick={() => navigate("messages")}><ArrowLeft /></IconButton><button className="conversation-heading" onClick={() => group ? navigate("messages", conversation.id, true) : contact && navigate("contacts", contact.id)}><Avatar contact={avatar(contact)} group={group} /><span><strong>{title}{!group && <span className="ai-badge">AI</span>}</strong><small>{readOnly ? "历史记录 · 只读" : group ? `${conversation.memberIds.length} 位成员 · @ 选择接话对象` : contactArchived ? "账号已停用 · 聊天记录保留" : "长期私聊 · 聊天记录持续保存"}</small></span></button><IconButton label="聊天详情" aria-pressed={details} onClick={() => navigate("messages", conversation.id, !details)}><Info /></IconButton></header>
    <div className="chat-timeline" ref={timeline} onScroll={() => { const el = timeline.current!; stick.current = el.scrollHeight - el.scrollTop - el.clientHeight < 100 }}>
      {state.hasMore[conversation.id] && <button className="social-button secondary live-load-history" disabled={state.loadingHistory[conversation.id]} onClick={() => { stick.current = false; void state.action(() => state.loadHistory(conversation.id, true)) }}>{state.loadingHistory[conversation.id] ? "加载中…" : "加载更早的消息"}</button>}
      {!messages ? <div className="list-empty"><button disabled={state.loadingHistory[conversation.id]} onClick={() => void state.action(() => state.loadHistory(conversation.id))}>{state.loadingHistory[conversation.id] ? "正在读取聊天记录…" : "加载聊天记录"}</button></div> : !messages.length ? <div className="conversation-start"><span><MessageCircle />{group ? "在这里一起聊聊" : "每次相遇，都在同一个窗口"}</span><p>{group ? "普通消息会保存；@ 选择的 AI 会依次回复。" : "从第一条消息开始你们的日常。"}</p></div> : messages.map(message => {
        const own = message.senderId === snapshot.selfId
        const actor = snapshot.accounts.find(c => c.id === message.senderId)
        const generation = message.status === "generating" ? state.generations[message.id] : undefined
        const body = generation?.text ?? message.text
        const execution = message.runId ? state.details[message.runId] : undefined
        const quoted = message.replyTo ? messages.find(m => m.id === message.replyTo) : undefined
        return <article key={message.id} className={`live-message ${own ? "own" : ""}`}><Avatar self={own} contact={avatar(actor)} size="small" /><div className="live-message-content"><div className="live-message-label"><span>{message.senderName}{!own && <span className="ai-badge">AI</span>}</span><time>{shortTime(message.createdAt)}</time></div>{message.replyTo && <div className="live-quote">回复 {quoted?.senderName ?? "早前的消息"}{quoted && (quoted.status === "withdrawn" ? "：消息已撤回" : `：${quoted.text.slice(0, 80)}`)}</div>}{!!message.mentions.length && <div className="live-message-mentions">{message.mentions.map(id => `@${name(id)}`).join(" · ")}</div>}<div className="live-bubble">{message.status === "withdrawn" ? "消息已撤回" : body ? <MarkdownView content={body} /> : message.status === "generating" ? <span className="live-thinking">正在回复…</span> : "本次回复没有正文"}</div>{message.status === "generating" && <p className="live-generation-status" role="status"><span className="typing-dot" />{message.senderName} 正在回复</p>}{message.status === "interrupted" && <p className="live-interruption">{execution?.error?.message ?? "本次回复已中断，片段保留在记录中。"}</p>}{message.runId && <details className="live-run-details"><summary onClick={() => void state.action(() => state.loadDetails(message.runId!))}>回复详情</summary>{execution && <p>{execution.provider} / {execution.model}{execution.usage && ` · ${execution.usage.total_tokens} tokens`}</p>}{(generation?.reasoning || execution?.reasoning) && <pre>{generation?.reasoning ?? execution?.reasoning}</pre>}</details>}{own && !readOnly && message.status === "committed" && <OwnMessageControls message={message} />}{!readOnly && message.status === "committed" && <RememberMessage conversation={conversation} message={message} />}{!readOnly && message.status !== "withdrawn" && <button className="live-reply" aria-label={`回复 ${message.senderName} 的消息`} onClick={() => state.targetDraft(conversation.id, draft?.mentions ?? [], message.id)}><Reply />回复</button>}</div></article>
      })}
    </div>
    {readOnly ? <div className="archive-footer"><p>聊天记录保留在这个窗口中。</p></div> : <div className="composer-area">
      {(conversation.paused || (!group && (contactArchived || conversation.members.some(m => m.actorId === contact?.id && m.paused)))) && <div className="chat-status-notice">{contactArchived ? "账号已停用，消息仍会保存；管理员恢复账号后，只有新消息才会触发回复。" : conversation.paused || conversation.members.some(m => m.actorId === contact?.id && m.paused) ? "AI 已暂停接话，仍可以发送和保存消息。" : "AI 已暂停接话，仍可以发送和保存消息。"}</div>}
      {!!conversation.pendingPlans.length && <div className="live-queue" role="status">接话队列：{conversation.pendingPlans.map(p => `${name(p.actorId)}${p.status === "running" ? "（回复中）" : ""}`).join(" → ")}</div>}
      {draft?.replyTo && <div className="live-reply-target"><span>回复 {reply?.senderName ?? "早前的消息"}{reply && (reply.status === "withdrawn" ? "：消息已撤回" : `：${reply.text.slice(0, 60)}`)}</span><IconButton label="取消回复" onClick={() => state.targetDraft(conversation.id, draft.mentions ?? [], null)}><X /></IconButton></div>}
      {group && <MentionChoices conversation={conversation} />}
      <div className="composer"><textarea aria-label="消息输入框" rows={2} maxLength={20000} placeholder={`给 ${title} 发消息…`} value={text} onChange={event => state.updateDraft(conversation.id, event.target.value)} onKeyDown={event => { if (event.key === "Enter" && !event.shiftKey && !event.nativeEvent.isComposing && event.keyCode !== 229) { event.preventDefault(); void state.send(conversation.id) } }} /><div className="composer-bottom"><span className="composer-hint">{group ? "@ 或回复 AI 才接话" : run ? "正在回复，你可以继续发送消息" : "每条消息都会先保存"}</span><button className="send-button" aria-label="发送消息" disabled={state.sending[conversation.id] || !text.trim()} onClick={() => void state.send(conversation.id)}><ArrowUp /></button></div></div>
      <div className="composer-caption"><span>Enter 发送 · Shift + Enter 换行</span>{run && <button onClick={() => void state.action(() => socialClient.cancel(run.id))}><CircleStop />停止 {name(run.actorId)} 的当前回复</button>}</div>
    </div>}
  </section>{details && <aside className="details-panel" aria-label="聊天详情"><header data-tauri-drag-region={CUSTOM_WINDOW_CHROME ? "deep" : undefined}><strong>聊天详情</strong><IconButton label="关闭聊天详情" onClick={() => navigate("messages", conversation.id)}><X /></IconButton></header><div className="details-tabs"><button className={!memories ? "selected" : ""} onClick={() => navigate("messages", conversation.id, true)}>聊天</button><button className={memories ? "selected" : ""} onClick={() => navigate("messages", conversation.id, true, true)}><Brain />意识与记忆</button></div><div className="details-scroll">{memories ? <ConsciousnessInfo conversation={conversation} /> : <><div className="details-hero"><Avatar contact={avatar(contact)} group={group} size="large" /><h2>{title}</h2><p>{group ? "独立保存记录的群聊空间" : "一个持续积累的长期私聊"}</p></div><ConversationControls key={conversation.id} conversation={conversation} />{group && !readOnly && <GroupMembers conversation={conversation} />}<button className="details-memory-link" onClick={() => navigate("messages", conversation.id, true, true)}><Brain /><span>查看意识与上下文<small>近期记录、显式记忆与来源</small></span><span>→</span></button>{!group && contact && <button className="social-button secondary" onClick={() => navigate("contacts", contact.id)}>查看联系人资料</button>}<details className="live-plan-history"><summary>最近的接话记录</summary>{plans.length ? plans.map(p => <p key={p.id}>{name(p.actorId)} · 第 {p.triggerSequence} 条消息 · {{ queued: "排队中", running: "回复中", succeeded: "已完成", failed: "失败", cancelled: "已取消", interrupted: "已中断" }[p.status]}{p.failure && <small>{p.failure}</small>}</p>) : <p>还没有接话记录</p>}</details></>}</div></aside>}</>
}
