import { useState, type FormEvent } from "react"
import { Settings2 } from "lucide-react"
import { Dialog, DialogContent, DialogDescription, DialogTitle } from "@/components/ui/dialog"
import { socialClient } from "./client"
import { useLiveSocialStore } from "./live-store"
import { useVersionedAction } from "./LiveControls"
import type { Conversation, ParticipationPolicy, ParticipationReason } from "./live-types"

export const participationReasons: Record<ParticipationReason, string> = {
  waiting: "等待新消息", input_pending: "等待接话", turn_limit: "本轮自动接话已达到上限",
  daily_limit: "今日自动接话次数已用完", token_limit: "今日自动接话额度不足",
  no_candidate: "暂时没有可接话的 AI", expired: "排队接话已过期，等待新消息",
  idle: "空闲交谈已结束", user_ended: "你已结束这次交谈", policy_changed: "接话规则已更新，等待新消息",
}

function ParticipationForm({ conversation, initial, done }: { conversation: Conversation; initial: ParticipationPolicy; done(): void }) {
  const [policy, setPolicy] = useState(initial)
  const state = useLiveSocialStore()
  const { busy, execute } = useVersionedAction()
  const number = (key: keyof ParticipationPolicy, value: string) => setPolicy(p => ({ ...p, [key]: Number(value) }))
  const save = async (event: FormEvent) => {
    event.preventDefault()
    const result = await execute(JSON.stringify(policy), initial.revision, input => socialClient.updateParticipation(conversation.id, { requestId: input.requestId, policy: { ...policy, revision: input.expectedRevision } }))
    if (result) { done(); await state.action(state.refresh) }
  }
  return <form className="social-form" onSubmit={event => void save(event)}>
    {state.error && <p role="alert" className="form-error">{state.error}</p>}
    <fieldset className="social-form-fields" disabled={busy}>
      {conversation.kind === "direct" && <><label>连续输入等待（毫秒）<input type="number" required min={0} max={2000} step={100} value={policy.mergeDelayMs} onChange={e => number("mergeDelayMs", e.target.value)} /></label><p className="form-note">默认等待 800 毫秒，将尚未开始回复的连续普通消息一起回应。每条原文都会保留；引用和回复消息各自接话。设为 0 可立即回复。</p></>}
      <label>空闲结束（秒）<input type="number" required min={30} max={3600} value={policy.idleSeconds} onChange={e => number("idleSeconds", e.target.value)} /></label>
      <p className="form-note">没有回复或排队任务时，达到这个时长就结束当前交谈。聊天记录保留，新消息可再次开始。</p>
      {conversation.kind === "group" && <>
        <label className="summary-toggle"><input type="checkbox" checked={policy.limitedAuto} onChange={e => setPolicy(p => ({ ...p, limitedAuto: e.target.checked }))} />允许 AI 有限自动接话</label>
        <p className="form-note">开启后，普通消息也可触发 AI 按顺序轮流接话，会额外调用模型。暂停的成员不会参与；明确 @ 的回复优先。关闭或修改规则会停止现有自动接话。</p>
        <div className="participation-limits"><label>无人插话时最多连续发言<input type="number" required min={1} max={6} value={policy.maxConsecutive} onChange={e => number("maxConsecutive", e.target.value)} /></label>
        <label>每次交谈最多自动接话<input type="number" required min={1} max={48} value={policy.maxAutoPerBout} onChange={e => number("maxAutoPerBout", e.target.value)} /></label>
        <label>每日最多自动接话<input type="number" required min={1} max={200} value={policy.maxAutoPerDay} onChange={e => number("maxAutoPerDay", e.target.value)} /></label>
        <label>每日 token 预留额度<input type="number" required min={1024} max={4000000} value={policy.dailyTokenBudget} onChange={e => number("dailyTokenBudget", e.target.value)} /></label>
        </div><p className="form-note">额度按本群、UTC 日期计算。每次自动调用预留整个模型上下文容量（输入与输出），即使失败也不退回；这是调用上限，不是实际账单。真人插话或结束交谈不会清零当天额度。明确 @ 的回复不占自动接话额度。</p>
      </>}
      <div className="dialog-actions"><button type="button" className="social-button secondary" onClick={done}>取消</button><button className="social-button primary">{busy ? "保存中…" : "保存规则"}</button></div>
    </fieldset>
  </form>
}

export function ParticipationPanel({ conversation }: { conversation: Conversation }) {
  const [editing, setEditing] = useState<ParticipationPolicy | null>(null)
  const { policy, state, autoTurnsToday, reservedTokensToday, budgetDay } = conversation.participation
  if (conversation.legacy) return null
  return <section className="live-conversation-controls">
    <h3>接话节奏</h3>
    <p>{conversation.kind === "group" ? policy.limitedAuto ? `允许有限自动接话 · 最多连续 ${policy.maxConsecutive} 次发言` : "只响应点名或回复 · 自动接话未开启" : policy.mergeDelayMs ? `等待 ${policy.mergeDelayMs} 毫秒，合并连续普通消息` : "收到消息后立即回复"}</p>
    {state.reason && !conversation.activeRun && !conversation.pendingPlans.length && <p className="form-note" role="status">{participationReasons[state.reason]}</p>}
    {conversation.kind === "group" && <p className="form-note">{budgetDay}（UTC）· 已自动接话 {autoTurnsToday} / {policy.maxAutoPerDay} 次 · 已预留 {reservedTokensToday.toLocaleString()} / {policy.dailyTokenBudget.toLocaleString()} tokens</p>}
    <button className="social-button secondary" disabled={conversation.status === "archived"} onClick={() => setEditing(policy)}><Settings2 />调整接话规则</button>
    <Dialog open={!!editing} onOpenChange={open => { if (!open) setEditing(null) }}><DialogContent className="social-dialog"><DialogTitle>接话规则</DialogTitle><DialogDescription>仅影响这个聊天窗口。账号和模型设定保持独立。</DialogDescription>{editing && <ParticipationForm conversation={conversation} initial={editing} done={() => setEditing(null)} />}</DialogContent></Dialog>
  </section>
}
