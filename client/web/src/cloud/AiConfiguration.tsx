import { useEffect, useRef, useState } from "react";
import { HttpError, request, server } from "./api";
import type { AgentManagementView, ManagedAi, SaveAgentProfile } from "./types";

type Pending = { requestId: string; operation: SaveAgentProfile };
// Keep the exact unconfirmed save across navigation; never persist private settings in the browser.
const pendingSaves = new Map<string, Pending>();
export const clearAiConfigurationCommands = () => pendingSaves.clear();

export function useAiConfiguration(aiId: string, operatorId: string) {
  const key = JSON.stringify([server, operatorId, aiId]);
  const pending = useRef<Pending | null>(pendingSaves.get(key) ?? null);
  const [view, setView] = useState<AgentManagementView | null>(null);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const [uncertain, setUncertain] = useState(!!pending.current);
  const [reviewed, setReviewed] = useState(false);
  const [saved, setSaved] = useState(false);
  const sequence = useRef(0);
  const path = `/ai/${aiId}/management`;
  const clearPending = () => { pendingSaves.delete(key); pending.current = null; setUncertain(false); setReviewed(false); };
  const load = async () => {
    const ticket = ++sequence.current;
    setBusy(true); setError(""); setView(null); setReviewed(false); setSaved(false);
    try {
      const result = await request<AgentManagementView>(path, "POST", { requestId: crypto.randomUUID(), operation: { type: "view" } });
      if (ticket === sequence.current) { setView(result); setReviewed(!!pending.current); }
    } catch (e) {
      if (ticket === sequence.current) {
        setError(e instanceof Error ? e.message : String(e));
        if (e instanceof HttpError && [401, 403].includes(e.status)) clearPending();
      }
    } finally { if (ticket === sequence.current) setBusy(false); }
  };
  useEffect(() => { void load(); return () => { sequence.current++; }; }, [path]);
  const save = async (operation?: SaveAgentProfile) => {
    if (busy) return;
    const first = !pending.current;
    if (!pending.current) {
      if (!operation) return;
      pending.current = { requestId: crypto.randomUUID(), operation };
      pendingSaves.set(key, pending.current);
    }
    const ticket = ++sequence.current;
    setBusy(true); setError(""); setUncertain(true); setReviewed(false); setSaved(false);
    const command = pending.current;
    try {
      const result = await request<AgentManagementView>(path, "POST", command);
      if (pendingSaves.get(key)?.requestId === command.requestId) pendingSaves.delete(key);
      pending.current = null;
      if (ticket === sequence.current) { setView(result); setUncertain(false); setSaved(true); }
    } catch (e) {
      if (ticket === sequence.current) {
        setError(e instanceof Error ? e.message : String(e));
        if (e instanceof HttpError && [401, 403].includes(e.status)) { setView(null); clearPending(); }
        else if (first && e instanceof HttpError && ["node_offline", "relay_busy", "unsupported_capability", "revision_conflict", "model_invalid", "profile_invalid"].includes(e.code ?? "")) clearPending();
      }
    } finally { if (ticket === sequence.current) setBusy(false); }
  };
  return { view, error, busy, uncertain, reviewed, saved, load, save, finishReview: () => { if (reviewed) clearPending(); } };
}

function ProfileEditor({ state }: { state: ReturnType<typeof useAiConfiguration> }) {
  const { profile, models } = state.view!;
  const [choice, setChoice] = useState(JSON.stringify([profile.providerId, profile.modelId]));
  const [effort, setEffort] = useState(profile.reasoningEffort ?? "");
  const model = models.find(m => JSON.stringify([m.providerId, m.modelId]) === choice);
  return <form className="cloud-form cloud-ai-form" onSubmit={e => {
    e.preventDefault();
    if (!model) return;
    const fields = new FormData(e.currentTarget);
    const number = (name: string) => { const value = String(fields.get(name) ?? "").trim(); return value === "" ? null : Number(value); };
    void state.save({ type: "saveProfile", expectedRevision: profile.revision, persona: String(fields.get("persona")), providerId: model.providerId, modelId: model.modelId, reasoningEffort: effort || null, temperature: number("temperature"), maxTokens: number("maxTokens"), autoDailyTokens: number("autoDailyTokens") });
  }}>
    <fieldset disabled={state.busy || state.uncertain} className="social-form-fields">
      <label>角色设定<textarea name="persona" defaultValue={profile.persona} maxLength={32768} rows={7} placeholder="它如何介绍自己、如何与你交流" /></label>
      <label>模型<select value={choice} onChange={e => { setChoice(e.target.value); setEffort(""); }} required>
        {!model && <option value={choice}>{profile.providerId} / {profile.modelId}（当前不可用）</option>}
        {models.map(m => <option key={JSON.stringify([m.providerId, m.modelId])} value={JSON.stringify([m.providerId, m.modelId])}>{m.name} · {m.providerId} / {m.modelId}</option>)}
      </select></label>
      {model && <p className="cloud-field-help">图片输入：{{ supported: "支持", unsupported: "不支持", unknown: "能力未知" }[model.imageInput]}</p>}
      <div className="cloud-ai-form-grid">
        <label>思考强度<select value={effort} onChange={e => setEffort(e.target.value)}>
          <option value="">模型默认</option>
          {effort && !model?.reasoningEfforts.includes(effort) && <option value={effort}>{effort}（当前不可用）</option>}
          {model?.reasoningEfforts.map(item => <option key={item} value={item}>{item}</option>)}
        </select></label>
        <label>温度<input name="temperature" type="number" step="any" min={0} max={2} defaultValue={profile.temperature ?? ""} placeholder="模型默认" /></label>
        <label>单次输出上限<input name="maxTokens" type="number" min={1} step={1} defaultValue={profile.maxTokens ?? ""} placeholder="自动分配" /></label>
        <label>每日自动联系 token 额度<input name="autoDailyTokens" type="number" min={1024} max={4000000} step={1} defaultValue={profile.autoDailyTokens ?? ""} placeholder="节点默认" /></label>
      </div>
      <p className="cloud-field-help">保存到运行节点后，新回复会采用这些设置。已经开始的回复继续使用原设置。</p>
      <button className="social-button primary" disabled={!model} type="submit">{state.busy ? "正在保存到节点…" : "保存到节点"}</button>
    </fieldset>
  </form>;
}

function OnlineConfiguration({ ai, operatorId }: { ai: ManagedAi; operatorId: string }) {
  const state = useAiConfiguration(ai.account.id, operatorId);
  return <div className="cloud-ai-section">
    <div className="cloud-ai-section-heading"><h2>角色与模型</h2><button className="text-button" disabled={state.busy} onClick={() => void state.load()}>重新读取</button></div>
    <p>这些资料保存在 {ai.node!.name}。模型密钥在该设备上管理。</p>
    {state.error && <p className="social-error" role="alert">{state.error}</p>}
    {state.busy && <p role="status">正在与节点确认…</p>}
    {state.saved && <p role="status">节点已保存。新的回复将采用这些设置。</p>}
    {state.uncertain && <div className="cloud-unknown-command" role="status">
      <p>上次保存的结果尚未确认，请重试原操作，或读取当前设置并核对。</p>
      <div className="cloud-actions"><button className="social-button secondary" disabled={state.busy} onClick={() => void state.save()}>重试原操作</button><button className="text-button" disabled={state.busy} onClick={() => void state.load()}>读取当前设置</button></div>
      {state.reviewed && state.view && <button className="text-button" onClick={state.finishReview}>已核对下方设置，结束原操作</button>}
    </div>}
    {state.view && <ProfileEditor key={`${state.view.profile.revision}/${state.reviewed}`} state={state} />}
  </div>;
}

export function AiConfiguration(props: { ai: ManagedAi; operatorId: string }) {
  if (!props.ai.node?.online) return <div className="cloud-ai-section"><h2>角色与模型</h2><p>节点离线。启动运行 Agent 的设备后，即可读取和修改设置。</p></div>;
  if (!props.ai.node.capabilities.includes("profileManagement")) return <div className="cloud-ai-section"><h2>角色与模型</h2><p>这个 Agent 没有提供在线配置入口，请在它所在的设备上管理。</p></div>;
  return <OnlineConfiguration key={props.ai.account.id} {...props} />;
}
