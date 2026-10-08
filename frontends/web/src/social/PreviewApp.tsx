import { useEffect, useRef, useState } from "react"
import { Archive, ArrowRight, Check, ChevronDown, MessageCircle, Moon, Pin, Plus, Search, Settings, Sun, Users, X } from "lucide-react"
import { useUiStore } from "@/store/ui"
import { useConfigStore } from "@/store/config"
import { useInit } from "@/hooks/useInit"
import { SettingsPage } from "@/components/settings/SettingsPage"
import { BrandMark } from "@/components/BrandMark"
import { useSocialStore } from "./store"
import { navigate, parseSocialRoute } from "./router"
import { Avatar, EmptyState, IconButton, shortTime } from "./components/common"
import { EditorDialogs } from "./components/EditorDialogs"
import type { Editor } from "./components/EditorDialogs"
import { ConversationView } from "./components/ConversationView"
import { ContactView } from "./components/ContactView"
import { DetailsPanel } from "./components/DetailsPanel"
import "./social.css"

function PreviewSettings() {
  const { ready, error, retry } = useInit()
  const saveError = useConfigStore(s => s.saveError)
  const retrySave = useConfigStore(s => s.retrySave)
  const saving = useConfigStore(s => s.saving)
  const notices = saveError ? <div role="alert" className="social-error"><span>{saveError}</span><button disabled={saving} onClick={retrySave}>重试保存</button></div> : undefined
  return <div className="social-settings"><div className="preview-settings-note">社交交互预览 · 以下设置使用独立的演示配置，尚未连接真实社交后端。</div>{error ? <div role="alert" className="social-startup"><p>{error}</p><button className="social-button secondary" onClick={retry}>重试</button></div> : ready ? <SettingsPage notices={notices} /> : <div className="social-startup">正在加载设置…</div>}</div>
}

export function PreviewApp() {
  const snapshot = useSocialStore(s => s.snapshot)
  const error = useSocialStore(s => s.error)
  const loading = useSocialStore(s => s.loading)
  const initialize = useSocialStore(s => s.initialize)
  const dismissError = useSocialStore(s => s.dismissError)
  const theme = useUiStore(s => s.theme)
  const toggleTheme = useUiStore(s => s.toggleTheme)
  const showSettings = useUiStore(s => s.showSettings)
  const [route, setRoute] = useState(() => parseSocialRoute(location.hash))
  const [messageQuery, setMessageQuery] = useState("")
  const [contactQuery, setContactQuery] = useState("")
  const [filter, setFilter] = useState<"all" | "unread" | "archived">("all")
  const [addMenu, setAddMenu] = useState(false)
  const [editor, setEditor] = useState<Editor>(null)
  const [settingsMounted, setSettingsMounted] = useState(route.page === "settings")
  const lastMessage = useRef<string | undefined>(undefined)
  const lastContact = useRef<string | undefined>(undefined)

  useEffect(() => { void initialize() }, [initialize])
  useEffect(() => {
    const sync = () => {
      const next = parseSocialRoute(location.hash)
      if (location.hash === "#/chat") { navigate("messages", lastMessage.current); return }
      setRoute(next); setAddMenu(false)
      if (next.page === "settings") { setSettingsMounted(true); setEditor(null) }
    }
    window.addEventListener("hashchange", sync)
    return () => window.removeEventListener("hashchange", sync)
  }, [])
  useEffect(() => {
    if (route.page === "messages" && route.id) lastMessage.current = route.id
    if (route.page === "contacts" && route.id) lastContact.current = route.id
  }, [route])
  useEffect(() => {
    if (!snapshot || route.page === "settings" || route.id || location.hash === "#/messages" || location.hash === "#/contacts") return
    navigate("messages", snapshot.conversations.find(c => c.pinned && c.status === "active")?.id ?? snapshot.conversations[0]?.id)
  }, [snapshot, route])
  useEffect(() => {
    if (!addMenu) return
    const close = (event: MouseEvent) => { if (!(event.target as HTMLElement).closest(".list-add")) setAddMenu(false) }
    const escape = (event: KeyboardEvent) => { if (event.key === "Escape") setAddMenu(false) }
    window.addEventListener("click", close); window.addEventListener("keydown", escape)
    return () => { window.removeEventListener("click", close); window.removeEventListener("keydown", escape) }
  }, [addMenu])

  if (!snapshot) return <div className="social-shell social-startup"><BrandMark className="h-10 w-10" /><h1>LLM Nest</h1><p>{error ? "无法加载浏览器中的预览数据" : "正在打开消息与通讯录…"}</p>{error && <p role="alert">{error}</p>}{error && <button disabled={loading} className="social-button primary" onClick={() => void initialize()}>重试加载</button>}</div>

  const conversation = route.page === "messages" ? snapshot.conversations.find(c => c.id === route.id) : undefined
  const contact = route.page === "contacts" ? snapshot.contacts.find(c => c.id === route.id) : undefined
  const totalUnread = snapshot.conversations.filter(c => c.status === "active").reduce((n, c) => n + c.unread, 0)
  const messages = snapshot.conversations.filter(c => {
    if (filter === "archived" ? c.status !== "archived" : c.status !== "active") return false
    if (filter === "unread" && !c.unread) return false
    const memberNames = snapshot.contacts.filter(a => c.memberIds.includes(a.id)).map(a => a.name).join(" ")
    return `${c.title} ${memberNames}`.toLocaleLowerCase().includes(messageQuery.trim().toLocaleLowerCase())
  }).sort((a, b) => Number(b.pinned) - Number(a.pinned) || b.updatedAt.localeCompare(a.updatedAt))
  const contacts = snapshot.contacts.filter(c => `${c.name} ${c.tagline} ${c.interests.join(" ")}`.toLocaleLowerCase().includes(contactQuery.trim().toLocaleLowerCase()))
  const edit = (next: Editor) => { dismissError(); setEditor(next); setAddMenu(false) }

  return <div className="social-shell">
    <div className={`social-main ${route.page === "settings" ? "is-hidden" : ""} ${route.id ? "has-selection" : ""}`}>
      <nav className="social-rail" aria-label="主导航">
        <div className="rail-brand" title="LLM Nest"><BrandMark className="h-8 w-8" /></div>
        <div className="rail-nav"><button className={route.page === "messages" ? "selected" : ""} aria-current={route.page === "messages" ? "page" : undefined} onClick={() => navigate("messages", lastMessage.current)}><span><MessageCircle />{!!totalUnread && <i />}</span><small>消息</small></button><button className={route.page === "contacts" ? "selected" : ""} aria-current={route.page === "contacts" ? "page" : undefined} onClick={() => navigate("contacts", lastContact.current)}><Users /><small>通讯录</small></button></div>
        <div className="rail-bottom"><IconButton label={theme === "light" ? "切换深色主题" : "切换浅色主题"} onClick={toggleTheme}>{theme === "light" ? <Moon /> : <Sun />}</IconButton><IconButton label="设置" onClick={() => showSettings()}><Settings /></IconButton><Avatar self size="small" /></div>
      </nav>
      <aside className="social-list-pane">
        <header className="list-header"><div><h1>{route.page === "contacts" ? "通讯录" : "消息"}</h1><p>{route.page === "contacts" ? `${snapshot.contacts.length} 位 AI 朋友` : "把每一次对话，连成日常"}</p></div><div className="list-add"><IconButton label={route.page === "contacts" ? "添加 AI 联系人" : "新建聊天"} aria-expanded={addMenu} onClick={() => route.page === "contacts" ? edit({ kind: "contact" }) : setAddMenu(!addMenu)}><Plus /></IconButton>{addMenu && <div className="list-add-menu"><button onClick={() => edit({ kind: "contact" })}><Plus />添加 AI 联系人</button><button onClick={() => edit({ kind: "group" })}><Users />创建群聊</button></div>}</div></header>
        <div className="list-search"><Search /><input aria-label={route.page === "contacts" ? "搜索联系人" : "搜索聊天"} placeholder={route.page === "contacts" ? "搜索名字、兴趣…" : "搜索聊天…"} value={route.page === "contacts" ? contactQuery : messageQuery} onChange={e => route.page === "contacts" ? setContactQuery(e.target.value) : setMessageQuery(e.target.value)} />{(route.page === "contacts" ? contactQuery : messageQuery) && <IconButton label="清空搜索" onClick={() => route.page === "contacts" ? setContactQuery("") : setMessageQuery("")}><X /></IconButton>}</div>
        {route.page !== "contacts" && <div className="message-filters" aria-label="筛选聊天">{(["all", "unread", "archived"] as const).map(value => <button key={value} aria-pressed={filter === value} className={filter === value ? "selected" : ""} onClick={() => setFilter(value)}>{value === "all" ? "全部" : value === "unread" ? "未读" : "归档"}{value === "unread" && !!totalUnread && <span>{totalUnread}</span>}</button>)}</div>}
        <div className="social-list">
          {route.page === "contacts" ? <><div className="list-section-label">AI 联系人<span>{contacts.length}</span></div>{contacts.map(item => <button key={item.id} className={`contact-list-item ${contact?.id === item.id ? "selected" : ""}`} onClick={() => navigate("contacts", item.id)}><Avatar contact={item} /><span><strong>{item.name}<span className="ai-badge">AI</span></strong><small>{item.tagline}</small></span><ArrowRight /></button>)}{!contacts.length && <div className="list-empty">没有找到联系人</div>}<button className="add-contact-card" onClick={() => edit({ kind: "contact" })}><Plus /><span>认识一位新朋友<small>创建拥有独立身份的 AI 联系人</small></span></button></> : <>
            {messages.map(item => {
              const actor = item.kind === "direct" ? snapshot.contacts.find(c => item.memberIds.includes(c.id)) : undefined
              const history = snapshot.messages.filter(m => m.conversationId === item.id)
              const latest = history[history.length - 1]
              return <button key={item.id} className={`conversation-list-item ${conversation?.id === item.id ? "selected" : ""}`} onClick={() => navigate("messages", item.id)}><Avatar contact={actor} group={item.kind === "group"} /><span className="conversation-list-text"><span><strong>{item.title}</strong><time>{shortTime(item.updatedAt)}</time></span><span><small>{latest ? latest.status === "redacted" ? "消息已撤回" : `${item.kind === "group" ? `${latest.senderName}：` : ""}${latest.text || "正在回复…"}` : "开始你们的第一段对话"}</small>{item.unread ? <b className="unread-count">{item.unread > 99 ? "99+" : item.unread}</b> : item.pinned ? <Pin className="list-pin" /> : item.status === "archived" ? <Archive className="list-pin" /> : null}</span>{item.paused && <em>AI 已暂停接话</em>}</span></button>
            })}
            {!messages.length && <div className="list-empty">{messageQuery ? "没有找到聊天" : filter === "unread" ? <><Check />消息都看过了</> : filter === "archived" ? "还没有归档聊天" : "从通讯录开始一次聊天吧"}</div>}
          </>}
        </div>
        <div className="preview-indicator"><span /><div><strong>Web 交互预览</strong><small>模拟数据 · 保存在当前浏览器</small></div><ChevronDown /></div>
      </aside>
      <main className="social-workspace">
        {error && !editor && <div className="social-error" role="alert"><span>{error}</span><IconButton label="关闭错误提示" onClick={dismissError}><X /></IconButton></div>}
        <div className="workspace-content">{conversation ? <><ConversationView conversation={conversation} details={route.details} edit={edit} />{route.details && <DetailsPanel conversation={conversation} memories={route.memories} edit={edit} />}</> : contact ? <ContactView contact={contact} memories={route.memories} edit={edit} /> : <EmptyState title={route.id ? "这个页面不存在" : route.page === "contacts" ? "从认识开始，让对话有归属" : "故事在对话中继续"} description={route.id ? "请从左侧重新选择聊天或联系人。" : route.page === "contacts" ? "给 AI 一个名字、一种性格，也给彼此一个可以持续相处的空间。" : "选择一段聊天，或邀请几位 AI 朋友一起聊聊。"}><button className="social-button primary" onClick={() => route.page === "contacts" ? edit({ kind: "contact" }) : edit({ kind: "group" })}><Plus />{route.page === "contacts" ? "添加 AI 联系人" : "创建群聊"}</button></EmptyState>}</div>
      </main>
    </div>
    {settingsMounted && <div className={route.page === "settings" ? "settings-container" : "is-hidden"}><PreviewSettings /></div>}
    <EditorDialogs key={editor?.kind === "contact" ? editor.contact?.id ?? "new-contact" : editor?.kind === "memory" ? editor.record?.id ?? editor.source?.id ?? "new-memory" : editor?.kind === "message" ? editor.message.id : editor?.kind ?? "closed"} editor={editor} close={() => setEditor(null)} />
  </div>
}
