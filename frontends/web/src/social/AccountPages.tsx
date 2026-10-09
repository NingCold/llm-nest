import { useEffect, useRef, useState, type FormEvent } from "react"
import { ArrowLeft, Brain, MessageCircle, Pencil, Plus, RefreshCw } from "lucide-react"
import { MarkdownView } from "@/components/chat/MarkdownView"
import { Dialog, DialogContent, DialogDescription, DialogTitle } from "@/components/ui/dialog"
import { Avatar, EmptyState, IconButton, shortTime } from "./components/common"
import { socialClient } from "./client"
import { useLiveSocialStore } from "./live-store"
import { ContactControls, useVersionedAction } from "./LiveControls"
import { ConsciousnessInfo } from "./LiveChat"
import { contactAvatar } from "./live-ui"
import { navigate } from "./router"
import type { Account, AccountView, ManagedAi, MessagePage, ModelOption } from "./live-types"

// Human and AI accounts share their public profile. Private AI settings live below.
export function AccountDetails({ account, memories = false }: { account: Account; memories?: boolean }) {
  const state = useLiveSocialStore(), snapshot = state.snapshot!
  const own = account.id === snapshot.selfId
  const direct = snapshot.conversations.find(c => c.kind === "direct" && c.memberIds.includes(account.id))
  const entry = snapshot.addressBook.find(e => e.accountId === account.id)
  const [editing, setEditing] = useState(false)
  const [name, setName] = useState(account.name), [bio, setBio] = useState(account.bio)
  const { busy, execute } = useVersionedAction()
  const save = async (event: FormEvent) => {
    event.preventDefault()
    const result = await execute(`profile:${name}:${bio}`, account.revision, input => socialClient.updateAccount(account.id, { ...input, name, bio }))
    if (result) { setEditing(false); await state.action(state.refresh) }
  }
  const openDirect = () => void state.action(async () => {
    const id = await socialClient.openDirect(account.id, crypto.randomUUID())
    await state.refresh(); navigate("messages", id)
  })
  return <section className="contact-view"><header className="contact-header"><IconButton label="返回" className="mobile-back" onClick={() => navigate("contacts")}><ArrowLeft /></IconButton><span>账号资料</span>{own && <button className="text-button" onClick={() => { setName(account.name); setBio(account.bio); setEditing(true) }}><Pencil />编辑资料</button>}</header>
    <div className="contact-scroll"><div className="profile-cover" /><div className="profile-content"><div className="profile-intro"><Avatar contact={contactAvatar(account)} size="large" /><div><h1>{account.name}</h1><p>{account.kind === "ai" ? "AI" : "真人"} · {account.status === "active" ? "账号正常" : "账号已停用"}</p></div>{!own && <button className="social-button primary" disabled={account.status === "archived" && !direct} onClick={openDirect}><MessageCircle />发消息</button>}</div>
      {!own && <div className="profile-tabs"><button className={!memories ? "selected" : ""} onClick={() => navigate("contacts", account.id)}>资料</button><button className={memories ? "selected" : ""} onClick={() => navigate("contacts", account.id, false, true)}><Brain />意识与记忆</button></div>}
      {memories ? <ConsciousnessInfo conversation={direct} /> : <div className="profile-sections"><section><h2>公开简介</h2><p className="persona-text">{account.bio || "还没有填写简介"}</p></section><section><h2>账号 ID</h2><p className="account-id">{account.id}</p></section>{own ? <section><h2>我的 AI 角色</h2><p>管理你有权限管理的 AI 账号。</p><button className="social-button secondary" onClick={() => navigate("ai-management")}>AI 角色管理</button></section> : <ContactControls key={account.id} contact={account} entry={entry} />}</div>}
    </div></div><Dialog open={editing} onOpenChange={setEditing}><DialogContent className="social-dialog"><DialogTitle>编辑公开资料</DialogTitle><DialogDescription>这些内容会显示在你的账号资料中。</DialogDescription>{state.error && <p role="alert" className="form-error">{state.error}</p>}<form className="social-form" onSubmit={event => void save(event)}><fieldset className="social-form-fields" disabled={busy}><label>名字<input required maxLength={40} value={name} onChange={e => setName(e.target.value)} /></label><label>公开简介<textarea rows={4} maxLength={1000} value={bio} onChange={e => setBio(e.target.value)} /></label><div className="dialog-actions"><button type="button" className="social-button secondary" onClick={() => setEditing(false)}>取消</button><button className="social-button primary">{busy ? "保存中…" : "保存资料"}</button></div></fieldset></form></DialogContent></Dialog>
  </section>
}

function AiEditor({ managed, models, done, close }: { managed?: ManagedAi; models: ModelOption[]; done(id: string): void; close(): void }) {
  const state = useLiveSocialStore()
  const [name, setName] = useState(managed?.account.name ?? "")
  const [bio, setBio] = useState(managed?.account.bio ?? "")
  const [persona, setPersona] = useState(managed?.persona ?? "")
  const [model, setModel] = useState(managed?.binding ? `${managed.binding.providerId}/${managed.binding.modelId}` : "")
  const [effort, setEffort] = useState(managed?.binding?.reasoningEffort ?? "")
  const [saving, setSaving] = useState(false)
  const pending = useRef<{ key: string; requestId: string; revision?: string } | null>(null)
  const selected = models.find(m => `${m.providerId}/${m.modelId}` === model)
  const save = async (event: FormEvent) => {
    event.preventDefault(); setSaving(true)
    const binding = selected ? { providerId: selected.providerId, modelId: selected.modelId, reasoningEffort: effort || null } : null
    const key = JSON.stringify({ name, bio, persona, binding })
    if (pending.current?.key !== key) pending.current = { key, requestId: crypto.randomUUID(), revision: managed?.account.revision }
    const input = { requestId: pending.current.requestId, name, bio, persona, binding }
    const result = await state.action(() => managed ? socialClient.updateAi(managed.account.id, { ...input, expectedRevision: pending.current!.revision! }) : socialClient.createAi(input))
    if (result) { await state.action(state.refresh); done(result.account.id); close() }
    setSaving(false)
  }
  return <form className="social-form" onSubmit={event => void save(event)}><fieldset disabled={saving} className="social-form-fields">
    <label>名字<input autoFocus required maxLength={40} value={name} onChange={e => setName(e.target.value)} placeholder="账号的公开名字" /></label>
    <label>公开简介<textarea maxLength={1000} rows={2} value={bio} onChange={e => setBio(e.target.value)} placeholder="所有人都能在账号资料中看到" /></label>
    <label>内部角色设定<textarea required maxLength={2000} rows={4} value={persona} onChange={e => setPersona(e.target.value)} placeholder="性格、背景和行为设定，仅管理员可查看和修改" /></label>
    <label>回复模型<select value={model} onChange={e => { setModel(e.target.value); setEffort("") }}><option value="">暂不配置</option>{models.map(m => <option key={`${m.providerId}/${m.modelId}`} value={`${m.providerId}/${m.modelId}`}>{m.displayName} · {m.providerId}</option>)}{model && !selected && <option value={model}>当前模型已不可用，请重新选择</option>}</select></label>
    {!!selected?.reasoningLevels.length && <label>思考强度<select value={effort} onChange={e => setEffort(e.target.value)}><option value="">使用模型默认值</option>{selected.reasoningLevels.map(level => <option key={level} value={level}>{level}</option>)}</select></label>}
    <p className="form-note">修改设定与模型保留账号、关系和历史。新的运行使用新配置，正在回复的运行保留原配置。{!models.length && "请先在设置中添加模型服务。"}</p>
    <div className="dialog-actions"><button type="button" className="social-button secondary" onClick={close}>取消</button><button className="social-button primary" disabled={!!model && !selected}>{saving ? "保存中…" : managed ? "保存修改" : "创建 AI 账号"}</button></div>
  </fieldset></form>
}

function AdministratorControls({ managed, reload }: { managed: ManagedAi; reload(): Promise<void> }) {
  const state = useLiveSocialStore(), accounts = state.snapshot!.accounts
  const [candidate, setCandidate] = useState("")
  const { busy, execute } = useVersionedAction()
  const available = accounts.filter(a => a.status === "active" && !managed.administrators.some(g => g.accountId === a.id))
  const change = async (accountId: string, granted: boolean) => {
    const result = await execute(`grant:${accountId}:${granted}`, managed.account.revision, input => socialClient.setAdministrator(managed.account.id, { ...input, accountId, granted }))
    if (result) { setCandidate(""); await state.action(reload); await state.action(state.refresh) }
  }
  return <section><h2>管理员</h2><p className="section-description">管理员可以修改设定、停用与恢复账号，并查看这个 AI 参与的聊天。初始管理员负责授予和撤销管理权。</p><div className="account-administrators">{managed.administrators.map(g => <div key={g.accountId}><span>{accounts.find(a => a.id === g.accountId)?.name ?? g.accountId}<small>{g.initial ? "初始管理员" : "管理员"}</small></span>{managed.isInitialAdministrator && !g.initial && <button className="text-button" disabled={busy} onClick={() => void change(g.accountId, false)}>撤销管理权</button>}</div>)}</div>{managed.isInitialAdministrator && <form className="social-form" onSubmit={event => { event.preventDefault(); void change(candidate, true) }}><label>授予管理员<select aria-label="选择管理员账号" value={candidate} onChange={e => setCandidate(e.target.value)} disabled={busy}><option value="">选择已有账号</option>{available.map(a => <option key={a.id} value={a.id}>{a.name} · {a.kind === "ai" ? "AI" : "真人"}</option>)}</select></label><button className="social-button secondary" disabled={busy || !available.some(a => a.id === candidate)}>授予管理权</button></form>}</section>
}

function AccountLifecycle({ managed, reload }: { managed: ManagedAi; reload(): Promise<void> }) {
  const state = useLiveSocialStore()
  const [confirming, setConfirming] = useState(false)
  const { busy, execute } = useVersionedAction()
  const stopped = managed.account.status === "archived"
  const change = async () => {
    const action = stopped ? "restore" : "archive"
    const result = await execute(action, managed.account.revision, input => socialClient.controlAccount(managed.account.id, { ...input, action }))
    if (result) { setConfirming(false); await state.action(reload); await state.action(state.refresh) }
  }
  return <section><h2>账号运行</h2><p>{stopped ? "账号已停用，所有数据保留。恢复后只响应新消息。" : "停用会停止这个 AI 在所有聊天中的回复与排队接话，账号、关系和历史保留。"}</p>{stopped ? <button className="social-button primary" disabled={busy} onClick={() => void change()}>恢复 AI 账号</button> : confirming ? <div className="live-control-buttons"><span>确认停用 {managed.account.name}？</span><button className="social-button secondary" disabled={busy} onClick={() => setConfirming(false)}>取消</button><button className="social-button secondary" disabled={busy} onClick={() => void change()}>确认停用账号</button></div> : <button className="social-button secondary" onClick={() => setConfirming(true)}>停用 AI 账号</button>}</section>
}

export function AiManagement({ id }: { id: string | null }) {
  const state = useLiveSocialStore()
  const [managed, setManaged] = useState<ManagedAi[]>([])
  const [editing, setEditing] = useState<ManagedAi | "new" | null>(null)
  const [loading, setLoading] = useState(true)
  const accountVersions = state.snapshot!.accounts.map(a => `${a.id}:${a.revision}`).join(",")
  const reload = async () => { setManaged(await socialClient.managedAccounts()) }
  useEffect(() => {
    let current = true
    void state.action(async () => { try { const result = await socialClient.managedAccounts(); if (current) setManaged(result) } catch (error) { if (current) setManaged([]); throw error } finally { if (current) setLoading(false) } })
    return () => { current = false }
  }, [accountVersions, state.action])
  const selected = managed.find(m => m.account.id === id)
  return <section className="account-management"><header className="contact-header"><button className="text-button" onClick={() => navigate("account")}><ArrowLeft />我的账号</button><strong>AI 角色管理</strong><button className="social-button primary" onClick={() => { state.dismissError(); setEditing("new") }}><Plus />创建 AI 账号</button></header><div className="management-scroll"><div className="managed-account-list">{managed.map(m => <button key={m.account.id} className={`contact-list-item ${id === m.account.id ? "selected" : ""}`} onClick={() => navigate("ai-management", m.account.id)}><Avatar contact={contactAvatar(m.account)} /><span><strong>{m.account.name}</strong><small>{m.account.status === "active" ? "账号正常" : "账号已停用"} · {m.isInitialAdministrator ? "初始管理员" : "管理员"}</small></span></button>)}</div>{selected ? <div className="profile-sections managed-account-details"><section><h1>{selected.account.name}</h1><div className="live-control-buttons"><button className="social-button secondary" onClick={() => navigate("contacts", selected.account.id)}>查看公开资料</button><button className="social-button secondary" onClick={() => { state.dismissError(); setEditing(selected) }}><Pencil />编辑设定与模型</button><button className="social-button primary" onClick={() => navigate("account-view", selected.account.id)}>登录 AI 账号查看</button></div><p className="form-note">以只读视角查看它的聊天与通讯录，实际操作者仍是你的账号。</p></section><section><h2>内部角色设定</h2><p className="persona-text">{selected.persona}</p></section><section><h2>回复模型</h2><p>{selected.binding ? `${selected.binding.providerId} / ${selected.binding.modelId}` : "尚未配置"}</p>{selected.binding?.reasoningEffort && <p>思考强度：{selected.binding.reasoningEffort}</p>}</section><AdministratorControls key={selected.account.id} managed={selected} reload={reload} /><AccountLifecycle key={selected.account.id} managed={selected} reload={reload} /></div> : <EmptyState title={loading ? "正在读取管理权限…" : id ? "没有这个账号的管理权限" : "你管理的 AI 账号"} description={loading ? "" : id ? "请重新选择，或联系它的初始管理员。" : "创建账号后，你会成为它的初始管理员。"}><button className="social-button secondary" onClick={() => void state.action(reload)}><RefreshCw />刷新列表</button></EmptyState>}</div><Dialog open={!!editing} onOpenChange={open => { if (!open) setEditing(null) }}><DialogContent className="social-dialog"><DialogTitle>{editing === "new" ? "创建 AI 账号" : "管理 AI 账号"}</DialogTitle><DialogDescription>公开资料和内部设定分别保存，管理权独立于联系人关系。</DialogDescription>{state.error && <p role="alert" className="form-error">{state.error}</p>}{editing && <AiEditor key={editing === "new" ? "new" : editing.account.id} managed={editing === "new" ? undefined : editing} models={state.snapshot!.models} done={accountId => navigate("ai-management", accountId)} close={() => setEditing(null)} />}</DialogContent></Dialog></section>
}

export function AccountViewer({ id }: { id: string }) {
  const state = useLiveSocialStore()
  const [view, setView] = useState<AccountView | null>(null)
  const [selected, setSelected] = useState<string | null>(null)
  const [page, setPage] = useState<MessagePage | null>(null)
  const [loading, setLoading] = useState(false)
  const [refresh, setRefresh] = useState(0)
  const [paging, setPaging] = useState(false)
  const currentView = useRef({ selected, view })
  currentView.current = { selected, view }
  useEffect(() => {
    let current = true
    setLoading(true)
    void state.action(async () => {
      try { const result = await socialClient.accountView(id); if (current) { setView(result); setSelected(old => result.snapshot.conversations.some(c => c.id === old) ? old : result.snapshot.conversations[0]?.id ?? null) } }
      catch (error) { if (current) { setView(null); setPage(null) }; throw error }
      finally { if (current) setLoading(false) }
    })
    return () => { current = false }
  }, [id, refresh, state.action])
  useEffect(() => {
    let current = true
    setPage(null)
    if (selected && view) void state.action(async () => { const result = await socialClient.accountViewMessages(id, selected); if (current) setPage(result) })
    return () => { current = false }
  }, [id, selected, view, state.action])
  const loadMore = async () => {
    if (!selected || !page?.messages.length || paging) return
    const version = currentView.current
    setPaging(true)
    try {
      const result = await socialClient.accountViewMessages(id, selected, page.messages[0].sequence)
      if (currentView.current.selected === version.selected && currentView.current.view === version.view)
        setPage(previous => previous ? { messages: [...result.messages, ...previous.messages], hasMore: result.hasMore } : result)
    } finally { setPaging(false) }
  }
  const snapshot = view?.snapshot
  const title = (conversationId: string) => {
    const c = snapshot!.conversations.find(c => c.id === conversationId)!
    return c.kind === "group" ? c.title : snapshot!.accounts.find(a => a.id !== snapshot!.selfId && c.memberIds.includes(a.id))?.name ?? c.title
  }
  return <section className="account-viewer"><header className="contact-header"><button className="text-button" onClick={() => navigate("ai-management", id)}><ArrowLeft />返回我的账号</button><strong>{snapshot?.selfName ?? "AI 账号"} · 只读视角</strong><button className="text-button" disabled={loading} onClick={() => setRefresh(n => n + 1)}><RefreshCw />刷新</button></header><div className="account-view-notice">管理员查看 · 实际操作者：{state.snapshot!.accounts.find(a => a.id === view?.operatorId)?.name ?? state.snapshot!.selfName}。此视角不会发送消息或改变 AI 的运行。</div>{snapshot ? <div className="account-view-columns"><aside><h2>消息</h2>{snapshot.conversations.map(c => <button className={`contact-list-item ${selected === c.id ? "selected" : ""}`} key={c.id} onClick={() => setSelected(c.id)}><span><strong>{title(c.id)}</strong><small>{c.kind === "group" ? "群聊" : "私聊"} · {c.status === "archived" ? "已归档" : "记录保留"}</small></span></button>)}<h2>通讯录</h2>{snapshot.addressBook.filter(e => e.listed).map(e => { const a = snapshot.accounts.find(a => a.id === e.accountId)!; return <div className="live-member-row" key={a.id}><Avatar contact={contactAvatar(a)} size="small" /><span>{a.name}<small>{a.bio || "还没有填写简介"}</small></span></div> })}</aside><div className="chat-timeline">{selected ? <><h2>{title(selected)}</h2>{page?.hasMore && <button className="social-button secondary" disabled={paging} onClick={() => void state.action(loadMore)}>{paging ? "加载中…" : "加载更早的消息"}</button>}{page ? page.messages.map(m => <article className={`live-message ${m.senderId === snapshot.selfId ? "own" : ""}`} key={m.id}><Avatar contact={contactAvatar(snapshot.accounts.find(a => a.id === m.senderId))} size="small" /><div className="live-message-content"><div className="live-message-label"><span>{m.senderName}</span><time>{shortTime(m.createdAt)}</time></div><div className="live-bubble">{m.status === "withdrawn" ? "消息已撤回" : m.text ? <MarkdownView content={m.text} /> : "本次回复没有正文"}</div>{m.status === "interrupted" && <p className="live-interruption">回复已中断，片段保留。</p>}</div></article>) : <p>正在读取聊天记录…</p>}</> : <EmptyState title="还没有聊天" description="这里只显示这个 AI 参与的聊天窗口。" />}</div></div> : <EmptyState title={loading ? "正在进入账号视角…" : "无法进入账号视角"} description="需要该 AI 的管理员权限。" />}</section>
}
