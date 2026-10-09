import { useRef, useState } from "react"
import { Pencil, X } from "lucide-react"
import { SocialApiError, socialClient } from "./client"
import { useLiveSocialStore } from "./live-store"
import type { Contact, ControlAction, Conversation, Membership, Message, VersionedCommand } from "./live-types"

// Keep the same command and expected version when an acknowledgement is lost.
function useVersionedAction() {
  const [busy, setBusy] = useState(false)
  const pending = useRef<{ key: string; input: VersionedCommand } | null>(null)
  const state = useLiveSocialStore()
  const execute = async <T,>(key: string, revision: string, operation: (input: VersionedCommand) => Promise<T>) => {
    if (busy) return
    if (pending.current?.key !== key) pending.current = { key, input: { requestId: crypto.randomUUID(), expectedRevision: revision } }
    setBusy(true)
    const result = await state.action(async () => {
      try { return await operation(pending.current!.input) }
      catch (error) {
        if (error instanceof SocialApiError) pending.current = null
        throw error
      }
    })
    if (result !== undefined) pending.current = null
    setBusy(false)
    return result
  }
  return { busy, execute }
}

export function ConversationControls({ conversation }: { conversation: Conversation }) {
  const state = useLiveSocialStore()
  const { busy, execute } = useVersionedAction()
  const change = async (action: ControlAction) => {
    const result = await execute(action, conversation.revision, input => socialClient.control(conversation.id, { ...input, action }))
    if (result) await state.action(state.refresh)
  }
  if (conversation.legacy) return null
  const archived = conversation.status === "archived"
  return <section className="live-conversation-controls"><h3>交谈控制</h3>
    <p>{archived ? "已归档，聊天记录可继续查看。" : conversation.paused ? "已暂停接话，仍可发送和保存消息。" : conversation.currentBout ? "当前交谈进行中。" : "当前没有进行中的交谈，新消息可以开始下一次交谈。"}</p>
    <div className="live-control-buttons">{archived ? <button className="social-button secondary" disabled={busy} onClick={() => void change("restore")}>恢复聊天</button> : <>
      <button className="social-button secondary" disabled={busy} onClick={() => void change(conversation.paused ? "resume" : "pause")}>{conversation.paused ? "恢复接话" : "暂停全部 AI"}</button>
      <button className="social-button secondary" disabled={busy || (!conversation.currentBout && !conversation.pendingPlans.length)} onClick={() => void change("end")}>结束当前交谈</button>
      <button className="social-button secondary" disabled={busy} onClick={() => void change("archive")}>归档聊天</button>
    </>}</div>
    <p className="form-note">结束会停止当前回复和排队接话，保留记录；恢复接话后，新的消息才会触发回应。</p>
  </section>
}

export function ContactControls({ contact }: { contact: Contact }) {
  const state = useLiveSocialStore()
  const [confirming, setConfirming] = useState(false)
  const { busy, execute } = useVersionedAction()
  const archived = contact.status === "archived"
  const change = async (action: "archive" | "restore") => {
    const result = await execute(action, contact.revision, input => socialClient.controlContact(contact.id, { ...input, action }))
    if (result) { setConfirming(false); await state.action(state.refresh) }
  }
  return <section className="live-conversation-controls"><h2>{archived ? "已归档联系人" : "联系人管理"}</h2>
    <p>{archived ? "私聊与群聊记录保留。该 AI 在所有窗口中停止接话，恢复后只有新消息会触发回复。" : "移除会将联系人归档，停止它在所有窗口中的回复与排队接话。聊天记录、群成员身份和角色设定会保留。"}</p>
    {archived ? <button className="social-button primary" disabled={busy} onClick={() => void change("restore")}>{busy ? "恢复中…" : "恢复联系人"}</button>
      : confirming ? <div className="live-control-buttons"><span>确认移除并归档 {contact.name}？</span><button className="social-button secondary" disabled={busy} onClick={() => setConfirming(false)}>取消</button><button className="social-button secondary" disabled={busy} onClick={() => void change("archive")}>{busy ? "归档中…" : "确认移除并归档"}</button></div>
        : <button className="social-button secondary" disabled={busy} onClick={() => setConfirming(true)}>移除联系人</button>}
    <p className="form-note">聊天窗口的归档和成员暂停设置分别保存，恢复联系人不会改变这些设置。</p>
  </section>
}

export function MemberParticipation({ conversation, member }: { conversation: Conversation; member: Membership }) {
  const state = useLiveSocialStore()
  const { busy, execute } = useVersionedAction()
  const archived = state.snapshot?.contacts.find(c => c.id === member.actorId)?.status === "archived"
  const toggle = async () => {
    const result = await execute(`pause:${!member.paused}`, member.revision, input => socialClient.pauseMember(conversation.id, { ...input, actorId: member.actorId, paused: !member.paused }))
    if (result) await state.action(state.refresh)
  }
  return <button className="live-member-participation" disabled={busy || archived} onClick={() => void toggle()}>{archived ? "联系人已归档" : member.paused ? "恢复接话" : "暂停接话"}</button>
}

export function OwnMessageControls({ message }: { message: Message }) {
  const state = useLiveSocialStore()
  const [editing, setEditing] = useState(false)
  const [text, setText] = useState(message.text)
  const [revision, setRevision] = useState(message.revision)
  const [confirming, setConfirming] = useState(false)
  const { busy, execute } = useVersionedAction()
  const save = async () => {
    const result = await execute(`edit:${text}`, revision, input => socialClient.revise(message.id, { ...input, text }))
    if (result) { state.applyMessage(result); setEditing(false); await state.action(state.refresh) }
  }
  const withdraw = async () => {
    const result = await execute("withdraw", message.revision, input => socialClient.withdraw(message.id, input))
    if (result) { state.applyMessage(result); setConfirming(false); await state.action(state.refresh) }
  }
  if (editing) return <form className="live-message-editor" onSubmit={event => { event.preventDefault(); void save() }}><textarea aria-label="编辑消息正文" rows={3} maxLength={20000} disabled={busy} value={text} onChange={event => setText(event.target.value)} /><p>保存修改会停止依赖旧内容的回复，后续聊天记录保留。</p><div><button type="button" disabled={busy} onClick={() => setEditing(false)}><X />取消编辑</button><button disabled={busy || !text.trim()}>保存修改</button></div></form>
  if (confirming) return <div className="live-withdraw-confirm"><p>撤回这条消息？正文和附件会移除，后续记录保留。</p><button disabled={busy} onClick={() => setConfirming(false)}>取消</button><button disabled={busy} onClick={() => void withdraw()}>确认撤回</button></div>
  return <div className="live-message-actions"><button onClick={() => { setText(message.text); setRevision(message.revision); setEditing(true) }}><Pencil />编辑</button><button onClick={() => setConfirming(true)}>撤回</button>{message.revision !== "1" && <small>已编辑</small>}</div>
}
