import { useEffect, useRef, useState } from "react";
import { cryptoDevice } from "./engine";
import { request, server, HttpError } from "../api";
import { loadResources, decryptResource, pendingResources, saveResource, synchronizeResources, discardResourceIntent, resourceStatus, type Resource, type ResourceKind } from "./resources";
import type { Conversation, SourceRevision } from "../types";

export function EncryptedResourcePanel({ operatorId, aiId, kind, conversation, source }: { operatorId: string; aiId: string; kind: ResourceKind; conversation?: Conversation; source?: SourceRevision }) {
  const cid = kind === "profile" ? null : conversation!.id;
  const [items, setItems] = useState<{ resource: Resource; data: Record<string, any> | null }[]>([]);
  const [pending, setPending] = useState<Awaited<ReturnType<typeof pendingResources>>>([]);
  const [pendingEdits, setPendingEdits] = useState<Record<string, Record<string, any> | null>>({});
  const [editing, setEditing] = useState<{ resource?: Resource; data: Record<string, any> | null } | null>(null);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const [offline, setOffline] = useState(false);
  const [reviewed, setReviewed] = useState(false);
  const sequence = useRef(0);
  const load = async () => {
    const ticket = ++sequence.current; setBusy(true); setError("");
    try {
      const result = await loadResources(operatorId, aiId, kind, cid);
      const decoded = await Promise.all(result.items.map(async resource => ({ resource, data: await decryptResource<Record<string, any>>(resource) })));
      const commands = (await pendingResources(operatorId)).filter(p => p.body.aiId === aiId && p.body.kind === kind && p.body.conversationId === cid);
      const localEdits = Object.fromEntries(await Promise.all(commands.map(async command => [command.id, await decryptResource<Record<string, any>>({ id: command.id, ...command.body, revision: (BigInt(command.body.expectedRevision) + 1n).toString(), applied: null })])));
      if (ticket !== sequence.current) return;
      setItems(decoded); setPending(commands); setPendingEdits(localEdits); setOffline(result.offline); setReviewed(!result.offline);
    } catch (e) { if (ticket === sequence.current) setError(e instanceof Error ? e.message : String(e)); }
    finally { if (ticket === sequence.current) setBusy(false); }
  };
  useEffect(() => { void load(); const timer = setInterval(() => { if (!editing) void load(); }, 10000); return () => { sequence.current++; clearInterval(timer); }; }, [operatorId, aiId, kind, cid, !!editing]);
  const act = async (work: () => Promise<unknown>) => { setBusy(true); setError(""); try { await work(); await load(); } catch (e) { setError(e instanceof Error ? e.message : String(e)); } finally { setBusy(false); } };
  const users = async () => {
    const key = `llmn-crypto-participants:${JSON.stringify([server, operatorId, aiId, cid, kind])}`;
    let audience: { accounts: string[]; revision: string };
    try { audience = await request<{ accounts: string[]; revision: string }>(`/crypto/resource-participants?aiId=${aiId}&kind=${kind}${cid ? `&conversationId=${cid}` : ""}`); localStorage.setItem(key, JSON.stringify(audience)); }
    catch (e) { if (e instanceof HttpError) throw e; const saved = localStorage.getItem(key); if (!saved) throw e; audience = JSON.parse(saved); }
    return audience;
  };
  const submit = async (form: HTMLFormElement) => {
    const fields = new FormData(form), previous = editing?.resource;
    const id = previous?.id ?? crypto.randomUUID(), expectedRevision = previous?.revision ?? "0", next = (BigInt(expectedRevision) + 1n).toString();
    const number = (name: string) => String(fields.get(name) ?? "").trim() ? Number(fields.get(name)) : null;
    let data: Record<string, unknown>;
    if (kind === "profile") data = { revision: next, persona: String(fields.get("persona")), selection: { provider: String(fields.get("provider")), model: String(fields.get("model")), reasoning_effort: String(fields.get("effort") || "") || undefined }, temperature: number("temperature"), max_tokens: number("maxTokens"), auto_daily_tokens: number("autoTokens") };
    else if (kind === "memory") data = { id, revision: next, kind: String(fields.get("memoryKind")), text: String(fields.get("text")), status: "confirmed", source: source ?? editing?.data?.source ?? null, createdAt: editing?.data?.createdAt ?? new Date().toISOString(), updatedAt: new Date().toISOString() };
    else data = { revision: next, enabled: fields.has("enabled"), binding: fields.has("enabled") ? { providerId: String(fields.get("provider")), modelId: String(fields.get("model")), reasoningEffort: String(fields.get("effort") || "") || null, maxTokens: number("maxTokens") } : null };
    const audience = await users();
    await saveResource(operatorId, { id, aiId, conversationId: cid, kind, expectedRevision, expectedAudienceRevision: audience.revision, users: audience.accounts, sources: data.source ? [data.source as SourceRevision] : previous?.sources ?? [] }, data);
    setEditing(null);
  };
  if (!cryptoDevice(operatorId)) return <p role="status">此范围采用密文资料。请先在“我的账号 → 可信设备与恢复”解锁并获得授权，登录不会自动获得解密能力。</p>;
  const name = { profile: "加密角色与模型", memory: "加密记忆", summary: "加密窗口摘要", summarySettings: "加密摘要设置" }[kind];
  const singleton = kind === "profile" || kind === "summarySettings";
  const describe = (value: Record<string, any> | null) => !value ? "停用/遗忘意图，不包含正文" : kind === "profile" ? `${value.persona}\n模型：${value.selection?.provider} / ${value.selection?.model}\n温度：${value.temperature ?? "默认"}；输出上限：${value.max_tokens ?? "自动"}` : kind === "memory" || kind === "summary" ? String(value.text ?? "") : `摘要${value.enabled ? "开启" : "关闭"}；模型：${value.binding?.providerId ?? ""} / ${value.binding?.modelId ?? ""}`;
  return <section className="cloud-ai-section"><div className="cloud-ai-section-heading"><h3>{name}</h3><button className="text-button" disabled={busy} onClick={() => void load()}>读取当前版本</button></div>
    <p>保存与节点应用分别确认。{offline ? "当前显示本机缓存；保存的意图会留在此设备，联网后同步。" : "节点离线也可保存密文；模型和参数由节点应用时严格校验。"}</p>
    {error && <p role="alert" className="social-error">{error}</p>}
    {pending.map(command => <div className="cloud-unknown-command" role="status" key={command.id}><p>{command.conflict ? `需要处理冲突：${command.conflict}` : "已保存在此设备，云端保存结果待同步或确认。"}</p><p>本机保存的编辑（基于版本 {command.body.expectedRevision}）：</p><textarea aria-label="本机待同步编辑" readOnly value={describe(pendingEdits[command.id] ?? null)} /><button className="text-button" onClick={() => void navigator.clipboard.writeText(describe(pendingEdits[command.id] ?? null))}>复制本机编辑</button><p>下方同时显示当前确认版本；冲突不会覆盖你的编辑。</p><button className="text-button" disabled={busy || !!command.conflict} onClick={() => void act(() => synchronizeResources(operatorId))}>重试原保存</button><button className="text-button" disabled={busy || !reviewed} onClick={() => { if (window.confirm("已核对下方云端当前资料，确认放弃这份本机保存意图？已被云端接受的版本不会被撤销。")) void act(() => discardResourceIntent(operatorId, command.id)); }}>已核对当前版本，放弃本机意图</button></div>)}
    {items.map(({ resource, data }) => <article className="memory-card" key={resource.id}><small>v{resource.revision} · {resourceStatus(resource)}</small>{resource.tombstone ? <><p>{resource.tombstone === "forgotten" ? "记忆已停用" : "来源已失效"}</p>{kind === "summarySettings" && resource.tombstone === "stale" && <button className="text-button" disabled={busy || pending.some(p => p.id === resource.id)} onClick={() => setEditing({ resource, data: null })}>重新设置摘要</button>}</> : <>{kind === "profile" ? <><p>{data?.persona}</p><small>{data?.selection?.provider} / {data?.selection?.model}</small></> : kind === "summarySettings" ? <p>{data?.enabled ? "摘要已开启" : "摘要已关闭"} · {data?.binding?.modelId}</p> : <p className="memory-text">{kind === "summary" ? data?.item?.text : data?.text}</p>}{kind !== "summary" && <button className="text-button" disabled={busy || pending.some(p => p.id === resource.id)} onClick={() => setEditing({ resource, data })}>编辑当前版本</button>}{kind === "memory" && <button className="text-button" disabled={busy || pending.some(p => p.id === resource.id)} onClick={() => { if (window.confirm("停用这条记忆？云端确认后会限制相关任务，离线节点须重连后清理。")) void act(async () => { const audience = await users(); await saveResource(operatorId, { id: resource.id, aiId, conversationId: cid, kind, expectedRevision: resource.revision, expectedAudienceRevision: audience.revision, users: audience.accounts, tombstone: "forgotten", sources: resource.sources }, null); }); }}>遗忘</button>}</>}</article>)}
    {!items.length && !busy && <p>{offline ? "本机没有已缓存的资料。" : "当前没有已发布的密文资料。已有节点资料须在可信授权后由节点加密发布，不从旧明文接口补读。"}</p>}
    {kind !== "summary" && (!singleton || !items.length) && <button className="social-button secondary" disabled={busy || !!pending.length} onClick={() => setEditing({ data: null })}>{singleton ? "明确创建新设定" : "添加记忆"}</button>}
    {editing && <form className="cloud-form" key={editing.resource?.revision ?? "new"} onSubmit={e => { e.preventDefault(); const form = e.currentTarget; void act(() => submit(form)); }}><fieldset className="social-form-fields" disabled={busy}>
      {kind === "memory" ? <><label>类别<select name="memoryKind" defaultValue={editing.data?.kind ?? "fact"}>{Object.entries({ preference: "偏好", fact: "事实", shared_experience: "共同经历", open_topic: "未完话题" }).map(([value, label]) => <option key={value} value={value}>{label}</option>)}</select></label><label>记忆正文<textarea name="text" defaultValue={editing.data?.text ?? ""} required maxLength={4000} /></label>{source && <p>来源 {source.messageId} · v{source.revision}</p>}</> : <>
        {kind === "profile" ? <label>角色设定<textarea name="persona" defaultValue={editing.data?.persona ?? ""} maxLength={20000} /></label> : <label><input name="enabled" type="checkbox" defaultChecked={!!editing.data?.enabled} />开启自动摘要</label>}
        <label>供应商 ID<input name="provider" required defaultValue={editing.data?.selection?.provider ?? editing.data?.binding?.providerId ?? ""} /></label><label>模型 ID<input name="model" required defaultValue={editing.data?.selection?.model ?? editing.data?.binding?.modelId ?? ""} /></label><label>推理强度<input name="effort" defaultValue={editing.data?.selection?.reasoning_effort ?? editing.data?.binding?.reasoningEffort ?? ""} /></label>
        {kind === "profile" && <label>温度<input name="temperature" type="number" step="any" min={0} max={2} defaultValue={editing.data?.temperature ?? ""} /></label>}
        <label>输出上限<input name="maxTokens" type="number" min={1} defaultValue={editing.data?.max_tokens ?? editing.data?.binding?.maxTokens ?? ""} required={kind === "summarySettings"} /></label>
        {kind === "profile" && <label>每日自动联系 token<input name="autoTokens" type="number" min={1024} max={4000000} defaultValue={editing.data?.auto_daily_tokens ?? ""} /></label>}
        <p>这里不接收模型 Key 或 provider 地址；节点不支持新配置时显示“无法应用”，不会自动换模型。</p>
      </>}
      <button className="social-button primary" type="submit">保存密文意图</button><button type="button" className="text-button" onClick={() => setEditing(null)}>取消未保存输入</button>
    </fieldset></form>}
  </section>;
}
