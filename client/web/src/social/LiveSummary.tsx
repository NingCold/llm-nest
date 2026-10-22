import { useEffect, useRef, useState, type FormEvent } from "react"
import { BookOpen, Settings2 } from "lucide-react"
import { Dialog, DialogContent, DialogDescription, DialogTitle } from "@/components/ui/dialog"
import { socialClient } from "./client"
import { useLiveSocialStore } from "./live-store"
import { useVersionedAction } from "./LiveControls"
import type { Conversation, Message, Summary, SummarySettings } from "./live-types"

const statusLabels: Record<Summary["status"], string> = {
  queued: "等待生成", running: "正在整理", ready: "可用于上下文", failed: "生成失败", interrupted: "已中断", stale: "来源已变化 · 暂不使用",
}

function SummarySettingsForm({ conversation, settings, done }: { conversation: string; settings: SummarySettings; done(): void }) {
  const state = useLiveSocialStore()
  const models = state.snapshot!.models
  const [enabled, setEnabled] = useState(settings.enabled)
  const [modelKey, setModelKey] = useState(settings.binding ? JSON.stringify([settings.binding.providerId, settings.binding.modelId]) : "")
  const [effort, setEffort] = useState(settings.binding?.reasoningEffort ?? "")
  const [maxTokens, setMaxTokens] = useState(settings.binding?.maxTokens ?? 1024)
  const model = models.find(m => JSON.stringify([m.providerId, m.modelId]) === modelKey)
  const limits = model?.generationLimits[effort || "default"]
  const { busy, execute } = useVersionedAction()
  const save = async (event: FormEvent) => {
    event.preventDefault()
    const binding = model ? { providerId: model.providerId, modelId: model.modelId, reasoningEffort: effort || null, maxTokens } : settings.binding
    const result = await execute(JSON.stringify([enabled, binding]), settings.revision, input => socialClient.configureSummaries(conversation, { ...input, enabled, binding }))
    if (result) { done(); await state.action(() => state.loadSummaries(conversation)) }
  }
  return <form className="social-form" onSubmit={event => void save(event)}>
    {state.error && <p role="alert" className="form-error">{state.error}</p>}
    <fieldset className="social-form-fields" disabled={busy}>
      <label className="summary-toggle"><input type="checkbox" checked={enabled} onChange={event => setEnabled(event.target.checked)} />自动整理较早的聊天</label>
      <label>摘要专用模型<select value={modelKey} onChange={event => { setModelKey(event.target.value); setEffort("") }}><option value="">请选择模型</option>{settings.binding && !model && <option value={modelKey}>当前模型不可用</option>}{models.map(m => <option key={JSON.stringify([m.providerId, m.modelId])} value={JSON.stringify([m.providerId, m.modelId])}>{m.displayName} · {m.providerId}</option>)}</select></label>
      {!!model?.reasoningLevels.length && <label>思考强度<select value={effort} onChange={event => setEffort(event.target.value)}><option value="">模型默认</option>{model.reasoningLevels.map(level => <option key={level}>{level}</option>)}</select></label>}
      <label>每段输出上限（tokens）<input type="number" required min={limits?.minOutputTokens ?? 1} max={Math.min(limits?.maxOutputTokens ?? 16384, 16384)} value={maxTokens} onChange={event => setMaxTokens(Number(event.target.value))} /></label>
      <p className="form-note">开启后会额外调用所选模型，可能产生服务费用。每段最多整理 32 条较早消息，最近 12 条保留原文。关闭自动整理会停止当前摘要任务，已有有效摘要仍可使用。</p>
      <div className="dialog-actions"><button type="button" className="social-button secondary" onClick={done}>取消</button><button className="social-button primary" disabled={enabled && !model}>{busy ? "保存中…" : "保存设置"}</button></div>
    </fieldset>
  </form>
}

export function SummaryPanel({ conversation }: { conversation: Conversation }) {
  const state = useLiveSocialStore()
  const data = state.summaries[conversation.id]
  const error = state.summaryErrors[conversation.id]
  const loading = state.loadingSummaries[conversation.id]
  const [settings, setSettings] = useState<SummarySettings | null>(null)
  const [sources, setSources] = useState<string | null>(null)
  const [original, setOriginal] = useState<Message | null>(null)
  const [notice, setNotice] = useState("")
  const sourceRead = useRef(0)
  const settingsButton = useRef<HTMLButtonElement | null>(null)
  const { busy, execute } = useVersionedAction()
  useEffect(() => { void state.action(() => state.loadSummaries(conversation.id)) }, [conversation.id])
  useEffect(() => { sourceRead.current++; setOriginal(null) }, [state.snapshot?.cursor])
  const writable = conversation.status === "active" && !conversation.legacy
  const running = data?.segments.some(s => s.status === "queued" || s.status === "running")
  const request = async (summary?: Summary) => {
    const result = await execute(`summary:${conversation.id}:${summary?.id ?? "new"}`, summary?.revision ?? data!.settings.revision, input => socialClient.requestSummary(conversation.id, {
      requestId: input.requestId, retry: summary ? { id: summary.id, expectedRevision: input.expectedRevision } : null,
    }))
    if (result !== undefined) {
      setNotice(result === null ? "当前没有需要整理的较早消息。最近 12 条继续保留原文。" : "摘要任务已提交，可在下方查看进度。")
      await state.action(() => state.loadSummaries(conversation.id))
    }
  }
  return <section className="summary-panel" aria-label="窗口分段摘要">
    <div className="summary-heading"><h3><BookOpen />窗口分段摘要</h3><button ref={settingsButton} className="text-button" disabled={!writable || !data || loading} onClick={() => setSettings(data!.settings)}><Settings2 />摘要设置</button></div>
    <p>较早的讨论整理成可核对的背景，原聊天完整保留。{conversation.kind === "group" ? "本群当前成员共享这些摘要。" : "摘要只用于这个私聊。"}它不等同于已确认记忆。</p>
    {error && <p role="alert" className="form-error">{error}<button className="text-button" onClick={() => void state.action(() => state.loadSummaries(conversation.id))}>重新读取摘要</button></p>}
    {!data && !error && <p role="status">正在读取摘要…</p>}
    {data && <>
      <div className="summary-toolbar"><span>{data.settings.enabled ? `自动整理已开启 · ${data.settings.binding?.modelId}` : "自动整理未开启"}</span><button className="social-button secondary" disabled={!writable || !data.settings.enabled || busy || running || loading} onClick={() => void request()}>整理下一段</button></div>
      {notice && <p role="status">{notice}</p>}
      {!data.segments.length && <p className="memory-note">还没有分段摘要。可在设置中选择模型并开启，聊天积累后会自动整理。</p>}
      {data.segments.map(summary => <article className="memory-card" key={summary.id}>
        <div className="memory-meta"><span>第 {summary.fromSequence}–{summary.throughSequence} 条</span><span>{statusLabels[summary.status]}</span></div>
        {summary.status === "ready" && <p className="memory-text">{summary.text}</p>}
        {summary.error && <p className="form-note">{summary.error}</p>}
        <small>{summary.binding.modelId} · 第 {summary.attempt} 次生成 · {new Date(summary.updatedAt).toLocaleString()}</small>
        <div className="live-message-actions"><button onClick={() => { sourceRead.current++; setSources(sources === summary.id ? null : summary.id); setOriginal(null) }}>核对来源（{summary.sources.length}）</button>{["failed", "interrupted", "stale"].includes(summary.status) && <button disabled={!writable || !data.settings.enabled || busy || running} onClick={() => void request(summary)}>{summary.status === "stale" ? "按当前原文重建" : "重试生成"}</button>}</div>
        {sources === summary.id && <div className="summary-sources"><p>以下版本是本次整理的依据；点击查看当前原文。</p><div className="live-message-actions">{summary.sources.map((source, index) => <button key={source.message_id} onClick={() => {
          const ticket = ++sourceRead.current
          const cursor = state.snapshot?.cursor
          void state.action(async () => { const message = await socialClient.message(source.message_id); if (ticket === sourceRead.current && cursor === useLiveSocialStore.getState().snapshot?.cursor) setOriginal(message) })
        }}>来源 {index + 1} · v{source.revision}</button>)}</div>{original && <blockquote>{original.senderName} · 第 {original.sequence} 条 · 当前 v{original.revision}<br />{original.status === "withdrawn" ? "消息已撤回" : original.text}</blockquote>}</div>}
      </article>)}
      {data.hasMore && <button className="text-button" disabled={loading} onClick={() => void state.action(() => state.loadSummaries(conversation.id, true))}>查看更早的分段</button>}
    </>}
    <Dialog open={settings !== null} onOpenChange={open => { if (!open) setSettings(null) }}><DialogContent className="social-dialog" onCloseAutoFocus={event => { event.preventDefault(); settingsButton.current?.focus() }}><DialogTitle>窗口摘要设置</DialogTitle><DialogDescription>为这个聊天单独选择摘要模型，不改变任何 AI 账号的模型与角色设定。</DialogDescription>{settings && <SummarySettingsForm conversation={conversation.id} settings={settings} done={() => setSettings(null)} />}</DialogContent></Dialog>
  </section>
}
