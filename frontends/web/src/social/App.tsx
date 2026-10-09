import { useEffect, useRef, useState } from "react"
import { ArrowRight, MessageCircle, Moon, Plus, Search, Settings, Sun, Users, X } from "lucide-react"
import { BrandMark } from "@/components/BrandMark"
import { SettingsPage } from "@/components/settings/SettingsPage"
import { Dialog, DialogContent, DialogDescription, DialogTitle } from "@/components/ui/dialog"
import { useInit } from "@/hooks/useInit"
import { useUiStore } from "@/store/ui"
import { useConfigStore } from "@/store/config"
import { CUSTOM_WINDOW_CHROME } from "@/lib/desktop"
import { Avatar, EmptyState, IconButton, shortTime } from "./components/common"
import { navigate, parseSocialRoute } from "./router"
import { useLiveSocialStore } from "./live-store"
import { Chat } from "./LiveChat"
import { CreateGroupForm } from "./GroupControls"
import { AccountDetails, AccountViewer, AiManagement } from "./AccountPages"
import { contactAvatar as avatar } from "./live-ui"
import "./social.css"

function SocialSettings({ onBack }: { onBack(): void }) {
  const { ready, error, retry } = useInit()
  const saveError = useConfigStore(s => s.saveError), saving = useConfigStore(s => s.saving), retrySave = useConfigStore(s => s.retrySave)
  return <div className="social-settings">{error ? <div className="social-startup"><p role="alert">{error}</p><button className="social-button secondary" onClick={retry}>重试</button></div> : ready ? <SettingsPage onBack={onBack} availableSections={["models", "appearance", "config", "about"]} notices={saveError ? <div role="alert" className="social-error"><span>{saveError}</span><button disabled={saving} onClick={retrySave}>重试保存</button></div> : undefined} /> : <div className="social-startup">正在加载设置…</div>}</div>
}

export default function SocialApp() {
  const state = useLiveSocialStore()
  const theme = useUiStore(s => s.theme), toggleTheme = useUiStore(s => s.toggleTheme), showSettings = useUiStore(s => s.showSettings)
  const [route, setRoute] = useState(() => parseSocialRoute(location.hash))
  const [query, setQuery] = useState("")
  const [contactFilter, setContactFilter] = useState<"listed" | "directory">("listed")
  const [creatingGroup, setCreatingGroup] = useState(false)
  const [settingsMounted, setSettingsMounted] = useState(route.page === "settings")
  const previousPage = useRef(route.page)
  const lastMessage = useRef<string | undefined>(undefined), lastContact = useRef<string | undefined>(undefined)
  useEffect(() => {
    const initialize = () => { void state.initialize() }
    initialize(); window.addEventListener("pagehide", state.disconnect); window.addEventListener("pageshow", initialize)
    return () => { window.removeEventListener("pagehide", state.disconnect); window.removeEventListener("pageshow", initialize); state.disconnect() }
  }, [state.initialize, state.disconnect])
  useEffect(() => {
    const sync = () => { const next = parseSocialRoute(location.hash); setRoute(next); setQuery(""); if (next.page === "settings") setSettingsMounted(true) }
    window.addEventListener("hashchange", sync); return () => window.removeEventListener("hashchange", sync)
  }, [])
  useEffect(() => {
    if (previousPage.current === "settings" && route.page !== "settings") void state.action(state.refresh)
    previousPage.current = route.page
  }, [route.page, state.action, state.refresh])
  useEffect(() => { if (route.page === "messages" && route.id) lastMessage.current = route.id; if (route.page === "contacts" && route.id) lastContact.current = route.id }, [route])
  const snapshot = state.snapshot
  const personal = ["account", "ai-management", "account-view"].includes(route.page)
  const conversation = snapshot?.conversations.find(c => route.page === "messages" && c.id === route.id)
  const contact = snapshot?.accounts.find(c => route.page === "contacts" ? c.id === route.id : conversation?.kind === "direct" && c.id !== snapshot.selfId && conversation.memberIds.includes(c.id))
  const listed = snapshot?.accounts.filter(a => snapshot.addressBook.some(e => e.accountId === a.id && e.listed)) ?? []
  const contacts = (contactFilter === "listed" ? listed : snapshot?.accounts.filter(a => a.id !== snapshot.selfId) ?? []).filter(a => `${a.name} ${a.bio}`.toLocaleLowerCase().includes(query.toLocaleLowerCase()))
  const activeContacts = listed.filter(a => a.kind === "ai" && a.status === "active")
  const conversations = snapshot?.conversations.filter(c => `${c.title} ${snapshot.accounts.filter(a => c.memberIds.includes(a.id)).map(a => a.name).join(" ")}`.toLocaleLowerCase().includes(query.toLocaleLowerCase())) ?? []
  return <div className={`social-shell${CUSTOM_WINDOW_CHROME ? " desktop-chrome" : ""}`}>
    {CUSTOM_WINDOW_CHROME && <div className="social-titlebar" data-tauri-drag-region><span data-tauri-drag-region>LLM Nest</span></div>}
    <div className={`social-main ${route.page === "settings" ? "is-hidden" : ""} ${route.id || personal ? "has-selection" : ""}`}>
      <nav className="social-rail" aria-label="主导航"><div className="rail-brand"><BrandMark className="h-8 w-8" /></div><div className="rail-nav"><button className={route.page === "messages" ? "selected" : ""} onClick={() => navigate("messages", lastMessage.current)}><MessageCircle /><small>消息</small></button><button className={route.page === "contacts" ? "selected" : ""} onClick={() => navigate("contacts", lastContact.current)}><Users /><small>通讯录</small></button></div><div className="rail-bottom"><IconButton label={theme === "light" ? "切换深色主题" : "切换浅色主题"} onClick={toggleTheme}>{theme === "light" ? <Moon /> : <Sun />}</IconButton><IconButton label="设置" onClick={() => showSettings()}><Settings /></IconButton><button className={`account-entry ${personal ? "selected" : ""}`} aria-label="我的账号" onClick={() => navigate("account")}><Avatar contact={avatar(snapshot?.selfAccount)} size="small" /></button></div></nav>
      <aside className="social-list-pane">{personal ? <><header className="list-header"><div><h1>我的账号</h1><p>{snapshot?.selfName}</p></div></header><div className="social-list"><button className={`contact-list-item ${route.page === "account" ? "selected" : ""}`} onClick={() => navigate("account")}><span><strong>账号资料</strong><small>公开名字与简介</small></span><ArrowRight /></button><button className={`contact-list-item ${route.page !== "account" ? "selected" : ""}`} onClick={() => navigate("ai-management")}><span><strong>AI 角色管理</strong><small>设定、模型与管理员权限</small></span><ArrowRight /></button></div></> : <><header className="list-header"><div><h1>{route.page === "contacts" ? "通讯录" : "消息"}</h1><p>{snapshot ? route.page === "contacts" ? `${listed.length} 位联系人` : "把每一次对话，连成日常" : "正在连接本地服务"}</p></div><div className="live-list-actions"><IconButton label="创建群聊" disabled={!activeContacts.length} onClick={() => { state.dismissError(); setCreatingGroup(true) }}><Users /></IconButton><IconButton label="添加联系人" disabled={!snapshot} onClick={() => { setContactFilter("directory"); navigate("contacts") }}><Plus /></IconButton></div></header><div className="list-search"><Search /><input aria-label={route.page === "contacts" ? "搜索联系人" : "搜索聊天"} placeholder={route.page === "contacts" ? "搜索联系人…" : "搜索聊天…"} value={query} onChange={e => setQuery(e.target.value)} /></div>{route.page === "contacts" && <div className="profile-tabs live-contact-tabs"><button className={contactFilter === "listed" ? "selected" : ""} onClick={() => setContactFilter("listed")}>我的联系人 · {listed.length}</button><button className={contactFilter === "directory" ? "selected" : ""} onClick={() => setContactFilter("directory")}>本地账号</button></div>}<div className="social-list">{route.page === "contacts" ? contacts.map(a => <button key={a.id} className={`contact-list-item ${contact?.id === a.id ? "selected" : ""}`} onClick={() => navigate("contacts", a.id)}><Avatar contact={avatar(a)} /><span><strong>{a.name}{a.kind === "ai" && <span className="ai-badge">AI</span>}</strong><small>{a.status === "archived" ? "账号已停用 · " : ""}{a.bio || "还没有填写简介"}</small></span><ArrowRight /></button>) : conversations.map(c => {
        const actor = snapshot!.accounts.find(a => a.id !== snapshot!.selfId && c.memberIds.includes(a.id))
        return <button key={c.id} className={`conversation-list-item ${conversation?.id === c.id ? "selected" : ""}`} onClick={() => navigate("messages", c.id)}><Avatar contact={avatar(actor)} group={c.kind === "group"} /><span className="conversation-list-text"><span><strong>{c.kind === "group" ? c.title : actor?.name ?? c.title}</strong><time>{c.latestMessage && shortTime(c.latestMessage.createdAt)}</time></span><span><small>{c.kind === "direct" && actor?.status === "archived" && "账号已停用 · "}{c.latestMessage?.text || (c.activeRun ? "正在回复…" : "开始你们的第一段对话")}</small></span></span></button>
      })}{snapshot && (route.page === "contacts" ? !contacts.length : !conversations.length) && <div className="list-empty">{query ? "没有找到结果" : "从认识一位朋友开始"}</div>}</div></>}<div className="preview-indicator"><span /><div><strong>本地聊天</strong><small>{state.connected ? "记录持续保存" : "连接恢复中"}</small></div></div></aside>
      <main className="social-workspace">{(state.error || state.connectionError) && <div className="social-error" role="alert"><span>{state.error ?? state.connectionError}</span><IconButton label="关闭错误提示" onClick={state.dismissError}><X /></IconButton></div>}<div className="workspace-content">{!snapshot ? <EmptyState title={state.error ? "无法打开消息与通讯录" : "正在打开消息与通讯录…"} description="连接本地服务后，可以继续原来的聊天。"><button className="social-button primary" disabled={state.loading} onClick={() => void state.initialize()}>重试连接</button><button className="social-button secondary" onClick={() => showSettings()}>打开设置</button></EmptyState> : route.page === "account" ? <AccountDetails key={snapshot.selfId} account={snapshot.selfAccount} /> : route.page === "ai-management" ? <AiManagement id={route.id} /> : route.page === "account-view" && route.id ? <AccountViewer key={route.id} id={route.id} /> : conversation ? <Chat key={conversation.id} conversation={conversation} contact={contact} details={route.details} memories={route.memories} /> : contact ? <AccountDetails key={contact.id} account={contact} memories={route.memories} /> : <EmptyState title={route.id ? "这个页面不存在" : "从认识开始，让对话有归属"} description={route.id ? "请重新选择聊天或账号。" : "每个账号都有自己的身份，每段关系都有持续的聊天空间。"}><button className="social-button primary" onClick={() => navigate("ai-management")}><Plus />创建 AI 账号</button></EmptyState>}</div></main>
    </div>{settingsMounted && <div className={route.page === "settings" ? "settings-container" : "is-hidden"}><SocialSettings onBack={() => navigate("messages", lastMessage.current)} /></div>}
    <Dialog open={creatingGroup} onOpenChange={setCreatingGroup}><DialogContent className="social-dialog"><DialogTitle>创建群聊</DialogTitle><DialogDescription>邀请已有联系人，一起开启一个独立的聊天窗口。</DialogDescription>{state.error && <p role="alert" className="form-error">{state.error}</p>}{creatingGroup && snapshot && <CreateGroupForm contacts={listed.filter(a => a.kind === "ai")} close={() => setCreatingGroup(false)} />}</DialogContent></Dialog>
  </div>
}
