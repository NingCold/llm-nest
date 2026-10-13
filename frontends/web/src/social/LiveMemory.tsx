import { useEffect, useRef, useState, type FormEvent } from "react"
import { SummaryPanel } from "./LiveSummary"
import { Brain, Plus } from "lucide-react"
import { Dialog, DialogContent, DialogDescription, DialogTitle } from "@/components/ui/dialog"
import { socialClient } from "./client"
import { useLiveSocialStore } from "./live-store"
import { useVersionedAction } from "./LiveControls"
import type { Conversation, Memory, MemoryContent, MemoryKind, Message } from "./live-types"

const labels: Record<MemoryKind, string> = { preference: "偏好", fact: "事实", shared_experience: "共同经历", open_topic: "未完话题" }

function MemoryEditor({ conversation, actor, memory, message, done }: { conversation: Conversation; actor: string; memory?: Memory; message?: Message; done(): void }) {
  const state = useLiveSocialStore()
  const [text, setText] = useState(memory?.text ?? message?.text ?? "")
  const [kind, setKind] = useState<MemoryKind>(memory?.kind ?? "fact")
  const [source, setSource] = useState<MemoryContent["source"]>(memory?.source ?? (message ? { messageId: message.id, revision: message.revision } : null))
  const [sourceRead, setCurrent] = useState<{ cursor: string; message: Message } | null>(null)
  const current = sourceRead?.cursor === state.snapshot!.cursor ? sourceRead.message : null
  useEffect(() => { setCurrent(null) }, [state.snapshot?.cursor])
  const [revision] = useState(memory?.revision ?? "1")
  const { busy, execute } = useVersionedAction()
  const save = async (event: FormEvent) => {
    event.preventDefault()
    const content = { text, kind, source }
    const result = await execute(JSON.stringify([conversation.id, actor, content]), revision, input => memory
      ? socialClient.updateMemory(memory.id, { ...input, ...content })
      : socialClient.createMemory(conversation.id, actor, { requestId: input.requestId, ...content }))
    if (result) { done(); await state.action(state.refresh) }
  }
  return <form className="social-form" onSubmit={event => void save(event)}>
    {state.error && <p role="alert" className="form-error">{state.error}</p>}
    <fieldset className="social-form-fields" disabled={busy}>
      <label>类型<select value={kind} onChange={event => setKind(event.target.value as MemoryKind)}>{Object.entries(labels).map(([key, label]) => <option key={key} value={key}>{label}</option>)}</select></label>
      <label>记忆内容<textarea autoFocus required rows={4} maxLength={2000} value={text} onChange={event => setText(event.target.value)} /></label>
      {source ? <div className="memory-source"><p>来源消息 · 版本 {source.revision}</p><button type="button" className="text-button" onClick={() => void state.action(async () => setCurrent({ cursor: state.snapshot!.cursor, message: await socialClient.message(source.messageId) }))}>查看当前原文</button>
        {current && <><blockquote>{current.status === "withdrawn" ? "消息已撤回" : current.text}</blockquote><p>{current.senderName} · 当前版本 {current.revision}</p>{current.status === "committed" && current.revision !== source.revision && <button type="button" className="social-button secondary" onClick={() => setSource({ messageId: current.id, revision: current.revision })}>已核对，采用此版本</button>}</>}
        <button type="button" className="text-button" onClick={() => { setSource(null); setCurrent(null) }}>移除消息依据，改为手动记忆</button></div> : <p>手动记忆，由你确认内容。只用于当前{conversation.kind === "group" ? "群聊" : "私聊"}。</p>}
      {memory?.status === "stale" && <p>来源已变化。请查看当前原文并核对版本，或明确改为手动记忆。</p>}
      <div className="dialog-actions"><button type="button" className="social-button secondary" onClick={done}>取消</button><button className="social-button primary" disabled={!text.trim()}>{busy ? "保存中…" : "保存记忆"}</button></div>
    </fieldset>
  </form>
}

export function RememberMessage({ conversation, message }: { conversation: Conversation; message: Message }) {
  const state = useLiveSocialStore()
  const actors = state.snapshot!.accounts.filter(a => a.kind === "ai" && conversation.memberIds.includes(a.id))
  const [open, setOpen] = useState(false)
  const [actor, setActor] = useState("")
  const trigger = useRef<HTMLButtonElement | null>(null)
  if (!actors.length) return null
  return <><button ref={trigger} className="live-reply" onClick={() => { setActor(actors[0].id); setOpen(true) }}><Brain />记住</button>
    <Dialog open={open} onOpenChange={setOpen}><DialogContent className="social-dialog" onCloseAutoFocus={event => { event.preventDefault(); trigger.current?.focus() }}><DialogTitle>让 AI 记住</DialogTitle><DialogDescription>保存为这个 AI 在当前聊天中的记忆，来源修改后会失效。</DialogDescription>
      <label className="memory-owner">归属 AI<select value={actor} onChange={event => setActor(event.target.value)}>{actors.map(a => <option key={a.id} value={a.id}>{a.name}</option>)}</select></label>
      {open && <MemoryEditor conversation={conversation} actor={actor} message={message} done={() => setOpen(false)} />}
    </DialogContent></Dialog></>
}

function MemoryList({ conversation, actor }: { conversation: Conversation; actor: string }) {
  const state = useLiveSocialStore()
  const key = `${conversation.id}/${actor}`
  const items = state.memories[key] ?? []
  const loading = state.loadingMemories[key] ?? true
  const error = state.memoryErrors[key]
  const [refresh, setRefresh] = useState(0)
  const [editor, setEditor] = useState<Memory | "new" | null>(null)
  const [forgetting, setForgetting] = useState<Memory | null>(null)
  const [sourceRead, setSource] = useState<{ cursor: string; memory: string; message: Message } | null>(null)
  const source = sourceRead?.cursor === state.snapshot!.cursor ? sourceRead : null
  const sourceRequest = useRef(0)
  const trigger = useRef<HTMLButtonElement | null>(null)
  const { busy, execute } = useVersionedAction()
  useEffect(() => {
    void state.action(() => state.loadMemories(conversation.id, actor))
  }, [conversation.id, actor, refresh])
  useEffect(() => { sourceRequest.current++; setSource(null) }, [state.snapshot?.cursor])
  const writable = conversation.status === "active" && !conversation.legacy && conversation.memberIds.includes(actor)
  const forget = async () => {
    if (!forgetting) return
    const result = await execute(`forget:${forgetting.id}`, forgetting.revision, input => socialClient.forgetMemory(forgetting.id, input))
    if (result) { setForgetting(null); setSource(null); setRefresh(n => n + 1); await state.action(state.refresh) }
  }
  return <div className="memory-list">
    <button className="social-button secondary" disabled={!writable} onClick={event => { trigger.current = event.currentTarget; setEditor("new") }}><Plus />添加记忆</button>
    {error && <p role="alert" className="form-error">{error}<button className="text-button" onClick={() => setRefresh(n => n + 1)}>重试</button></p>}
    {loading ? <p role="status">正在读取记忆…</p> : !items.length && !error ? <p>还没有记忆。可以手动添加，或在一条消息上选择“记住”。</p> : null}
    {items.map(memory => <article className="memory-card" key={memory.id}><div className="memory-meta"><span>{labels[memory.kind]}</span><span>{memory.status === "stale" ? "来源已变化 · 暂不使用" : "已确认"}</span></div><p className="memory-text">{memory.text}</p><small>{new Date(memory.updatedAt).toLocaleString()} · 版本 {memory.revision}</small>
      <div className="live-message-actions">{memory.source ? <button onClick={() => { const request = ++sourceRequest.current, cursor = state.snapshot!.cursor; setSource(null); void state.action(async () => { const message = await socialClient.message(memory.source!.messageId); if (request === sourceRequest.current) setSource({ cursor, memory: memory.id, message }) }) }}>查看来源 · v{memory.source.revision}</button> : <span>手动添加</span>}<button disabled={!writable} onClick={event => { trigger.current = event.currentTarget; setEditor(memory) }}>编辑</button><button onClick={event => { trigger.current = event.currentTarget; setForgetting(memory) }}>遗忘</button></div>
      {source?.memory === memory.id && <blockquote>{source.message.senderName} · 当前版本 {source.message.revision}<br />{source.message.status === "withdrawn" ? "消息已撤回" : source.message.text}</blockquote>}
    </article>)}
    <p className="memory-note">遗忘会移除记忆条目，原聊天记录仍保留，近期上下文仍可能读到原文。需要移除原文时，请在聊天中撤回对应消息。</p>
    <Dialog open={editor !== null} onOpenChange={open => { if (!open) setEditor(null) }}><DialogContent className="social-dialog" onCloseAutoFocus={event => { event.preventDefault(); trigger.current?.focus() }}><DialogTitle>{editor === "new" ? "添加记忆" : "编辑记忆"}</DialogTitle><DialogDescription>{editor === "new" ? "记忆只供所选 AI 在当前聊天中使用，从下一次回复开始读取。" : "记忆只供所选 AI 在当前聊天中使用。保存修改会停止本窗口正在生成的回复。"}</DialogDescription>{editor && <MemoryEditor conversation={conversation} actor={actor} memory={editor === "new" ? undefined : editor} done={() => { setEditor(null); setRefresh(n => n + 1) }} />}</DialogContent></Dialog>
    <Dialog open={forgetting !== null} onOpenChange={open => { if (!open && !busy) setForgetting(null) }}><DialogContent className="social-dialog" onCloseAutoFocus={event => { event.preventDefault(); trigger.current?.focus() }}><DialogTitle>遗忘这条记忆？</DialogTitle><DialogDescription>记忆正文及其修订正文将移除，本窗口正在生成的回复会停止。原聊天、已发送回答和备份保留。</DialogDescription>{state.error && <p role="alert" className="form-error">{state.error}</p>}<div className="dialog-actions"><button className="social-button secondary" disabled={busy} onClick={() => setForgetting(null)}>取消</button><button className="social-button primary" disabled={busy} onClick={() => void forget()}>{busy ? "正在遗忘…" : "确认遗忘"}</button></div></DialogContent></Dialog>
  </div>
}

export function MemoryPanel({ conversation }: { conversation: Conversation }) {
  const state = useLiveSocialStore()
  const actors = state.snapshot!.accounts.filter(a => a.kind === "ai" && (conversation.memberIds.includes(a.id) || conversation.memoryOwnerIds.includes(a.id)))
  const [selected, setSelected] = useState("")
  const actor = actors.some(a => a.id === selected) ? selected : actors[0]?.id
  return <section className="live-consciousness"><h2>意识与记忆</h2><p>记录完整保存在窗口中。每次回复从本窗口的近期记录、有效摘要与这个 AI 的记忆组织上下文，私聊记忆不会自动带入群聊。</p>
    {actor ? <><label className="memory-owner">归属 AI<select value={actor} onChange={event => setSelected(event.target.value)}>{actors.map(a => <option key={a.id} value={a.id}>{a.name}{conversation.memberIds.includes(a.id) ? "" : "（已离群）"}</option>)}</select></label><MemoryList key={`${conversation.id}:${actor}`} conversation={conversation} actor={actor} /></> : <p>当前聊天没有 AI 成员。</p>}
    <SummaryPanel key={conversation.id} conversation={conversation} />
  </section>
}
