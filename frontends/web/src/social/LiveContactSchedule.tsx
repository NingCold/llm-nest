import { useState, type FormEvent } from "react"
import { CalendarClock, Settings2 } from "lucide-react"
import { Dialog, DialogContent, DialogDescription, DialogTitle } from "@/components/ui/dialog"
import { socialClient, SocialApiError } from "./client"
import { useLiveSocialStore } from "./live-store"
import { useVersionedAction } from "./LiveControls"
import type { ContactIntent, ContactIntentStatus, ContactSettings, Conversation } from "./live-types"

const statusNames: Record<ContactIntentStatus, string> = { pending: "等待到期", claimed: "排队中", executing: "正在联系", delivered: "已送达", failed: "联系失败", cancelled: "已取消", expired: "已过期", skipped: "已跳过", uncertain: "未确认送达 · 不会重试" }
const active = (i: ContactIntent) => ["pending", "claimed", "executing"].includes(i.status)
const pad = (n: number) => String(n).padStart(2, "0")
export function localDateTime(date: Date) { return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}T${pad(date.getHours())}:${pad(date.getMinutes())}` }
export function offsetLabel(minutes: number) { return `UTC${minutes >= 0 ? "+" : "-"}${pad(Math.floor(Math.abs(minutes) / 60))}:${pad(Math.abs(minutes) % 60)}` }
export function explicitTime(value: string) {
  const date = new Date(value)
  if (!Number.isFinite(date.getTime()) || localDateTime(date) !== value) throw new SocialApiError("请选择有效的本地日期与时间", "invalid_input")
  return `${value}:00${offsetLabel(-date.getTimezoneOffset()).slice(3)}`
}
function formatTime(value: string, offset: number) { return `${new Date(Date.parse(value) + offset * 60000).toISOString().slice(0, 16).replace("T", " ")} ${offsetLabel(offset)}` }

function SettingsForm({ conversation, initial, done }: { conversation: Conversation; initial: ContactSettings; done(): void }) {
  const [enabled, setEnabled] = useState(initial.enabled)
  const [quiet, setQuiet] = useState(initial.quietUntil && Date.parse(initial.quietUntil) > Date.now() ? localDateTime(new Date(initial.quietUntil)) : "")
  const [mode, setMode] = useState(initial.quietAction)
  const state = useLiveSocialStore(), { busy, execute } = useVersionedAction()
  const save = async (e: FormEvent) => {
    e.preventDefault()
    const result = await execute(JSON.stringify([enabled, quiet, mode]), initial.revision, input => socialClient.updateContactSettings(conversation.id, { requestId: input.requestId, settings: { revision: input.expectedRevision, enabled, quietUntil: quiet ? explicitTime(quiet) : null, quietAction: mode } }))
    if (result) { done(); await state.action(state.refresh) }
  }
  return <form className="social-form" onSubmit={e => void save(e)}>{state.error && <p className="form-error" role="alert">{state.error}</p>}<fieldset className="social-form-fields" disabled={busy}>
    <label className="summary-toggle"><input type="checkbox" checked={enabled} onChange={e => setEnabled(e.target.checked)} />允许本聊天的一次性约定联系</label>
    <p className="form-note">每次约定都由你明确创建，到期会调用所选 AI 的模型。关闭授权会取消所有待完成约定并停止正在进行的约定联系。</p>
    <label>约定联系免打扰至<input type="datetime-local" value={quiet} onChange={e => setQuiet(e.target.value)} /></label>
    <p className="form-note">使用本机时区 {Intl.DateTimeFormat().resolvedOptions().timeZone}，按所选日期的 UTC 偏移保存。清空时间即可关闭；免打扰只影响约定联系，正常消息仍可回复。</p>
    <label>免打扰期间到期<select value={mode} onChange={e => setMode(e.target.value as "defer" | "skip")}><option value="defer">延后到免打扰结束</option><option value="skip">直接跳过本次约定</option></select></label>
    <p className="form-note">延后仍受失效时间限制。开启免打扰会停止已经开始的约定联系，不会重新发送。</p>
    <div className="dialog-actions"><button type="button" className="social-button secondary" onClick={done}>取消</button><button className="social-button primary">{busy ? "保存中…" : "保存联系设置"}</button></div>
  </fieldset></form>
}
function IntentForm({ conversation, done }: { conversation: Conversation; done(): void }) {
  const state = useLiveSocialStore(), { busy, execute } = useVersionedAction()
  const accounts = (state.snapshot?.accounts ?? []).filter(a => a.kind === "ai" && a.status === "active" && conversation.members.some(m => m.actorId === a.id && !m.paused))
  const [actorId, setActor] = useState(accounts[0]?.id ?? ""), [text, setText] = useState("")
  const [due, setDue] = useState(localDateTime(new Date(Date.now() + 3600000))), [expires, setExpires] = useState(localDateTime(new Date(Date.now() + 7200000)))
  const save = async (e: FormEvent) => {
    e.preventDefault()
    const result = await execute(JSON.stringify([actorId, text, due, expires]), "1", input => socialClient.createContactIntent(conversation.id, { requestId: input.requestId, actorId, text, dueAt: explicitTime(due), expiresAt: explicitTime(expires) }))
    if (result) { done(); await state.action(state.refresh) }
  }
  return <form className="social-form" onSubmit={e => void save(e)}>{state.error && <p className="form-error" role="alert">{state.error}</p>}<fieldset className="social-form-fields" disabled={busy}>
    <label>联系你的 AI<select required value={actorId} onChange={e => setActor(e.target.value)}>{accounts.map(a => <option key={a.id} value={a.id}>{a.name}</option>)}</select></label>
    <label>约定内容<textarea required maxLength={1000} rows={3} value={text} onChange={e => setText(e.target.value)} placeholder="例如：到时来问问我，今天的阅读计划进行得怎么样" /></label>
    <div className="participation-limits"><label>联系时间<input required type="datetime-local" value={due} onChange={e => setDue(e.target.value)} /></label><label>失效时间<input required type="datetime-local" value={expires} onChange={e => setExpires(e.target.value)} /></label></div>
    <p className="form-note">本机时区 {Intl.DateTimeFormat().resolvedOptions().timeZone}，所选时间 {due && Number.isFinite(new Date(due).getTime()) ? offsetLabel(-new Date(due).getTimezoneOffset()) : ""}。仅执行一次，失效后不补发。应用关闭时不会运行；下次打开只处理仍在有效期内、尚未开始的约定。</p>
    <p className="form-note">到期会在本窗口发一条 AI 消息，使用当前模型与意识上下文，并占用本窗口的主动调用额度。</p>
    <div className="dialog-actions"><button type="button" className="social-button secondary" onClick={done}>取消</button><button className="social-button primary" disabled={!actorId || !text.trim()}>{busy ? "保存中…" : "授权并创建约定"}</button></div>
  </fieldset></form>
}
function IntentRow({ intent, name }: { intent: ContactIntent; name: string }) {
  const state = useLiveSocialStore(), { busy, execute } = useVersionedAction()
  const cancel = async () => {
    const result = await execute(intent.id, intent.revision, input => socialClient.cancelContactIntent(intent.conversationId, intent.id, input))
    if (result) await state.action(state.refresh)
  }
  return <article className="contact-intent"><div><strong>{name}</strong><span role="status">{statusNames[intent.status]}</span></div><p>{intent.text}</p><small>{formatTime(intent.dueAt, intent.utcOffsetMinutes)}<br />有效至 {formatTime(intent.expiresAt, intent.utcOffsetMinutes)}</small>{intent.reason && <small>{intent.reason}</small>}{active(intent) && <button className="social-button secondary" disabled={busy} onClick={() => void cancel()}>取消约定</button>}</article>
}
export function ContactSchedulePanel({ conversation }: { conversation: Conversation }) {
  const [editing, setEditing] = useState<ContactSettings | null>(null), [creating, setCreating] = useState(false)
  const state = useLiveSocialStore(), { settings, intents } = conversation.contactSchedule
  if (conversation.legacy) return null
  const running = intents.filter(active), history = intents.filter(i => !active(i))
  const row = (i: ContactIntent) => <IntentRow key={i.id} intent={i} name={state.snapshot?.accounts.find(a => a.id === i.actorId)?.name ?? "AI"} />
  return <section className="live-conversation-controls"><h3>约定联系</h3><p>{settings.enabled ? "已允许你明确创建的一次性约定" : "未授权主动联系"}</p>
    {settings.quietUntil && Date.parse(settings.quietUntil) > Date.now() && <p className="form-note">免打扰至 {new Date(settings.quietUntil).toLocaleString()}（本机时区）· {settings.quietAction === "defer" ? "到期延后" : "到期跳过"}</p>}
    <div className="live-control-buttons"><button className="social-button secondary" disabled={conversation.status === "archived"} onClick={() => setEditing(settings)}><Settings2 />联系设置</button><button className="social-button secondary" disabled={!settings.enabled || conversation.paused || conversation.status === "archived"} onClick={() => setCreating(true)}><CalendarClock />新建约定</button></div>
    <p className="form-note">结束当前交谈保留未来约定。到期时暂停会跳过，AI 停用或退出会取消。失败和结果不确定的联系不会自动重试。</p>
    {running.map(row)}{history.length > 0 && <details className="live-plan-history"><summary>最近的约定记录（{history.length}）</summary>{history.map(row)}</details>}
    <Dialog open={!!editing || creating} onOpenChange={open => { if (!open) { setEditing(null); setCreating(false) } }}><DialogContent className="social-dialog"><DialogTitle>{creating ? "一次性约定联系" : "约定联系与免打扰"}</DialogTitle><DialogDescription>仅适用于这个聊天窗口，所有约定都可以查看和取消。</DialogDescription>{editing && <SettingsForm conversation={conversation} initial={editing} done={() => setEditing(null)} />}{creating && <IntentForm conversation={conversation} done={() => setCreating(false)} />}</DialogContent></Dialog>
  </section>
}
