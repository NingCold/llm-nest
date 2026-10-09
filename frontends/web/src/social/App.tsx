import { useEffect, useRef, useState, type FormEvent } from "react"
import { ArrowLeft, ArrowRight, ArrowUp, Brain, CircleStop, Info, MessageCircle, Moon, Pencil, Plus, Search, Settings, Sun, Users, X } from "lucide-react"
import { BrandMark } from "@/components/BrandMark"
import { SettingsPage } from "@/components/settings/SettingsPage"
import { MarkdownView } from "@/components/chat/MarkdownView"
import { Dialog, DialogContent, DialogDescription, DialogTitle } from "@/components/ui/dialog"
import { useInit } from "@/hooks/useInit"
import { useUiStore } from "@/store/ui"
import { useConfigStore } from "@/store/config"
import { Avatar, EmptyState, IconButton, shortTime } from "./components/common"
import { navigate, parseSocialRoute } from "./router"
import { socialClient } from "./client"
import { useLiveSocialStore } from "./live-store"
import type { Contact, Conversation, ModelOption } from "./live-types"
import "./social.css"

function avatar(contact?: Contact) {
  return contact ? { avatar: contact.name.slice(0, 1), color: (["sage", "clay", "blue", "lavender"] as const)[parseInt(contact.id[0], 16) % 4] } : undefined
}

function SocialSettings({ onBack }: { onBack(): void }) {
  const { ready, error, retry } = useInit()
  const saveError = useConfigStore(s => s.saveError)
  const saving = useConfigStore(s => s.saving)
  const retrySave = useConfigStore(s => s.retrySave)
  return <div className="social-settings">{error ? <div className="social-startup"><p role="alert">{error}</p><button className="social-button secondary" onClick={retry}>重试</button></div> : ready ? <SettingsPage onBack={onBack} availableSections={["models", "appearance", "config", "about"]} notices={saveError ? <div role="alert" className="social-error"><span>{saveError}</span><button disabled={saving} onClick={retrySave}>重试保存</button></div> : undefined} /> : <div className="social-startup">正在加载设置…</div>}</div>
}

function ContactEditor({ contact, models, close }: { contact?: Contact; models: ModelOption[]; close(): void }) {
  const action = useLiveSocialStore(s => s.action), refresh = useLiveSocialStore(s => s.refresh)
  const [name, setName] = useState(contact?.name ?? "")
  const [persona, setPersona] = useState(contact?.persona ?? "")
  const [model, setModel] = useState(contact?.binding ? `${contact.binding.providerId}/${contact.binding.modelId}` : "")
  const [effort, setEffort] = useState(contact?.binding?.reasoningEffort ?? "")
  const [saving, setSaving] = useState(false)
  const requestId = useRef(crypto.randomUUID())
  const selected = models.find(m => `${m.providerId}/${m.modelId}` === model)
  const save = async (event: FormEvent) => {
    event.preventDefault(); setSaving(true)
    const binding = selected ? { providerId: selected.providerId, modelId: selected.modelId, reasoningEffort: effort || null } : null
    const input = { requestId: requestId.current, name, persona, binding }
    const result = await action(() => contact ? socialClient.updateContact(contact.id, { ...input, expectedRevision: contact.revision }) : socialClient.createContact(input))
    if (result) { await action(refresh); navigate("contacts", result.id); close() }
    setSaving(false)
  }
  return <form className="social-form" onSubmit={event => void save(event)}><fieldset disabled={saving} className="social-form-fields">
    <label>名字<input autoFocus required maxLength={40} value={name} onChange={e => { setName(e.target.value); requestId.current = crypto.randomUUID() }} placeholder="你想怎么称呼这位朋友？" /></label>
    <label>角色设定<textarea required maxLength={2000} rows={5} value={persona} onChange={e => { setPersona(e.target.value); requestId.current = crypto.randomUUID() }} placeholder="描述性格、背景，以及你希望如何相处。" /></label>
    <label>回复模型<select value={model} onChange={e => { setModel(e.target.value); setEffort(""); requestId.current = crypto.randomUUID() }}><option value="">暂不配置</option>{models.map(m => <option key={`${m.providerId}/${m.modelId}`} value={`${m.providerId}/${m.modelId}`}>{m.displayName} · {m.providerId}</option>)}{model && !selected && <option value={model}>当前模型已不可用，请重新选择</option>}</select></label>
    {!!selected?.reasoningLevels.length && <label>思考强度<select value={effort} onChange={e => { setEffort(e.target.value); requestId.current = crypto.randomUUID() }}><option value="">使用模型默认值</option>{selected.reasoningLevels.map(level => <option key={level} value={level}>{level}</option>)}</select></label>}
    <p className="form-note">身份与模型独立保存。更换模型后，仍使用原来的私聊窗口和聊天记录。{!models.length && "请先在设置中添加模型服务。"}</p>
    <div className="dialog-actions"><button type="button" className="social-button secondary" onClick={close}>取消</button><button className="social-button primary" disabled={!!model && !selected}>{saving ? "保存中…" : contact ? "保存修改" : "添加联系人"}</button></div>
  </fieldset></form>
}

function ConsciousnessInfo({ conversation }: { conversation?: Conversation }) {
  return <section className="live-consciousness"><Brain /><h2>意识与上下文</h2><p>聊天记录完整保存。每次回复时，软件从这个窗口选择近期记录，并结合角色设定准备上下文。</p>{conversation && <p>这个窗口已保存 {conversation.lastSequence} 条消息。</p>}<p>当前阶段使用近期原文；历史摘要、长期记忆和群聊话题接续将在后续接入。</p></section>
}

function Chat({ conversation, contact, details, memories }: { conversation: Conversation; contact?: Contact; details: boolean; memories: boolean }) {
  const state = useLiveSocialStore()
  const messages = state.histories[conversation.id]
  const draft = state.drafts[conversation.id]?.text ?? ""
  const run = conversation.activeRun
  const timeline = useRef<HTMLDivElement>(null)
  const stick = useRef(true)
  useEffect(() => {
    stick.current = true
    void state.action(() => state.loadHistory(conversation.id))
  }, [conversation.id, state.action, state.loadHistory])
  useEffect(() => {
    if (stick.current && timeline.current) timeline.current.scrollTop = timeline.current.scrollHeight
  }, [messages, state.generations])
  useEffect(() => {
    if (!run) return
    const tick = setInterval(() => {
      const store = useLiveSocialStore.getState()
      // On reconnect, show persisted drafts instead of joining an incomplete token stream.
      if (!store.generations[run.messageId]) void store.action(async () => { await store.loadHistory(conversation.id); await store.loadDetails(run.id) })
    }, 500)
    return () => clearInterval(tick)
  }, [run?.id, conversation.id])
  const readOnly = conversation.status === "archived" || conversation.legacy || conversation.kind !== "direct"
  return <><section className="conversation-view" aria-label={`${contact?.name ?? conversation.title}聊天`}>
    <header className="conversation-header"><IconButton label="返回消息列表" className="mobile-back" onClick={() => navigate("messages")}><ArrowLeft /></IconButton><button className="conversation-heading" onClick={() => contact && navigate("contacts", contact.id)}><Avatar contact={avatar(contact)} group={conversation.kind === "group"} /><span><strong>{contact?.name ?? conversation.title}<span className="ai-badge">AI</span></strong><small>{readOnly ? "历史记录 · 只读" : "长期私聊 · 聊天记录持续保存"}</small></span></button><div className="header-actions"><IconButton label="聊天详情" aria-pressed={details} onClick={() => navigate("messages", conversation.id, !details)}><Info /></IconButton></div></header>
    <div className="chat-timeline" ref={timeline} onScroll={() => { const el = timeline.current!; stick.current = el.scrollHeight - el.scrollTop - el.clientHeight < 100 }}>
      {state.hasMore[conversation.id] && <button className="social-button secondary live-load-history" disabled={state.loadingHistory[conversation.id]} onClick={() => { stick.current = false; void state.action(() => state.loadHistory(conversation.id, true)) }}>{state.loadingHistory[conversation.id] ? "加载中…" : "加载更早的消息"}</button>}
      {!messages ? <div className="list-empty"><button disabled={state.loadingHistory[conversation.id]} onClick={() => void state.action(() => state.loadHistory(conversation.id))}>{state.loadingHistory[conversation.id] ? "正在读取聊天记录…" : "加载聊天记录"}</button></div> : !messages.length ? <div className="conversation-start"><span><MessageCircle />每次相遇，都在同一个窗口</span><p>从第一条消息开始你们的日常。</p></div> : messages.map(message => {
        const own = message.senderId === state.snapshot!.selfId
        const generation = message.status === "generating" ? state.generations[message.id] : undefined
        const text = generation?.text ?? message.text
        const execution = message.runId ? state.details[message.runId] : undefined
        return <article key={message.id} className={`live-message ${own ? "own" : ""}`}><Avatar self={own} contact={avatar(contact)} size="small" /><div className="live-message-content"><div className="live-message-label"><span>{message.senderName}{!own && <span className="ai-badge">AI</span>}</span><time>{shortTime(message.createdAt)}</time></div><div className="live-bubble">{message.status === "withdrawn" ? "消息已撤回" : text ? <MarkdownView content={text} /> : message.status === "generating" ? <span className="live-thinking">正在回复…</span> : "本次回复没有正文"}</div>{message.status === "interrupted" && <p className="live-interruption">{execution?.error?.message ?? "本次回复已中断，片段保留在记录中。"}</p>}{message.runId && <details className="live-run-details"><summary onClick={() => void state.action(() => state.loadDetails(message.runId!))}>回复详情</summary>{execution && <p>{execution.provider} / {execution.model}{execution.usage && ` · ${execution.usage.total_tokens} tokens`}</p>}{(generation?.reasoning || execution?.reasoning) && <pre>{generation?.reasoning ?? execution?.reasoning}</pre>}</details>}</div></article>
      })}
    </div>
    {readOnly ? <div className="archive-footer"><p>聊天记录保留在这个窗口中。</p></div> : <div className="composer-area">{(!contact?.binding || conversation.paused) && <div className="chat-status-notice">{conversation.paused ? "AI 已暂停接话，仍可以发送和保存消息。" : "该 AI 尚未配置模型，发送的消息仍会保存。"}</div>}<div className="composer"><textarea aria-label="消息输入框" rows={2} maxLength={20000} placeholder={`给 ${contact?.name ?? conversation.title} 发消息…`} value={draft} onChange={e => state.updateDraft(conversation.id, e.target.value)} onKeyDown={e => { if (e.key === "Enter" && !e.shiftKey && !e.nativeEvent.isComposing && e.keyCode !== 229) { e.preventDefault(); void state.send(conversation.id) } }} /><div className="composer-bottom"><span className="composer-hint">{run ? "正在回复，你可以继续发送消息" : "每条消息都会先保存"}</span><button className="send-button" aria-label="发送消息" disabled={state.sending[conversation.id] || !draft.trim()} onClick={() => void state.send(conversation.id)}><ArrowUp /></button></div></div><div className="composer-caption"><span>Enter 发送 · Shift + Enter 换行</span>{run && <button onClick={() => void state.action(() => socialClient.cancel(run.id))}><CircleStop />停止这次回复</button>}</div></div>}
  </section>{details && <aside className="details-panel" aria-label="聊天详情"><header><strong>聊天详情</strong><IconButton label="关闭聊天详情" onClick={() => navigate("messages", conversation.id)}><X /></IconButton></header><div className="details-tabs"><button className={!memories ? "selected" : ""} onClick={() => navigate("messages", conversation.id, true)}>聊天</button><button className={memories ? "selected" : ""} onClick={() => navigate("messages", conversation.id, true, true)}><Brain />意识与记忆</button></div><div className="details-scroll">{memories ? <ConsciousnessInfo conversation={conversation} /> : <><div className="details-hero"><Avatar contact={avatar(contact)} size="large" /><h2>{contact?.name ?? conversation.title}</h2><p>一个持续积累的长期私聊</p></div><button className="details-memory-link" onClick={() => navigate("messages", conversation.id, true, true)}><Brain /><span>查看意识与上下文<small>近期记录与角色设定</small></span><span>→</span></button>{contact && <button className="social-button secondary" onClick={() => navigate("contacts", contact.id)}>查看联系人资料</button>}</>}</div></aside>}</>
}

export default function SocialApp() {
  const state = useLiveSocialStore()
  const theme = useUiStore(s => s.theme), toggleTheme = useUiStore(s => s.toggleTheme), showSettings = useUiStore(s => s.showSettings)
  const [route, setRoute] = useState(() => parseSocialRoute(location.hash))
  const [query, setQuery] = useState("")
  const [editing, setEditing] = useState<Contact | "new" | null>(null)
  const [settingsMounted, setSettingsMounted] = useState(route.page === "settings")
  const previousPage = useRef(route.page)
  const lastMessage = useRef<string | undefined>(undefined), lastContact = useRef<string | undefined>(undefined)
  useEffect(() => { void state.initialize() }, [state.initialize])
  useEffect(() => {
    const sync = () => {
      const next = parseSocialRoute(location.hash)
      setRoute(next)
      setQuery("")
      if (next.page === "settings") { setSettingsMounted(true); setEditing(null) }
    }
    window.addEventListener("hashchange", sync); return () => window.removeEventListener("hashchange", sync)
  }, [])
  useEffect(() => {
    if (previousPage.current === "settings" && route.page !== "settings") void state.action(state.refresh)
    previousPage.current = route.page
  }, [route.page, state.action, state.refresh])
  useEffect(() => { if (route.page === "messages" && route.id) lastMessage.current = route.id; if (route.page === "contacts" && route.id) lastContact.current = route.id }, [route])
  const snapshot = state.snapshot
  const conversation = snapshot?.conversations.find(c => route.page === "messages" && c.id === route.id)
  const contact = snapshot?.contacts.find(c => route.page === "contacts" ? c.id === route.id : conversation?.memberIds.includes(c.id))
  const direct = contact && snapshot?.conversations.find(c => c.kind === "direct" && c.memberIds.includes(contact.id))
  const contacts = snapshot?.contacts.filter(c => `${c.name} ${c.persona}`.toLocaleLowerCase().includes(query.toLocaleLowerCase())) ?? []
  const conversations = snapshot?.conversations.filter(c => `${c.title} ${snapshot.contacts.filter(a => c.memberIds.includes(a.id)).map(a => a.name).join(" ")}`.toLocaleLowerCase().includes(query.toLocaleLowerCase())) ?? []
  const openDirect = (actor: Contact) => void state.action(async () => { const id = await socialClient.openDirect(actor.id, crypto.randomUUID()); await state.refresh(); navigate("messages", id) })
  return <div className="social-shell"><div className={`social-main ${route.page === "settings" ? "is-hidden" : ""} ${route.id ? "has-selection" : ""}`}>
    <nav className="social-rail" aria-label="主导航"><div className="rail-brand"><BrandMark className="h-8 w-8" /></div><div className="rail-nav"><button className={route.page === "messages" ? "selected" : ""} onClick={() => navigate("messages", lastMessage.current)}><MessageCircle /><small>消息</small></button><button className={route.page === "contacts" ? "selected" : ""} onClick={() => navigate("contacts", lastContact.current)}><Users /><small>通讯录</small></button></div><div className="rail-bottom"><IconButton label={theme === "light" ? "切换深色主题" : "切换浅色主题"} onClick={toggleTheme}>{theme === "light" ? <Moon /> : <Sun />}</IconButton><IconButton label="设置" onClick={() => showSettings()}><Settings /></IconButton><Avatar self size="small" /></div></nav>
    <aside className="social-list-pane"><header className="list-header"><div><h1>{route.page === "contacts" ? "通讯录" : "消息"}</h1><p>{snapshot ? route.page === "contacts" ? `${snapshot.contacts.length} 位 AI 联系人` : "把每一次对话，连成日常" : "正在连接本地服务"}</p></div><IconButton label="添加 AI 联系人" disabled={!snapshot} onClick={() => { state.dismissError(); setEditing("new") }}><Plus /></IconButton></header><div className="list-search"><Search /><input aria-label={route.page === "contacts" ? "搜索联系人" : "搜索聊天"} placeholder={route.page === "contacts" ? "搜索联系人…" : "搜索聊天…"} value={query} onChange={e => setQuery(e.target.value)} /></div><div className="social-list">{route.page === "contacts" ? contacts.map(c => <button key={c.id} className={`contact-list-item ${contact?.id === c.id ? "selected" : ""}`} onClick={() => navigate("contacts", c.id)}><Avatar contact={avatar(c)} /><span><strong>{c.name}<span className="ai-badge">AI</span></strong><small>{c.persona}</small></span><ArrowRight /></button>) : conversations.map(c => {
      const actor = snapshot!.contacts.find(a => c.memberIds.includes(a.id))
      return <button key={c.id} className={`conversation-list-item ${conversation?.id === c.id ? "selected" : ""}`} onClick={() => navigate("messages", c.id)}><Avatar contact={avatar(actor)} group={c.kind === "group"} /><span className="conversation-list-text"><span><strong>{actor?.name ?? c.title}</strong><time>{c.latestMessage && shortTime(c.latestMessage.createdAt)}</time></span><span><small>{c.latestMessage?.text || (c.activeRun ? "正在回复…" : "开始你们的第一段对话")}</small></span></span></button>
    })}{snapshot && (route.page === "contacts" ? !contacts.length : !conversations.length) && <div className="list-empty">{query ? "没有找到结果" : "从认识一位 AI 朋友开始"}</div>}</div><div className="preview-indicator"><span /><div><strong>本地聊天</strong><small>{state.connected ? "记录持续保存" : "连接恢复中"}</small></div></div></aside>
    <main className="social-workspace">{(state.error || state.connectionError) && <div className="social-error" role="alert"><span>{state.error ?? state.connectionError}</span><IconButton label="关闭错误提示" onClick={state.dismissError}><X /></IconButton></div>}<div className="workspace-content">{!snapshot ? <EmptyState title={state.error ? "无法打开消息与通讯录" : "正在打开消息与通讯录…"} description="连接本地服务后，可以继续原来的聊天。"><button className="social-button primary" disabled={state.loading} onClick={() => void state.initialize()}>重试连接</button><button className="social-button secondary" onClick={() => showSettings()}>打开设置</button></EmptyState> : conversation ? <Chat key={conversation.id} conversation={conversation} contact={contact} details={route.details} memories={route.memories} /> : contact ? <section className="contact-view"><header className="contact-header"><IconButton label="返回通讯录" className="mobile-back" onClick={() => navigate("contacts")}><ArrowLeft /></IconButton><span>联系人资料</span><button className="text-button" onClick={() => { state.dismissError(); setEditing(contact) }}><Pencil />编辑资料</button></header><div className="contact-scroll"><div className="profile-cover" /><div className="profile-content"><div className="profile-intro"><Avatar contact={avatar(contact)} size="large" /><div><h1>{contact.name}<span className="ai-badge">AI 联系人</span></h1></div><button className="social-button primary" onClick={() => openDirect(contact)}><MessageCircle />发消息</button></div><div className="profile-tabs"><button className={!route.memories ? "selected" : ""} onClick={() => navigate("contacts", contact.id)}>资料</button><button className={route.memories ? "selected" : ""} onClick={() => navigate("contacts", contact.id, false, true)}><Brain />意识与记忆</button></div>{route.memories ? <ConsciousnessInfo conversation={direct} /> : <div className="profile-sections"><section><h2>关于 {contact.name}</h2><p className="persona-text">{contact.persona}</p></section><section><h2>回复模型</h2><p>{contact.binding ? `${contact.binding.providerId} / ${contact.binding.modelId}` : "尚未配置"}</p><p className="section-description">身份与执行配置独立，更换模型后仍是同一位联系人。</p></section></div>}</div></div></section> : <EmptyState title={route.id ? "这个页面不存在" : "从认识开始，让对话有归属"} description={route.id ? "请从左侧重新选择聊天或联系人。" : "给 AI 一个名字、一种性格，也给彼此一个可以持续相处的空间。"}><button className="social-button primary" onClick={() => setEditing("new")}><Plus />添加 AI 联系人</button></EmptyState>}</div></main>
  </div>{settingsMounted && <div className={route.page === "settings" ? "settings-container" : "is-hidden"}><SocialSettings onBack={() => navigate("messages", lastMessage.current)} /></div>}
    <Dialog open={!!editing} onOpenChange={open => { if (!open) setEditing(null) }}><DialogContent className="social-dialog"><DialogTitle>{editing === "new" ? "认识一位新朋友" : "编辑联系人"}</DialogTitle><DialogDescription>角色设定决定如何相处，模型配置决定由哪个模型回复。</DialogDescription>{state.error && <p role="alert" className="form-error">{state.error}</p>}{editing && snapshot && <ContactEditor key={editing === "new" ? "new" : editing.id} contact={editing === "new" ? undefined : editing} models={snapshot.models} close={() => setEditing(null)} />}</DialogContent></Dialog>
  </div>
}
