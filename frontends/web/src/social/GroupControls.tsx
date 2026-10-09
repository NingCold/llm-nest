import { useRef, useState, type FormEvent } from "react"
import { Plus, X } from "lucide-react"
import { Avatar } from "./components/common"
import { socialClient } from "./client"
import { useLiveSocialStore } from "./live-store"
import { contactAvatar } from "./live-ui"
import { navigate } from "./router"
import type { Contact, Conversation } from "./live-types"

export function CreateGroupForm({ contacts, close }: { contacts: Contact[]; close(): void }) {
  const state = useLiveSocialStore()
  const [title, setTitle] = useState("")
  const [members, setMembers] = useState<string[]>([])
  const [saving, setSaving] = useState(false)
  const requestId = useRef(crypto.randomUUID())
  const toggle = (id: string) => { setMembers(ids => ids.includes(id) ? ids.filter(m => m !== id) : [...ids, id]); requestId.current = crypto.randomUUID() }
  const save = async (event: FormEvent) => {
    event.preventDefault(); setSaving(true)
    const id = await state.action(() => socialClient.createGroup({ requestId: requestId.current, title, memberIds: members }))
    if (id) { await state.action(state.refresh); navigate("messages", id); close() }
    setSaving(false)
  }
  return <form className="social-form" onSubmit={event => void save(event)}><fieldset className="social-form-fields" disabled={saving}>
    <label>群聊名称<input autoFocus required maxLength={80} value={title} onChange={event => { setTitle(event.target.value); requestId.current = crypto.randomUUID() }} placeholder="给这个空间起个名字" /></label>
    <div className="live-contact-choices" aria-label="选择群成员">{contacts.map(contact => <label key={contact.id}><input type="checkbox" checked={members.includes(contact.id)} onChange={() => toggle(contact.id)} /><Avatar contact={contactAvatar(contact)} size="small" /><span>{contact.name}</span></label>)}</div>
    <p className="form-note">你会自动加入。每个群拥有独立记录；成员使用原来的 AI 身份和模型。发送时 @ 选择接话对象。</p>
    <div className="dialog-actions"><button type="button" className="social-button secondary" onClick={close}>取消</button><button className="social-button primary" disabled={!members.length}>{saving ? "创建中…" : "创建群聊"}</button></div>
  </fieldset></form>
}

export function GroupMembers({ conversation }: { conversation: Conversation }) {
  const state = useLiveSocialStore()
  const snapshot = state.snapshot!
  const [adding, setAdding] = useState(false)
  const [actor, setActor] = useState("")
  const [history, setHistory] = useState<"from_now" | "all">("from_now")
  const [saving, setSaving] = useState(false)
  const requestId = useRef(crypto.randomUUID())
  const expectedRevision = useRef<string | null>(null)
  const available = snapshot.contacts.filter(c => !conversation.memberIds.includes(c.id))
  const change = async (actorId: string, visibility: "from_now" | "all" | null) => {
    setSaving(true)
    expectedRevision.current ??= conversation.revision
    const result = await state.action(() => socialClient.changeMembership(conversation.id, { requestId: requestId.current, actorId, history: visibility, expectedRevision: expectedRevision.current! }))
    if (result) { await state.action(state.refresh); setAdding(false); setActor(""); requestId.current = crypto.randomUUID(); expectedRevision.current = null }
    setSaving(false)
  }
  return <section className="live-group-members"><h3>群成员 · {conversation.memberIds.length}</h3>
    <div className="live-member-row"><Avatar self size="small" /><span>{snapshot.selfName}<small>你 · 群管理者</small></span></div>
    {conversation.members.filter(m => m.actorId !== snapshot.selfId).map(member => {
      const contact = snapshot.contacts.find(c => c.id === member.actorId)
      return <div className="live-member-row" key={member.actorId}><button onClick={() => navigate("contacts", member.actorId)}><Avatar contact={contactAvatar(contact)} size="small" /><span>{contact?.name}<small>{member.historyFromSeq === "1" ? "可见全部群记录" : `可见第 ${member.historyFromSeq} 条起的记录`}</small></span></button><button className="live-remove-member" aria-label={`移出 ${contact?.name}`} disabled={saving} onClick={() => { requestId.current = crypto.randomUUID(); expectedRevision.current = null; void change(member.actorId, null) }}><X /></button></div>
    })}
    {!adding ? <button className="social-button secondary" disabled={!available.length || saving} onClick={() => setAdding(true)}><Plus />添加成员</button> : <form className="social-form" onSubmit={event => { event.preventDefault(); void change(actor, history) }}><fieldset className="social-form-fields" disabled={saving}>
      <label>新成员<select required value={actor} onChange={event => { setActor(event.target.value); requestId.current = crypto.randomUUID(); expectedRevision.current = null }}><option value="">选择 AI 联系人</option>{available.map(c => <option key={c.id} value={c.id}>{c.name}</option>)}{actor && !available.some(c => c.id === actor) && <option value={actor}>{snapshot.contacts.find(c => c.id === actor)?.name}</option>}</select></label>
      <label>历史可见范围<select value={history} onChange={event => { setHistory(event.target.value as typeof history); requestId.current = crypto.randomUUID(); expectedRevision.current = null }}><option value="from_now">仅加入后的消息</option><option value="all">允许读取全部群记录</option></select></label>
      <p className="form-note">成员变更会停止当前回复，并保留已生成片段。移出的 AI 不再接话；重新加入时重新授权历史范围。</p>
      <div className="dialog-actions"><button type="button" className="social-button secondary" onClick={() => setAdding(false)}>取消</button><button className="social-button primary" disabled={!actor}>加入群聊</button></div>
    </fieldset></form>}
  </section>
}

export function MentionChoices({ conversation }: { conversation: Conversation }) {
  const state = useLiveSocialStore()
  const draft = state.drafts[conversation.id]
  const mentions = draft?.mentions ?? []
  const contacts = state.snapshot!.contacts.filter(c => conversation.memberIds.includes(c.id))
  return <details className="live-mention-choices"><summary>@ 选择接话的 AI{mentions.length > 0 && ` · ${mentions.length} 位`}</summary><div className="live-contact-choices">{contacts.map(contact => {
    const index = mentions.indexOf(contact.id)
    return <label key={contact.id}><input type="checkbox" checked={index !== -1} onChange={() => state.targetDraft(conversation.id, index === -1 ? [...mentions, contact.id] : mentions.filter(id => id !== contact.id), draft?.replyTo ?? null)} /><Avatar contact={contactAvatar(contact)} size="small" /><span>{contact.name}{index !== -1 && <small>第 {index + 1} 位接话</small>}</span></label>
  })}{mentions.filter(id => !conversation.memberIds.includes(id)).map(id => <label key={id}><input type="checkbox" checked onChange={() => state.targetDraft(conversation.id, mentions.filter(m => m !== id), draft?.replyTo ?? null)} /><span>{state.snapshot!.contacts.find(c => c.id === id)?.name ?? "AI"}<small>已离开群聊，请取消选择</small></span></label>)}</div><p>按选择顺序回复。仅在正文里输入 @名字不会触发接话。</p></details>
}
