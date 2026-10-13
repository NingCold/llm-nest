import { MessageFiles } from "./MessageFiles"
import { MessageImages } from "./MessageImages"
import { useEffect, useState } from "react"
import { ArrowUpRight, Link2, X } from "lucide-react"
import { Dialog, DialogContent, DialogDescription, DialogTitle } from "@/components/ui/dialog"
import { socialClient } from "./client"
import { useLiveSocialStore } from "./live-store"
import { navigate } from "./router"
import type { Conversation, Message, ReferenceInput, ReferencePreview, ReferenceSource } from "./live-types"

function Preview({ input, stale = false }: { input: ReferenceInput; stale?: boolean }) {
  const state = useLiveSocialStore()
  const stamp = JSON.stringify([input, state.snapshot?.cursor, state.connected, stale])
  const [result, setResult] = useState<{ stamp: string; data?: ReferencePreview; error?: string } | null>(null)
  const [retry, setRetry] = useState(0)
  useEffect(() => {
    let current = true
    if (!stale && state.connected) void socialClient.referencePreview(input).then(data => { if (current) setResult({ stamp, data }) }, error => { if (current) setResult({ stamp, error: String(error) }) })
    return () => { current = false }
  }, [stamp, retry])
  const visible = result?.stamp === stamp ? result : null
  if (stale) return <p role="status">引用已失效，请回到群聊重新选择。原私聊记录保留。</p>
  if (!state.connected) return <p role="status">连接恢复后才能核对引用来源。</p>
  if (visible?.error) return <p role="alert">{visible.error}<button className="text-button" onClick={() => setRetry(n => n + 1)}>重新读取</button></p>
  if (!visible?.data) return <p role="status">正在核对群成员与原文版本…</p>
  const data = visible.data
  return <div className="reference-preview"><strong>来自群聊 · {data.title}</strong>{data.messages.map(m => <blockquote key={m.id}><small>{m.senderName} · {new Date(m.createdAt).toLocaleString()} · #{m.sequence}</small><p>{m.text}</p><MessageImages message={m} /><MessageFiles message={m} /></blockquote>)}{data.summary && <section><strong>附带分段摘要 · #{data.summary.fromSequence}–{data.summary.throughSequence}</strong><p>{data.summary.text}</p><small>摘要有损，需要具体原话时请查看群记录。</small></section>}</div>
}

export function ReferenceCard({ input, stale, remove }: { input: ReferenceInput; stale?: boolean; remove?: () => void }) {
  const [open, setOpen] = useState(false)
  return <div className={`live-reference ${stale ? "stale" : ""}`}><Link2 /><button className="text-button" onClick={() => setOpen(true)}>{stale ? "群话题引用已失效" : `群话题引用 · ${input.selection.sources.filter(s => s.kind === "message").length} 条原文${input.selection.sources.some(s => s.kind === "summary") ? "与摘要" : ""}`}</button>{remove && <button className="social-icon-button" aria-label="移除群话题引用" onClick={remove}><X /></button>}<Dialog open={open} onOpenChange={setOpen}><DialogContent className="social-dialog"><DialogTitle>群话题来源</DialogTitle><DialogDescription>只读取双方仍可见的所选内容，来源变化后不会采用旧版本。</DialogDescription><Preview input={input} stale={stale} /></DialogContent></Dialog></div>
}

function ContinueEditor({ conversation, message, done }: { conversation: Conversation; message: Message; done(): void }) {
  const state = useLiveSocialStore()
  const snapshot = state.snapshot!
  const current = snapshot.conversations.find(c => c.id === conversation.id)
  const actors = snapshot.accounts.filter(a => a.kind === "ai" && a.status === "active" && current?.memberIds.includes(a.id))
  const [actorId, setActorId] = useState(actors.find(a => a.id === message.senderId)?.id ?? actors[0]?.id ?? "")
  const [selected, setSelected] = useState([message.id])
  const [summaryId, setSummaryId] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  const history = state.histories[conversation.id] ?? []
  const anchor = history.findIndex(m => m.id === message.id)
  const choices = history.slice(Math.max(0, anchor - 4), anchor + 4).filter(m => m.status === "committed")
  const summaries = state.summaries[conversation.id]?.segments.filter(s => s.status === "ready" && s.sources.some(source => selected.includes(source.message_id))) ?? []
  useEffect(() => { void state.action(() => state.loadSummaries(conversation.id)) }, [conversation.id])
  const sources: ReferenceSource[] = choices.filter(m => selected.includes(m.id)).map(m => ({ kind: "message", conversation_id: conversation.id, message_id: m.id, revision: m.revision }))
  const summary = summaries.find(s => s.id === summaryId)
  if (summary) sources.push({ kind: "summary", summary_id: summary.id, revision: summary.revision })
  const valid = actors.some(a => a.id === actorId) && sources.some(s => s.kind === "message")
  const input = { actorId, selection: { conversationId: conversation.id, sources } }
  const open = async () => {
    setBusy(true)
    await state.action(async () => {
      await socialClient.referencePreview(input)
      const id = await socialClient.openDirect(actorId, crypto.randomUUID())
      await state.refresh()
      state.referenceDraft(id, input)
      navigate("messages", id)
      done()
    })
    setBusy(false)
  }
  return <div className="social-form">
    {state.error && <p className="form-error" role="alert">{state.error}</p>}
    <fieldset className="social-form-fields" disabled={busy || !state.draftsReady || state.draftsClosing}>
      <label>与谁私聊<select value={actorId} onChange={e => setActorId(e.target.value)}>{actors.map(a => <option key={a.id} value={a.id}>{a.name}</option>)}</select></label>
      {!actors.length && <p>当前没有可接续话题的共同 AI 成员。</p>}
      <p>选择需要带入私聊的原文（最多 8 条）。可以先在群聊加载更早记录。</p>
      <div className="reference-choices">{choices.map(m => <label key={m.id}><input type="checkbox" checked={selected.includes(m.id)} onChange={e => setSelected(ids => e.target.checked ? [...ids, m.id] : ids.filter(id => id !== m.id))} /><span><small>{m.senderName} · #{m.sequence}</small><span>{m.text || (m.images?.length ? "[图片]" : "")}</span></span></label>)}</div>
      {!!summaries.length && <label>可附带相关群摘要<select value={summary?.id ?? ""} onChange={e => setSummaryId(e.target.value || null)}><option value="">仅引用原文</option>{summaries.map(s => <option key={s.id} value={s.id}>第 {s.fromSequence}–{s.throughSequence} 条消息的摘要</option>)}</select></label>}
      {summary && <blockquote className="reference-summary">{summary.text}</blockquote>}
      {state.summaryErrors[conversation.id] && <p>摘要未能读取，仍可选择原文。<button className="text-button" onClick={() => void state.action(() => state.loadSummaries(conversation.id))}>重试摘要</button></p>}
      <p>将打开已有的唯一私聊，保留输入文字，替换该窗口的引用草稿。不会自动发送。</p>
      <div className="form-actions"><button className="social-button secondary" onClick={done}>取消</button><button className="social-button primary" disabled={!valid} onClick={() => void open()}>{busy ? "核对来源…" : "带入私聊草稿"}</button></div>
    </fieldset>
  </div>
}

export function ContinuePrivately({ conversation, message }: { conversation: Conversation; message: Message }) {
  const [open, setOpen] = useState(false)
  return <><button className="live-reply" onClick={() => setOpen(true)}><ArrowUpRight />私聊继续</button><Dialog open={open} onOpenChange={setOpen}><DialogContent className="social-dialog"><DialogTitle>与 AI 私聊继续</DialogTitle><DialogDescription>选择共同群话题，接续到这个 AI 的长期私聊。</DialogDescription>{open && <ContinueEditor conversation={conversation} message={message} done={() => setOpen(false)} />}</DialogContent></Dialog></>
}
