import { useEffect, useRef, useState } from "react";
import { Dialog, DialogContent, DialogTitle } from "@/components/ui/dialog";
import { HttpError, request, server } from "./api";
import type { Account, ConsciousnessOperation, ConsciousnessView, Conversation, MemoryItem, MemoryKind, Message, Page, SourceRevision, SummarySettings, SummaryModelChoice } from "./types";
import { EncryptedResourcePanel } from "./crypto/ResourcePanel";

const kinds: Record<MemoryKind, string> = { preference: "偏好", fact: "事实", shared_experience: "共同经历", open_topic: "未完话题" };
const statuses = { queued: "等待整理", running: "正在整理", ready: "可用于此 AI 的上下文", failed: "整理失败", interrupted: "已中断", stale: "来源已变化 · 暂不使用" };

const summaryErrors: Record<string, string> = {
  source_changed: "来源消息已修改或撤回，请按当前原文重新整理。",
  window_changed: "窗口成员或权限已变化，请重新核对来源。",
  settings_changed: "摘要设置已修改，本次整理已停止。",
  node_restarted: "节点已重启，本次整理已中断。",
  summary_failed: "摘要整理失败，请检查节点配置后重试。",
  summary_interrupted: "摘要整理已中断，可手动重试。",
};

type PendingOperation = { requestId: string; operation: ConsciousnessOperation };
// Retain an unconfirmed command while its editor is temporarily unmounted by navigation.
const pendingOperations = new Map<string, PendingOperation>();
export const clearConsciousnessCommands = () => pendingOperations.clear();
export function useConsciousness(conversationId: string, aiId: string, type: "memories" | "summaries", cursor: string, operatorId: string) {
  const commandKey = JSON.stringify([server, operatorId, conversationId, aiId, type]);
  const restored = pendingOperations.get(commandKey);
  const [view, setView] = useState<ConsciousnessView | null>(null);
  const [error, setError] = useState("");
  const [loading, setLoading] = useState(false);
  const [uncertain, setUncertain] = useState(!!restored);
  const [busy, setBusy] = useState(false);
  const [completed, setCompleted] = useState(0);
  const [reviewed, setReviewed] = useState(false);
  const sequence = useRef(0);
  const pending = useRef<PendingOperation | null>(restored || null);
  const path = `/chat/conversations/${conversationId}/ai/${aiId}/consciousness`;
  const load = async () => {
    const ticket = ++sequence.current;
    setLoading(true); setError(""); setView(null);
    try {
      const data = await request<ConsciousnessView>(path, "POST", { requestId: crypto.randomUUID(), operation: { type } });
      if (ticket === sequence.current) { setView(data); if (pending.current) setReviewed(true); }
    } catch (e) { if (ticket === sequence.current) setError(e instanceof Error ? e.message : String(e)); }
    finally { if (ticket === sequence.current) setLoading(false); }
  };
  useEffect(() => { void load(); return () => { sequence.current++; }; }, [path, type, cursor]);
  const execute = async (operation?: ConsciousnessOperation) => {
    if (busy) return false;
    const firstAttempt = !pending.current;
    if (!pending.current) {
      if (!operation) return false;
      pending.current = { requestId: crypto.randomUUID(), operation };
      pendingOperations.set(commandKey, pending.current);
    }
    setBusy(true); setUncertain(true); setReviewed(false); setError("");
    const ticket = ++sequence.current;
    try {
      const data = await request<ConsciousnessView>(path, "POST", pending.current);
      if (ticket === sequence.current) { setView(data); setLoading(false); }
      pendingOperations.delete(commandKey); pending.current = null; setUncertain(false); setCompleted(value => value + 1);
      return true;
    } catch (e) {
      if (firstAttempt && e instanceof HttpError && (e.status === 400 || ["node_offline", "relay_busy", "source_changed", "revision_conflict", "model_invalid", "invalid_memory", "summary_model_required"].includes(e.code || ""))) { pendingOperations.delete(commandKey); pending.current = null; setUncertain(false); }
      setError(e instanceof Error ? e.message : String(e)); return false;
    }
    finally { setBusy(false); }
  };
  const releaseReviewed = () => {
    if (!reviewed) return;
    pendingOperations.delete(commandKey); pending.current = null; setUncertain(false); setReviewed(false); setError(""); setCompleted(value => value + 1);
  };
  return { view, error, loading, uncertain, busy, completed, reviewed, operation: pending.current?.operation, load, execute, releaseReviewed };
}
type State = ReturnType<typeof useConsciousness>;
function RequestState({ state }: { state: State }) {
  return <>{state.error && <p className="social-error" role="alert">{state.error}{!state.uncertain && <> 请根据错误重新读取或调整内容。<button className="text-button" disabled={state.busy} onClick={() => void state.load()}>重新读取</button></>}</p>}{state.uncertain && !state.busy && <div className="cloud-unknown-command" role="status"><p>保存结果尚未确认。重试会使用原命令。</p>{state.operation && "text" in state.operation && <blockquote>{state.operation.text}</blockquote>}<button className="text-button" onClick={() => void state.execute()}>重试原操作</button><button className="text-button" disabled={state.loading} onClick={() => void state.load()}>核对节点当前资料</button>{state.reviewed && state.view && <><div className="memory-card"><p>以下是重新读取的节点当前资料，请核对原操作是否已生效：</p>{state.view.memories.filter(m => m.status !== "forgotten").map(m => <p key={m.id}>{kinds[m.kind]} · {m.text} · v{m.revision}</p>)}{state.view.settings && <p>自动摘要{state.view.settings.enabled ? "已开启" : "未开启"} · {state.view.settings.binding?.modelId || "未配置模型"}</p>}{state.view.summaries.map(summary => <p key={summary.id}>{statuses[summary.status]} · {summary.fromPosition}–{summary.throughPosition}</p>)}</div><button className="social-button secondary" onClick={state.releaseReviewed}>已核对当前资料，结束原操作</button><p>结束后不再重试原命令，实际结果以刚核对的节点资料为准。</p></>}</div>}{state.loading && <p role="status">正在读取在线节点…</p>}</>;
}

function SourceView({ conversationId, source, cursor, adopt }: { conversationId: string; source: SourceRevision; cursor: string; adopt?: (source: SourceRevision | null) => void }) {
  const [result, setResult] = useState<{ stamp: string; message: Message } | null>(null);
  const [error, setError] = useState("");
  const stamp = JSON.stringify([source, cursor]);
  const ticket = useRef(0);
  useEffect(() => { ticket.current++; setResult(null); setError(""); }, [stamp]);
  const current = result?.stamp === stamp ? result.message : null;
  return <div className="memory-source"><p>来源版本 {source.revision}</p><button type="button" className="text-button" onClick={async () => {
    const version = ++ticket.current; setResult(null); setError("");
    try {
      const page = await request<Page>(`/chat/messages/${source.messageId}/context`);
      const message = page.messages.find(m => m.id === source.messageId && m.conversationId === conversationId);
      if (!message) throw new Error("来源消息已不可读取");
      if (version === ticket.current) setResult({ stamp, message });
    } catch (e) { if (version === ticket.current) setError(e instanceof Error ? e.message : String(e)); }
  }}>核对当前原文</button>
    {error && <p role="alert" className="social-error">{error}</p>}
    {current && <blockquote>{current.senderName} · 当前版本 {current.revision}<p>{current.status === "withdrawn" ? "消息已撤回" : current.text || "[附件消息]"}</p>{adopt && current.status === "committed" && <button type="button" className="text-button" onClick={() => adopt({ messageId: current.id, revision: current.revision })}>已核对，采用当前版本</button>}</blockquote>}
    {adopt && <button type="button" className="text-button" onClick={() => adopt(null)}>移除依据，改为手动记忆</button>}
  </div>;
}

function MemoryEditor({ memory, message, state, conversationId, cursor, done }: { memory?: MemoryItem; message?: Message; state: State; conversationId: string; cursor: string; done(): void }) {
  const [text, setText] = useState(memory?.text ?? message?.text ?? "");
  const [kind, setKind] = useState<MemoryKind>(memory?.kind ?? "fact");
  const [source, setSource] = useState<SourceRevision | null>(memory?.source ?? (message ? { messageId: message.id, revision: message.revision } : null));
  const [confirmed, setConfirmed] = useState(memory?.status !== "stale");
  return <form className="cloud-form" onSubmit={async e => { e.preventDefault();
    const content = { kind, text, source };
    if (await state.execute(memory ? { type: "editMemory", id: memory.id, revision: memory.revision, ...content } : { type: "createMemory", ...content })) done();
  }}>
    <RequestState state={state} />
    <fieldset className="social-form-fields" disabled={state.busy || state.uncertain}>
      <label>类型<select value={kind} onChange={e => setKind(e.target.value as MemoryKind)}>{Object.entries(kinds).map(([id, label]) => <option key={id} value={id}>{label}</option>)}</select></label>
      <label>记忆内容<textarea required maxLength={4000} rows={4} value={text} onChange={e => setText(e.target.value)} /></label>
      {source ? <SourceView conversationId={conversationId} source={source} cursor={cursor} adopt={value => { setSource(value); setConfirmed(true); }} /> : <p>手动记忆，仅供此 AI 在当前窗口使用。</p>}
      {!confirmed && <p>来源已变化，请核对当前原文或明确改为手动记忆。</p>}
      <button className="social-button primary" disabled={!text.trim() || !confirmed}>保存记忆</button>
      <button type="button" className="social-button secondary" onClick={done}>取消</button>
    </fieldset>
  </form>;
}

function Memories({ conversationId, ai, cursor, operatorId, message, onLock }: { conversationId: string; ai: Account; cursor: string; operatorId: string; message?: Message; onLock?(locked: boolean): void }) {
  const state = useConsciousness(conversationId, ai.id, "memories", cursor, operatorId);
  const [editor, setEditor] = useState<MemoryItem | "new" | null>(message ? "new" : null);
  const [forget, setForget] = useState<MemoryItem | null>(null);
  useEffect(() => { onLock?.(state.uncertain); }, [state.uncertain]);
  useEffect(() => { if (state.completed) { setEditor(null); setForget(null); } }, [state.completed]);
  return <section className="cloud-detail-section live-consciousness"><h3>{ai.name} · 当前窗口记忆</h3><p>当前成员可维护此 AI 的显式记忆。私聊记忆不会自动进入群聊；只有已确认条目用于新回复。</p>
    {!editor && !forget && <RequestState state={state} />}
    <button className="social-button secondary" disabled={state.busy || state.uncertain || !state.view} onClick={() => setEditor("new")}>添加记忆</button>
    {state.view && !state.loading && !state.view.memories.filter(m => m.status !== "forgotten").length && <p>还没有显式记忆。</p>}
    {state.view?.memories.filter(m => m.status !== "forgotten").map(memory => <article className="memory-card" key={memory.id}><div className="memory-meta"><span>{kinds[memory.kind]}</span><span>{memory.status === "stale" ? "来源已变化 · 暂不使用" : "已确认"}</span></div><p className="memory-text">{memory.text}</p><small>版本 {memory.revision} · {new Date(memory.updatedAt).toLocaleString()}</small><div className="cloud-actions"><button className="text-button" disabled={state.uncertain || state.busy} onClick={() => setEditor(memory)}>编辑</button><button className="text-button" disabled={state.uncertain || state.busy} onClick={() => setForget(memory)}>遗忘</button></div>{memory.source && <SourceView conversationId={conversationId} source={memory.source} cursor={cursor} />}</article>)}
    <p className="memory-note">遗忘清除记忆正文及旧修订，原聊天、已发送回答和备份仍保留。</p>
    <Dialog open={editor !== null} onOpenChange={open => { if (!open && !state.uncertain) setEditor(null); }}><DialogContent className="social-dialog" aria-describedby={undefined}><DialogTitle>{editor === "new" ? "添加显式记忆" : "编辑显式记忆"}</DialogTitle>{editor && <MemoryEditor key={editor === "new" ? "new" : editor.id} memory={editor === "new" ? undefined : editor} message={editor === "new" ? message : undefined} conversationId={conversationId} cursor={cursor} state={state} done={() => setEditor(null)} />}</DialogContent></Dialog>
    <Dialog open={forget !== null} onOpenChange={open => { if (!open && !state.uncertain) setForget(null); }}><DialogContent className="social-dialog" aria-describedby={undefined}><DialogTitle>遗忘这条记忆？</DialogTitle><p>将清除记忆及修订正文，并停止依赖它的当前回复。原聊天保留。</p><RequestState state={state} /><button className="social-button primary" disabled={state.busy || state.uncertain} onClick={async () => { if (await state.execute({ type: "forgetMemory", id: forget!.id, revision: forget!.revision })) setForget(null); }}>确认遗忘</button><button className="social-button secondary" disabled={state.uncertain} onClick={() => setForget(null)}>取消</button></DialogContent></Dialog>
  </section>;
}

function SummarySettingsForm({ settings, models, state, done }: { settings: SummarySettings; models: SummaryModelChoice[]; state: State; done(): void }) {
  const [enabled, setEnabled] = useState(settings.enabled);
  const [model, setModel] = useState(settings.binding ? JSON.stringify([settings.binding.providerId, settings.binding.modelId]) : "");
  const [effort, setEffort] = useState(settings.binding?.reasoningEffort ?? "");
  const [tokens, setTokens] = useState(settings.binding?.maxTokens ?? 1024);
  const choice = models.find(m => JSON.stringify([m.providerId, m.modelId]) === model);
  return <form className="cloud-form" onSubmit={async e => { e.preventDefault();
    const binding = choice ? { providerId: choice.providerId, modelId: choice.modelId, reasoningEffort: effort || null, maxTokens: tokens } : model ? settings.binding : null;
    if (await state.execute({ type: "configureSummaries", revision: settings.revision, enabled, binding })) done();
  }}><RequestState state={state} /><fieldset className="social-form-fields" disabled={state.busy || state.uncertain}>
    <label className="cloud-check"><input type="checkbox" checked={enabled} onChange={e => setEnabled(e.target.checked)} />启用此 AI 在当前窗口的自动摘要</label>
    <label>摘要专用模型<select required={enabled} value={model} onChange={e => { setModel(e.target.value); setEffort(""); }}><option value="">请选择节点模型</option>{model && !choice && <option value={model}>当前模型不可用，请重新选择</option>}{models.map(m => <option key={JSON.stringify([m.providerId, m.modelId])} value={JSON.stringify([m.providerId, m.modelId])}>{m.name} · {m.providerId}</option>)}</select></label>
    {!!choice?.reasoningEfforts.length && <label>思考强度<select value={effort} onChange={e => setEffort(e.target.value)}><option value="">模型默认</option>{choice.reasoningEfforts.map(level => <option key={level}>{level}</option>)}</select></label>}
    <label>每段输出上限（tokens）<input type="number" required min={1} disabled={!choice} value={tokens} onChange={e => setTokens(Number(e.target.value))} /></label>
    <p>摘要会额外调用模型，使用节点预算。最近 12 条消息保留原文，每段最多整理 32 条。失败不会自动重试。</p>
    <button className="social-button primary" disabled={enabled && !choice}>保存摘要设置</button><button type="button" className="social-button secondary" onClick={done}>取消</button>
  </fieldset></form>;
}

function Summaries({ conversationId, ai, cursor, operatorId, onLock }: { conversationId: string; ai: Account; cursor: string; operatorId: string; onLock(locked: boolean): void }) {
  const state = useConsciousness(conversationId, ai.id, "summaries", cursor, operatorId);
  const [settings, setSettings] = useState<{ value: SummarySettings; models: SummaryModelChoice[] } | null>(null);
  useEffect(() => { onLock(state.uncertain); }, [state.uncertain]);
  useEffect(() => { if (state.completed) setSettings(null); }, [state.completed]);
  const running = state.view?.summaries.some(s => s.status === "queued" || s.status === "running");
  return <section className="cloud-detail-section summary-panel"><h3>{ai.name} · 私有窗口摘要</h3><p>这是此 AI 的内部资料，仅其管理员且仍有本窗口访问权时可管理。不会作为群共享摘要，也不等同于显式记忆。</p>
    {!settings && <RequestState state={state} />}
    {state.view && <><p>{state.view.settings?.enabled ? "自动摘要已开启" : "自动摘要未开启"}</p><div className="cloud-actions"><button className="social-button secondary" disabled={state.uncertain || state.busy || !state.view.settings} onClick={() => setSettings({ value: state.view!.settings!, models: state.view!.models })}>摘要设置</button><button className="social-button secondary" disabled={state.uncertain || state.busy || running || !state.view.settings?.enabled} onClick={() => void state.execute({ type: "generateSummary", retryId: null })}>整理下一段</button><button className="text-button" disabled={state.busy} onClick={() => void state.load()}>刷新摘要进度</button></div>
      {!state.view.summaries.length && <p>还没有此 AI 的窗口摘要。</p>}
      {state.view.summaries.map(summary => <article className="memory-card" key={summary.id}><div className="memory-meta"><span>位置 {summary.fromPosition}–{summary.throughPosition}</span><span>{statuses[summary.status]}</span>{summary.importOrigin && <span>导入摘要</span>}</div>{summary.status === "ready" && <p className="memory-text">{summary.text}</p>}{summary.error && <p className="social-error">{summaryErrors[summary.error] || summary.error}</p>}<details><summary>核对来源（{summary.sources.length}）</summary>{summary.sources.map(source => <SourceView key={source.messageId} conversationId={conversationId} source={source} cursor={cursor} />)}</details>{["failed", "interrupted", "stale"].includes(summary.status) && <button className="text-button" disabled={state.uncertain || state.busy || running || !state.view!.settings?.enabled} onClick={() => void state.execute({ type: "generateSummary", retryId: summary.id })}>按当前来源重试</button>}</article>)}
    </>}
    <Dialog open={settings !== null} onOpenChange={open => { if (!open && !state.uncertain) setSettings(null); }}><DialogContent className="social-dialog" aria-describedby={undefined}><DialogTitle>{ai.name} 的私有摘要设置</DialogTitle>{settings && <SummarySettingsForm settings={settings.value} models={settings.models} state={state} done={() => setSettings(null)} />}</DialogContent></Dialog>
  </section>;
}

export function ConsciousnessPanel({ conversation, cursor, operatorId, summaryAiIds, summaryError, onlyAi }: { conversation: Conversation; cursor: string; operatorId: string; summaryAiIds: string[] | null; summaryError: string; onlyAi?: string }) {
  const ais = conversation.members.filter(m => m.account.kind === "ai" && (!onlyAi || m.account.id === onlyAi)).map(m => m.account);
  const [selected, setSelected] = useState("");
  const [memoryLocked, setMemoryLocked] = useState(false);
  const [summaryLocked, setSummaryLocked] = useState(false);
  const ai = ais.find(a => a.id === selected) || ais[0];
  if (!ai) return <section className="cloud-detail-section"><p>当前窗口没有 AI 成员。</p></section>;
  if (conversation.encryption) return <section className="cloud-consciousness"><h2>加密意识资料</h2><EncryptedResourcePanel operatorId={operatorId} aiId={ai.id} kind="memory" conversation={conversation} />{summaryAiIds?.includes(ai.id) && <><EncryptedResourcePanel operatorId={operatorId} aiId={ai.id} kind="summarySettings" conversation={conversation} /><EncryptedResourcePanel operatorId={operatorId} aiId={ai.id} kind="summary" conversation={conversation} /></>}</section>;
  return <section className="cloud-consciousness"><h2>意识与记忆</h2>{ais.length > 1 && <label className="memory-owner">归属 AI<select value={ai.id} disabled={memoryLocked || summaryLocked} onChange={e => setSelected(e.target.value)}>{ais.map(a => <option key={a.id} value={a.id}>{a.name}</option>)}</select></label>}<Memories key={`${conversation.id}/${ai.id}`} conversationId={conversation.id} ai={ai} cursor={cursor} operatorId={operatorId} onLock={setMemoryLocked} />{summaryAiIds?.includes(ai.id) && <Summaries key={`summary/${conversation.id}/${ai.id}`} conversationId={conversation.id} ai={ai} cursor={cursor} operatorId={operatorId} onLock={setSummaryLocked} />}{summaryError && <p role="alert" className="social-error">摘要管理权限未能读取：{summaryError}</p>}{summaryAiIds && !summaryAiIds.includes(ai.id) && <p className="memory-note">此 AI 的私有摘要仅向其管理员开放。</p>}</section>;
}

export function RememberMessage({ conversation, message, cursor, operatorId }: { conversation: Conversation; message: Message; cursor: string; operatorId: string }) {
  const ais = conversation.members.filter(m => m.account.kind === "ai").map(m => m.account);
  const [open, setOpen] = useState(false);
  const [locked, setLocked] = useState(false);
  const [selected, setSelected] = useState("");
  const ai = ais.find(a => a.id === selected) || ais[0];
  if (!ai) return null;
  if (conversation.encryption) return <><button className="text-button" onClick={() => setOpen(true)}>记住</button><Dialog open={open} onOpenChange={setOpen}><DialogContent className="social-dialog" aria-describedby={undefined}><DialogTitle>加密保存这条消息的记忆</DialogTitle><EncryptedResourcePanel operatorId={operatorId} aiId={ai.id} kind="memory" conversation={conversation} source={{ messageId: message.id, revision: message.revision }} /></DialogContent></Dialog></>;
  return <><button className="text-button" onClick={() => setOpen(true)}>记住</button><Dialog open={open} onOpenChange={value => { if (!locked) setOpen(value); }}><DialogContent className="social-dialog" aria-describedby={undefined}><DialogTitle>让 AI 记住当前窗口的消息</DialogTitle><label>归属 AI<select disabled={locked} value={ai.id} onChange={e => setSelected(e.target.value)}>{ais.map(a => <option key={a.id} value={a.id}>{a.name}</option>)}</select></label>{open && <Memories key={ai.id} conversationId={conversation.id} ai={ai} cursor={cursor} operatorId={operatorId} message={message} onLock={setLocked} />}</DialogContent></Dialog></>;
}
