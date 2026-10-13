import { useRef, useState } from "react"
import { Pencil, X } from "lucide-react"
import { SocialApiError, socialClient } from "./client"
import { useLiveSocialStore } from "./live-store"
import type { Account, ContactEntry, ControlAction, Conversation, Membership, Message } from "./live-types"

// Keep the same command and expected version when an acknowledgement is lost.
export function useVersionedAction<R extends string | null = string>() {
  const [busy, setBusy] = useState(false)
  const pending = useRef<{ key: string; input: { requestId: string; expectedRevision: R } } | null>(null)
  const state = useLiveSocialStore()
  const execute = async <T,>(key: string, revision: R, operation: (input: { requestId: string; expectedRevision: R }) => Promise<T>) => {
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
    <p>{conversation.directBlocked ? "此私聊已屏蔽，解除屏蔽后可发送新消息。" : archived ? "已归档，聊天记录可继续查看。" : conversation.paused ? "已暂停接话，仍可发送和保存消息。" : conversation.currentBout ? "当前交谈进行中。" : "当前没有进行中的交谈，新消息可以开始下一次交谈。"}</p>
    <div className="live-control-buttons">{archived ? <button className="social-button secondary" disabled={busy} onClick={() => void change("restore")}>恢复聊天</button> : <>
      <button className="social-button secondary" disabled={busy} onClick={() => void change(conversation.paused ? "resume" : "pause")}>{conversation.paused ? "恢复接话" : "暂停全部 AI"}</button>
      <button className="social-button secondary" disabled={busy || (!conversation.currentBout && !conversation.pendingPlans.length)} onClick={() => void change("end")}>结束当前交谈</button>
      <button className="social-button secondary" disabled={busy} onClick={() => void change("archive")}>归档聊天</button>
    </>}</div>
    <p className="form-note">结束会停止当前回复和排队接话，保留记录与未来约定。暂停期间到期的约定会跳过。</p>
  </section>
}

export function ContactControls({ contact, entry }: { contact: Account; entry?: ContactEntry }) {
  const state = useLiveSocialStore()
  const [confirming, setConfirming] = useState(false)
  const { busy, execute } = useVersionedAction<string | null>()
  const listed = entry?.listed ?? false
  const change = async (listed: boolean) => {
    const result = await execute(`listed:${listed}`, entry?.revision ?? null, input => socialClient.setContact(contact.id, { ...input, listed }))
    if (result) { setConfirming(false); await state.action(state.refresh) }
  }
  return <section className="live-conversation-controls"><h2>联系人关系</h2>
    <p>移除只会从你的通讯录撤下。未屏蔽时仍可继续私聊与共同群聊；重新添加沿用原来的私聊窗口，并保留屏蔽设置。</p>
    {!listed ? <button className="social-button primary" disabled={busy} onClick={() => void change(true)}>{busy ? "添加中…" : "添加到通讯录"}</button>
      : confirming ? <div className="live-control-buttons"><span>从通讯录移除 {contact.name}？</span><button className="social-button secondary" disabled={busy} onClick={() => setConfirming(false)}>取消</button><button className="social-button secondary" disabled={busy} onClick={() => void change(false)}>{busy ? "移除中…" : "确认移除"}</button></div>
        : <button className="social-button secondary" disabled={busy} onClick={() => setConfirming(true)}>移除联系人</button>}
    <ContactBlockControls contact={contact} entry={entry} />
  </section>
}

export function ContactBlockControls({ contact, entry }: { contact: Account; entry?: ContactEntry }) {
  const state = useLiveSocialStore(), { busy, execute } = useVersionedAction<string | null>()
  const [confirming, setConfirming] = useState(false)
  const blocked = entry?.blocked ?? false
  const change = async () => {
    const result = await execute(`blocked:${!blocked}`, entry?.revision ?? null, input => socialClient.setContactBlock(contact.id, { ...input, blocked: !blocked }))
    if (result) { setConfirming(false); await state.action(state.refresh) }
  }
  return <section className="live-conversation-controls"><h3>{blocked ? "已屏蔽" : "屏蔽联系人"}</h3>
    <p>屏蔽后停止私聊发送与回复，取消你与对方尚未完成的约定联系。共同群仍可正常交流，历史与记忆保留。</p>
    {blocked ? <><p className="form-note">解除后可在原窗口继续私聊，旧回复与约定不会补发。</p><button className="social-button secondary" disabled={busy} onClick={() => void change()}>{busy ? "保存中…" : "解除屏蔽"}</button></>
      : confirming ? <div className="live-control-buttons"><span>确认屏蔽 {contact.name}？</span><button className="social-button secondary" disabled={busy} onClick={() => setConfirming(false)}>取消</button><button className="social-button secondary" disabled={busy} onClick={() => void change()}>{busy ? "保存中…" : "确认屏蔽"}</button></div>
      : <button className="social-button secondary" disabled={busy} onClick={() => setConfirming(true)}>屏蔽联系人</button>}
  </section>
}

export function MemberParticipation({ conversation, member }: { conversation: Conversation; member: Membership }) {
  const state = useLiveSocialStore()
  const { busy, execute } = useVersionedAction()
  const archived = state.snapshot?.accounts.find(c => c.id === member.actorId)?.status === "archived"
  const toggle = async () => {
    const result = await execute(`pause:${!member.paused}`, member.revision, input => socialClient.pauseMember(conversation.id, { ...input, actorId: member.actorId, paused: !member.paused }))
    if (result) await state.action(state.refresh)
  }
  return <button className="live-member-participation" disabled={busy || archived} onClick={() => void toggle()}>{archived ? "账号已停用" : member.paused ? "恢复接话" : "暂停接话"}</button>
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
  if (editing) return <form className="live-message-editor" onSubmit={event => { event.preventDefault(); void save() }}><textarea aria-label="编辑消息正文" rows={3} maxLength={20000} disabled={busy} value={text} onChange={event => setText(event.target.value)} /><p>保存修改会停止依赖旧内容的回复，后续聊天记录保留。</p><div><button type="button" disabled={busy} onClick={() => setEditing(false)}><X />取消编辑</button><button disabled={busy || (!text.trim() && !message.images?.length)}>保存修改</button></div></form>
  if (confirming) return <div className="live-withdraw-confirm"><p>撤回这条消息？正文和附件会移除，后续记录保留。</p><button disabled={busy} onClick={() => setConfirming(false)}>取消</button><button disabled={busy} onClick={() => void withdraw()}>确认撤回</button></div>
  return <div className="live-message-actions"><button onClick={() => { setText(message.text); setRevision(message.revision); setEditing(true) }}><Pencil />编辑</button><button onClick={() => setConfirming(true)}>撤回</button>{message.revision !== "1" && !message.contextReference?.stale && <small>已编辑</small>}</div>
}
