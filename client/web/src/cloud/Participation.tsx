import { useEffect, useRef, useState } from "react";
import { encryptIntent, projectIntent } from "./crypto/content";
import { cryptoDevice } from "./crypto/engine";
import { HttpError, request, server } from "./api";
import type { Account, ContactIntent, ContactSettings, Conversation, ParticipationPolicy, ParticipationView } from "./types";

const reasons: Record<string, string> = {
  waiting: "等待接话", input_pending: "等待连续输入结束", turn_limit: "已达到本次接话上限", daily_limit: "今天的自动调用次数已用完",
  token_limit: "节点的自动调用预算不足", node_budget_exhausted: "节点今天的调用预算已用完", no_candidate: "暂无可接话的 AI", expired: "已超过有效时间", idle: "空闲后已结束交谈",
  user_ended: "已结束当前交谈", policy_changed: "接话设置已更新", paused: "窗口已暂停", ai_paused: "此 AI 已暂停", conversation_paused: "窗口已暂停",
  quiet: "处于免打扰时段", quiet_skip: "免打扰期间跳过", quiet_deferred: "延后至免打扰结束", settings_disabled: "一次性约定已关闭", contact_disabled: "一次性约定已关闭",
  membership_changed: "窗口成员已变化", account_disabled: "AI 账号已停用", direct_blocked: "双方私聊已屏蔽", cancelled: "已取消", node_restart_uncertain: "节点重启，结果尚未确认",
  authorization_closed: "约定授权已关闭", authorization_changed: "成员或联系授权已变化",
  automatic_quota: "自动调用次数已达上限", node_call_limit: "节点今天的调用次数已用完", node_token_limit: "节点今天的模型用量预算已用完",
  budget_clock_rollback: "节点时钟已回拨，预算暂不可用", server_restarted: "服务已重启，结果尚未确认",
};
export const participationReason = (reason: string) => reasons[reason] || reason;
const intentStatuses: Record<ContactIntent["status"], string> = { pending: "等待到期", claimed: "等待 AI 执行", executing: "AI 正在联系", delivered: "已完成", cancelled: "已取消", skipped: "已跳过", expired: "已过期", failed: "联系失败", uncertain: "结果尚未确认，不会自动重试" };
export const canCancelIntent = (intent: ContactIntent, operatorId: string, canManage: boolean) => (canManage || intent.creatorId === operatorId) && ["pending", "claimed", "executing"].includes(intent.status);

type Command = { path: string; method: string; body: Record<string, unknown> & { requestId: string }; label: string };
// Only unconfirmed commands survive closing this panel. Nothing is persisted to browser storage.
const commands = new Map<string, Command>();
export const clearParticipationCommands = () => commands.clear();
export function useParticipation(conversationId: string, operatorId: string, cursor: string, conversation?: Conversation) {
  const key = JSON.stringify([server, operatorId, conversationId]);
  const [view, setView] = useState<ParticipationView | null>(null);
  const [error, setError] = useState("");
  const [loading, setLoading] = useState(false);
  const [busy, setBusy] = useState(false);
  const [pending, setPending] = useState<Command | null>(() => commands.get(key) || null);
  const [reviewed, setReviewed] = useState(false);
  const [completed, setCompleted] = useState(0);
  const sequence = useRef(0), mounted = useRef(true), executing = useRef(false);
  const path = `/chat/conversations/${conversationId}`;
  const load = async (review = false) => {
    const ticket = ++sequence.current;
    setLoading(review || !view);
    try {
      const next = await request<ParticipationView>(`${path}/participation`);
      if (conversation) next.intents = await Promise.all(next.intents.map(i => projectIntent(i, conversation)));
      if (mounted.current && ticket === sequence.current) { setView(next); setError(""); if (review) setReviewed(commands.has(key)); }
    } catch (e) {
      if (mounted.current && ticket === sequence.current) {
        if (e instanceof HttpError && [401, 403, 404].includes(e.status)) setView(null);
        setError(e instanceof Error ? e.message : String(e));
      }
    } finally { if (mounted.current && ticket === sequence.current) setLoading(false); }
  };
  useEffect(() => { mounted.current = true; return () => { mounted.current = false; sequence.current++; }; }, []);
  useEffect(() => { void load(); }, [path, cursor]);
  const execute = async (suffix?: string, method?: string, body?: Record<string, unknown>, label = "原操作") => {
    if (executing.current) return false;
    const firstAttempt = !commands.has(key);
    let command = commands.get(key);
    executing.current = true; setBusy(true);
    try {
      if (!command) {
        if (!suffix || !method || !body) return false;
        if (suffix === "/contact-intents" && conversation?.encryption) {
          const intentId = crypto.randomUUID();
          const sealed = await encryptIntent(conversation, operatorId, intentId, body as { aiId: string; content: string; dueAt: string; expiresAt: string; utcOffsetMinutes: number });
          body = { ...body, intentId, deviceId: cryptoDevice(operatorId), sealed, content: "", expectedConversationRevision: conversation.revision };
        }
        command = { path: path + suffix, method, body: { ...body, requestId: crypto.randomUUID() }, label };
        commands.set(key, command);
      }
      setPending(command); setReviewed(false); setError("");
      await request(command.path, command.method, command.body);
      if (commands.get(key) === command) commands.delete(key);
      if (mounted.current) { setPending(null); setCompleted(value => value + 1); await load(); }
      return true;
    } catch (e) {
      // A first explicit rejection proves this command was not accepted. A later rejection
      // cannot establish the result of an earlier request whose response was lost.
      if (firstAttempt && e instanceof HttpError && e.status >= 400 && e.status < 500) {
        if (commands.get(key) === command) commands.delete(key);
        if (mounted.current) { setPending(null); await load(); }
      }
      if (mounted.current) setError(e instanceof Error ? e.message : String(e));
      return false;
    } finally { executing.current = false; if (mounted.current) setBusy(false); }
  };
  const release = () => { if (reviewed) { commands.delete(key); setPending(null); setReviewed(false); setError(""); } };
  return { view, error, loading, busy, pending, reviewed, completed, load, execute, release };
}
type State = ReturnType<typeof useParticipation>;
function RequestState({ state }: { state: State }) {
  return <>{state.loading && <p role="status">正在读取接话与约定状态…</p>}{state.error && <p className="social-error" role="alert">{state.error}</p>}
    {state.pending && !state.busy && <div className="cloud-unknown-command" role="status"><p>“{state.pending.label}”的结果尚未确认。重试会使用同一请求和原内容。</p>{typeof state.pending.body.content === "string" && <blockquote>{state.pending.body.content}</blockquote>}<button className="text-button" onClick={() => void state.execute()}>重试原操作</button><button className="text-button" onClick={() => void state.load(true)}>核对当前状态</button>{state.reviewed && state.view && <button className="text-button" onClick={state.release}>已核对当前状态，结束原操作</button>}</div>}
    {!state.pending && state.error && <button className="text-button" disabled={state.busy} onClick={() => void state.load()}>重新读取</button>}</>;
}

export function localDateTime(value: Date) {
  return new Date(value.getTime() - value.getTimezoneOffset() * 60000).toISOString().slice(0, 16);
}
function parseLocalTime(value: string) {
  const date = new Date(value);
  if (!Number.isFinite(date.getTime()) || localDateTime(date) !== value) throw new Error("这个本机时间无效，可能位于夏令时跳过的时段，请重新选择。");
  return date;
}
export function contactTimes(due: string, expires: string, now = new Date()) {
  const dueDate = parseLocalTime(due), expiryDate = parseLocalTime(expires);
  const year = new Date(now); year.setFullYear(year.getFullYear() + 1);
  if (dueDate <= now || dueDate > year) throw new Error("联系时间须在未来一年内。");
  if (expiryDate <= dueDate || expiryDate.getTime() - dueDate.getTime() > 7 * 86400000) throw new Error("失效时间须晚于联系时间，且最多延后七天。");
  return { dueAt: dueDate.toISOString(), expiresAt: expiryDate.toISOString(), utcOffsetMinutes: -now.getTimezoneOffset() };
}
const offsetLabel = (minutes: number) => `UTC${minutes < 0 ? "−" : "+"}${String(Math.floor(Math.abs(minutes) / 60)).padStart(2, "0")}:${String(Math.abs(minutes) % 60).padStart(2, "0")}`;

function PolicyEditor({ value, revision, group, state, done }: { value: ParticipationPolicy; revision: string; group: boolean; state: State; done(): void }) {
  const [policy, setPolicy] = useState(value);
  const fields = group ? [["maxConsecutiveAi", "无人插话时最多连续 AI 发言", 1, 6], ["maxBoutCalls", "每次交谈最多自动调用", 1, 48], ["maxDailyCalls", "每天最多自动调用（UTC 日）", 1, 200], ["idleSeconds", "空闲多久结束交谈（秒）", 30, 3600]] as const : [["directMergeMs", "连续输入等待（毫秒）", 0, 2000], ["maxConsecutiveAi", "无人插话时最多连续 AI 发言", 1, 6], ["maxBoutCalls", "每次交谈最多自动调用", 1, 48], ["maxDailyCalls", "每天最多自动调用（UTC 日）", 1, 200], ["idleSeconds", "空闲多久结束交谈（秒）", 30, 3600]] as const;
  return <form className="cloud-form" onSubmit={async e => { e.preventDefault(); if (await state.execute("/participation", "PUT", { expectedRevision: revision, policy }, "保存接话设置")) done(); }}><fieldset className="social-form-fields" disabled={state.busy || !!state.pending || state.loading || !!state.error || !state.view?.canManage}>
    {group && <label>群内接话方式<select value={policy.mode} onChange={e => setPolicy({ ...policy, mode: e.target.value as ParticipationPolicy["mode"] })}><option value="mentionedOnly">仅在明确提及或回复时接话</option><option value="limitedAuto">有限自动接话</option></select></label>}
    {fields.map(([name, label, min, max]) => <label key={name}>{label}<input type="number" required min={min} max={max} step={1} value={policy[name]} onChange={e => setPolicy({ ...policy, [name]: Number(e.target.value) })} /></label>)}
    <p>自动接话按固定顺序轮流进行。明确提及或回复不消耗自动次数；模型调用仍受各节点自身预算限制。保存设置不会补发旧消息。</p>
    <button className="social-button primary">保存接话设置</button><button type="button" className="text-button" onClick={done}>取消编辑接话设置</button>
  </fieldset></form>;
}
function SettingsEditor({ value, state, done }: { value: ContactSettings; state: State; done(): void }) {
  const [enabled, setEnabled] = useState(value.enabled), [quiet, setQuiet] = useState(value.quietUntil ? localDateTime(new Date(value.quietUntil)) : ""), [action, setAction] = useState(value.quietAction);
  return <form className="cloud-form" onSubmit={async e => { e.preventDefault(); if (await state.execute("/contact-settings", "PUT", { expectedRevision: value.revision, enabled, quietUntil: quiet ? new Date(quiet).toISOString() : null, quietAction: action }, "保存约定设置")) done(); }}><fieldset className="social-form-fields" disabled={state.busy || !!state.pending || state.loading || !!state.error || !state.view?.canManage}>
    <label className="cloud-check"><input type="checkbox" checked={enabled} onChange={e => setEnabled(e.target.checked)} />允许成员创建一次性约定</label>
    <label>免打扰截止时间（本机时间，可留空）<input type="datetime-local" value={quiet} onChange={e => setQuiet(e.target.value)} /></label>
    <label>免打扰期间到期的约定<select value={action} onChange={e => setAction(e.target.value as ContactSettings["quietAction"])}><option value="defer">延后至免打扰结束，过期后不补发</option><option value="skip">直接跳过，不再补发</option></select></label>
    <p>免打扰仅影响约定联系，普通消息仍可回复。关闭授权会取消所有未完成约定；开启免打扰会停止已经执行的联系。</p>
    <button className="social-button primary">保存约定设置</button><button type="button" className="text-button" onClick={done}>取消编辑约定设置</button>
  </fieldset></form>;
}
function IntentEditor({ ais, state, done }: { ais: Account[]; state: State; done(): void }) {
  const [aiId, setAiId] = useState(ais[0]?.id || ""), [content, setContent] = useState("");
  const [due, setDue] = useState(() => localDateTime(new Date(Date.now() + 3600000)));
  const [expires, setExpires] = useState(() => localDateTime(new Date(Date.now() + 7200000)));
  const [error, setError] = useState("");
  return <form className="cloud-form" onSubmit={async e => { e.preventDefault(); setError(""); try { const times = contactTimes(due, expires); if (await state.execute("/contact-intents", "POST", { aiId, content: content.trim(), ...times }, "创建一次性约定")) done(); } catch (e) { setError(e instanceof Error ? e.message : String(e)); } }}><fieldset className="social-form-fields" disabled={state.busy || !!state.pending || state.loading || !!state.error || !state.view?.settings.enabled}>
    <label>联系你的 AI<select required value={aiId} onChange={e => setAiId(e.target.value)}><option value="">选择 AI</option>{ais.map(ai => <option key={ai.id} value={ai.id}>{ai.name}</option>)}</select></label>
    <label>这一次约定的内容<textarea rows={3} required maxLength={4000} value={content} onChange={e => setContent(e.target.value)} /></label>
    <label>联系时间（本机时间）<input type="datetime-local" required value={due} onChange={e => { setDue(e.target.value); if (e.target.value) setExpires(localDateTime(new Date(new Date(e.target.value).getTime() + 3600000))); }} /></label>
    <label>失效时间（本机时间）<input type="datetime-local" required value={expires} onChange={e => setExpires(e.target.value)} /></label>
    <p>创建时固定时区偏移为 {offsetLabel(-new Date().getTimezoneOffset())}。默认联系后一小时失效，最多七天；离线超过有效时间不会补发。内容不能修改，需要取消后重新创建。</p>
    {error && <p role="alert" className="social-error">{error}</p>}<button className="social-button primary" disabled={!aiId || !content.trim() || !ais.some(ai => ai.id === aiId)}>创建一次性约定</button><button type="button" className="text-button" onClick={done}>取消创建约定</button>
  </fieldset></form>;
}

export function ParticipationPanel({ conversation, operatorId, cursor }: { conversation: Conversation; operatorId: string; cursor: string }) {
  const state = useParticipation(conversation.id, operatorId, cursor, conversation);
  const [policy, setPolicy] = useState<{ value: ParticipationPolicy; revision: string } | null>(null);
  const [settings, setSettings] = useState<ContactSettings | null>(null), [creating, setCreating] = useState(false), [ending, setEnding] = useState(false);
  const view = state.view;
  const ais = conversation.members.map(m => m.account).filter(a => a.kind === "ai" && a.status === "active");
  const locked = state.busy || !!state.pending || state.loading || !!state.error;
  useEffect(() => { if (state.completed) { setPolicy(null); setSettings(null); setCreating(false); setEnding(false); } }, [state.completed]);
  const control = (action: "pause" | "resume" | "end", aiId?: string) => void state.execute("/participation/control", "POST", { expectedRevision: view!.revision, action, ...(aiId ? { aiId } : {}) }, action === "end" ? "结束当前交谈" : action === "pause" ? "暂停接话" : "恢复接话");
  return <section className="cloud-detail-section cloud-participation"><h3>接话与一次性约定</h3><RequestState state={state} />
    {view && <>
      <p>{view.state.paused ? "窗口已暂停" : conversation.kind === "group" ? view.policy.mode === "limitedAuto" ? "有限自动接话已开启" : "仅明确提及或回复时接话" : "私聊正常接话"}{view.state.reason && ` · ${participationReason(view.state.reason)}`}</p>
      <dl className="cloud-participation-counts"><div><dt>连续 AI 发言</dt><dd>{view.state.consecutiveAi} / {view.policy.maxConsecutiveAi}</dd></div><div><dt>本次自动调用</dt><dd>{view.state.boutCalls} / {view.policy.maxBoutCalls}</dd></div><div><dt>今日自动调用（UTC）</dt><dd>{view.state.dailyCalls} / {view.policy.maxDailyCalls}</dd></div></dl>
      <p>次数由云端记录，开始后失败或取消也不返还。模型用量、并发和费用由运行节点管理。</p>
      {view.canManage ? <><div className="cloud-actions"><button className="social-button secondary" disabled={locked || !!policy} onClick={() => setPolicy({ value: { ...view.policy }, revision: view.revision })}>编辑接话设置</button><button className="social-button secondary" disabled={locked} onClick={() => control(view.state.paused ? "resume" : "pause")}>{view.state.paused ? "恢复窗口接话" : "暂停窗口接话"}</button><button className="text-button" disabled={locked} onClick={() => setEnding(true)}>结束当前交谈</button></div>
        <p>暂停会停止相应排队和回复；恢复只响应新消息，不补发暂停期间到期的约定。结束当前交谈会保留未来尚未到期的约定。</p>
        {ending && <div className="cloud-participation-confirm"><p>结束当前交谈并停止当前排队和回复？未来尚未到期的约定保留。</p><button className="social-button secondary" disabled={locked} onClick={() => control("end")}>确认结束当前交谈</button><button className="text-button" disabled={locked} onClick={() => setEnding(false)}>暂不结束</button></div>}
        {ais.map(ai => <div className="cloud-group-member" key={ai.id}><span>{ai.name}{view.state.pausedAiIds.includes(ai.id) && <small>此窗口中已暂停</small>}</span><button className="text-button" disabled={locked} onClick={() => control(view.state.pausedAiIds.includes(ai.id) ? "resume" : "pause", ai.id)}>{view.state.pausedAiIds.includes(ai.id) ? "恢复" : "暂停"} {ai.name}</button></div>)}
      </> : <p>接话和约定授权设置由群主或群管理员维护。授权开启后，你可以创建和取消自己的约定。</p>}
      {policy && <PolicyEditor value={policy.value} revision={policy.revision} group={conversation.kind === "group"} state={state} done={() => setPolicy(null)} />}
      <h3 className="cloud-participation-subtitle">一次性约定</h3><p>{view.settings.enabled ? "已允许成员明确创建一次性约定" : "尚未允许一次性约定"}{view.settings.quietUntil && ` · 免打扰至 ${new Date(view.settings.quietUntil).toLocaleString()}，到期约定${view.settings.quietAction === "defer" ? "延后" : "跳过"}`}</p>
      <div className="cloud-actions">{view.canManage && <button className="social-button secondary" disabled={locked || !!settings} onClick={() => setSettings({ ...view.settings })}>约定授权与免打扰</button>}<button className="social-button secondary" disabled={locked || !view.settings.enabled || !ais.length || creating} onClick={() => setCreating(true)}>新建一次性约定</button></div>
      {settings && <SettingsEditor value={settings} state={state} done={() => setSettings(null)} />}{creating && <IntentEditor ais={ais} state={state} done={() => setCreating(false)} />}
      {!view.intents.length && <p>还没有一次性约定。</p>}{view.intents.map(intent => <article key={intent.id} className="memory-card cloud-contact-intent"><strong>{conversation.members.find(m => m.account.id === intent.aiId)?.account.name || "AI"} · {intentStatuses[intent.status]}</strong><p className="memory-text">{intent.cryptoError || intent.content}</p>{conversation.encryption && !intent.sealed && !intent.cryptoError && <small>旧明文约定 · 尚未迁移</small>}<p>{new Date(intent.dueAt).toLocaleString()} 联系 · {new Date(intent.expiresAt).toLocaleString()} 失效<br />创建时区 {offsetLabel(intent.utcOffsetMinutes)} · {intent.creatorId === operatorId ? "我创建的约定" : `创建者：${conversation.members.find(m => m.account.id === intent.creatorId)?.account.name || intent.creatorId}`}</p>{intent.reason && <p>{participationReason(intent.reason)}</p>}{canCancelIntent(intent, operatorId, view.canManage) && <button className="text-button" disabled={locked} onClick={() => void state.execute(`/contact-intents/${intent.id}/cancel`, "POST", { expectedRevision: intent.revision }, "取消一次性约定")}>取消这次约定</button>}</article>)}
    </>}
  </section>;
}
