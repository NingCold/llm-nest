import { useState, type FormEvent } from "react"
import { Check, Plus, Users } from "lucide-react"
import { Dialog, DialogContent, DialogDescription, DialogTitle } from "@/components/ui/dialog"
import { useSocialStore } from "../store"
import { navigate } from "../router"
import type { AiContact, AvatarColor, ContactInput, MemoryRecord, MemoryScope, SocialMessage } from "../types"
import { Avatar } from "./common"

export type Editor =
  | { kind: "contact"; contact?: AiContact }
  | { kind: "group" }
  | { kind: "memory"; scope: MemoryScope; record?: MemoryRecord; source?: SocialMessage }
  | { kind: "message"; message: SocialMessage }
  | { kind: "confirm"; title: string; description: string; confirm: () => Promise<boolean> }
  | null

function ContactForm({ existing, close }: { existing?: AiContact; close(): void }) {
  const action = useSocialStore(s => s.action)
  const [name, setName] = useState(existing?.name ?? "")
  const [tagline, setTagline] = useState(existing?.tagline ?? "")
  const [persona, setPersona] = useState(existing?.persona ?? "")
  const [interests, setInterests] = useState(existing?.interests.join("、") ?? "")
  const [color, setColor] = useState<AvatarColor>(existing?.color ?? "sage")
  const [style, setStyle] = useState<ContactInput["responseStyle"]>(existing?.responseStyle ?? "natural")
  const [execution, setExecution] = useState<ContactInput["execution"]>(existing?.execution ?? "preview")
  const [saving, setSaving] = useState(false)
  const submit = async (event: FormEvent) => {
    event.preventDefault(); setSaving(true)
    const input: ContactInput = { name, avatar: name.trim().slice(0, 1) || "友", color, tagline, persona, interests: [...new Set(interests.split(/[、,，]/).map(t => t.trim()).filter(Boolean))], responseStyle: style, execution, allowProactive: existing?.allowProactive ?? false }
    const result = await action(async api => {
      if (existing) { await api.updateContact(existing.id, input, existing.revision); return existing }
      return api.createContact(input)
    })
    setSaving(false)
    if (result) { navigate("contacts", result.id); close() }
  }
  return <form className="social-form" onSubmit={event => void submit(event)}>
    <div className="avatar-picker"><Avatar contact={{ name, avatar: name.slice(0, 1) || "友", color } as AiContact} size="large" /><div><p>给这位朋友一种颜色</p><div className="color-options">{(["sage", "clay", "blue", "lavender"] as const).map(tone => <button key={tone} type="button" className={`color-option avatar-${tone}`} aria-label={`选择${({ sage: "青绿", clay: "陶土", blue: "蓝色", lavender: "紫色" })[tone]}头像`} aria-pressed={color === tone} onClick={() => setColor(tone)}>{color === tone && <Check />}</button>)}</div></div></div>
    <label>名字<input autoFocus required maxLength={40} value={name} onChange={e => setName(e.target.value)} placeholder="你想怎么称呼这位朋友？" /></label>
    <label>一句话介绍<input maxLength={120} value={tagline} onChange={e => setTagline(e.target.value)} placeholder="让你一眼想起对方的那句话" /></label>
    <label>角色设定<textarea required maxLength={2000} rows={4} value={persona} onChange={e => setPersona(e.target.value)} placeholder="描述性格、背景，以及你希望如何相处。" /></label>
    <label>兴趣标签<input value={interests} onChange={e => setInterests(e.target.value)} placeholder="电影、散步、写作" /></label>
    <div className="form-columns"><label>交流风格<select value={style} onChange={e => setStyle(e.target.value as ContactInput["responseStyle"])}><option value="natural">自然交流</option><option value="concise">简洁清晰</option><option value="thoughtful">细腻展开</option></select></label><label>回复配置<select value={execution} onChange={e => setExecution(e.target.value as ContactInput["execution"])}><option value="preview">模拟回复</option><option value="unconfigured">暂未配置</option></select></label></div>
    <p className="form-note">当前是 Web 交互预览，模型与工具的真实配置在接入后端后启用。</p>
    <div className="dialog-actions"><button type="button" className="social-button secondary" onClick={close}>取消</button><button className="social-button primary" disabled={saving}>{saving ? "保存中…" : existing ? "保存修改" : "添加联系人"}</button></div>
  </form>
}

function GroupForm({ close }: { close(): void }) {
  const contacts = useSocialStore(s => s.snapshot!.contacts)
  const action = useSocialStore(s => s.action)
  const [title, setTitle] = useState("")
  const [selected, setSelected] = useState<string[]>([])
  const [saving, setSaving] = useState(false)
  return <form className="social-form" onSubmit={event => {
    event.preventDefault(); setSaving(true)
    void action(api => api.createGroup(title, selected)).then(result => { setSaving(false); if (result) { navigate("messages", result.id); close() } })
  }}>
    <label>群聊名字<input autoFocus required maxLength={60} value={title} onChange={e => setTitle(e.target.value)} placeholder="为共同的话题取个名字" /></label>
    <p className="field-heading">邀请 AI 联系人 <span>已选择 {selected.length} 位</span></p>
    <div className="group-selection">{contacts.map(contact => <label key={contact.id} className={selected.includes(contact.id) ? "selected" : ""}><Avatar contact={contact} /><span><strong>{contact.name}</strong><small>{contact.tagline}</small></span><input type="checkbox" checked={selected.includes(contact.id)} onChange={e => setSelected(e.target.checked ? [...selected, contact.id] : selected.filter(id => id !== contact.id))} /></label>)}</div>
    <p className="form-note"><Users />新群默认只在 @、回复或明确邀请时由 AI 接话。</p>
    <div className="dialog-actions"><button type="button" className="social-button secondary" onClick={close}>取消</button><button className="social-button primary" disabled={saving || !selected.length}><Plus />{saving ? "创建中…" : "创建群聊"}</button></div>
  </form>
}

function TextForm({ editor, close }: { editor: Extract<NonNullable<Editor>, { kind: "memory" | "message" }>; close(): void }) {
  const action = useSocialStore(s => s.action)
  const [text, setText] = useState(editor.kind === "message" ? editor.message.text : editor.record?.text ?? editor.source?.text.slice(0, 1200) ?? "")
  const [saving, setSaving] = useState(false)
  return <form className="social-form" onSubmit={event => {
    event.preventDefault(); setSaving(true)
    void action(async api => {
      if (editor.kind === "message") await api.editMessage(editor.message.id, text, editor.message.revision)
      else await api.saveMemory(editor.scope, text, editor.source?.id, editor.record?.id, editor.record?.revision)
      return true
    }).then(ok => { setSaving(false); if (ok) close() })
  }}>
    <label>{editor.kind === "memory" ? "希望记住的内容" : "消息内容"}<textarea autoFocus required rows={6} maxLength={editor.kind === "memory" ? 1200 : 20_000} value={text} onChange={e => setText(e.target.value)} /></label>
    {editor.kind === "memory" && <p className="form-note">{editor.scope.kind === "relationship" ? "只保存在你与这位 AI 的关系记忆中。" : "保存在当前群聊的共同记忆中。"}{editor.source && "来源会关联到所选消息。"}</p>}
    <div className="dialog-actions"><button type="button" className="social-button secondary" onClick={close}>取消</button><button className="social-button primary" disabled={saving}>{saving ? "保存中…" : "保存"}</button></div>
  </form>
}

function ConfirmForm({ editor, close }: { editor: Extract<NonNullable<Editor>, { kind: "confirm" }>; close(): void }) {
  const [saving, setSaving] = useState(false)
  return <div className="dialog-actions"><button className="social-button secondary" onClick={close}>取消</button><button className="social-button danger" disabled={saving} onClick={() => { setSaving(true); void editor.confirm().then(ok => { setSaving(false); if (ok) close() }) }}>{saving ? "处理中…" : "确认"}</button></div>
}

export function EditorDialogs({ editor, close }: { editor: Editor; close(): void }) {
  const error = useSocialStore(s => s.error)
  const title = editor?.kind === "contact" ? editor.contact ? "编辑联系人" : "添加 AI 联系人"
    : editor?.kind === "group" ? "创建群聊" : editor?.kind === "memory" ? editor.record ? "编辑记忆" : "添加记忆"
    : editor?.kind === "message" ? "编辑消息" : editor?.title ?? ""
  return <Dialog open={!!editor} onOpenChange={open => { if (!open) close() }}><DialogContent className="social-dialog">
    <DialogTitle>{title}</DialogTitle>
    <DialogDescription>{editor?.kind === "confirm" ? editor.description : editor?.kind === "contact" ? "稳定的身份，让每次聊天都有熟悉的起点。" : editor?.kind === "group" ? "让几位朋友围绕同一个话题相遇。" : "内容与来源会保存在当前浏览器的预览数据中。"}</DialogDescription>
    {error && <p role="alert" className="dialog-error">{error}</p>}
    {editor?.kind === "contact" && <ContactForm existing={editor.contact} close={close} />}
    {editor?.kind === "group" && <GroupForm close={close} />}
    {(editor?.kind === "memory" || editor?.kind === "message") && <TextForm editor={editor} close={close} />}
    {editor?.kind === "confirm" && <ConfirmForm editor={editor} close={close} />}
  </DialogContent></Dialog>
}
