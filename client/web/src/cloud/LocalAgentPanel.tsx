import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { request } from "./api";
import type { ManagedAi } from "./types";

interface LocalConfiguration {
  nodeName: string; provider: string; model: string;
  protocol?: string | null; baseUrl?: string | null; persona?: string | null;
  temperature?: number | null; maxTokens?: number | null; reasoningEffort?: string | null;
  apiKeyEnv?: string | null; hasSavedKey: boolean;
}
interface LocalStatus {
  configured: boolean; running: boolean; nodeId: string | null;
  configuration: LocalConfiguration | null;
  pairing: { pairingId: string; userCode: string; expiresAt: string } | null;
  aiIds: string[]; error: string | null;
}
interface Catalog { providers: { id: string; name: string; models: { id: string; name: string; contextWindow: number; reasoningEfforts: string[] }[] }[] }

function LocalSetup({ configuration, catalog, busy, save }: { configuration: LocalConfiguration | null; catalog: Catalog; busy: boolean; save(value: object): Promise<boolean> }) {
  const [providerId, setProvider] = useState(configuration?.provider ?? "");
  const [modelId, setModel] = useState(configuration?.model ?? "");
  const [secretKind, setSecretKind] = useState(configuration?.hasSavedKey ? "keep" : configuration?.apiKeyEnv ? "env" : "key");
  const provider = catalog.providers.find(p => p.id === providerId);
  const [key, setKey] = useState("");
  return <form className="cloud-form cloud-ai-form" onSubmit={async e => {
    e.preventDefault(); const fields = new FormData(e.currentTarget);
    const options = { nodeName: String(fields.get("nodeLabel")).trim(), provider: providerId, model: modelId, ...(configuration && providerId === configuration.provider ? { persona: configuration.persona, temperature: configuration.temperature, maxTokens: configuration.maxTokens, ...(modelId === configuration.model ? { reasoningEffort: configuration.reasoningEffort } : {}) } : {}), protocol: String(fields.get("protocol") ?? ""), baseUrl: String(fields.get("baseUrl") ?? "").trim(), ...(secretKind === "key" ? { apiKey: key } : secretKind === "env" ? { apiKeyEnv: String(fields.get("apiKeyEnv")).trim() } : {}) };
    if (await save(options)) setKey("");
  }}>
    <fieldset disabled={busy} className="social-form-fields">
      <label>这台设备的节点名称<input name="nodeLabel" required maxLength={80} defaultValue={configuration?.nodeName ?? "我的电脑"} /></label>
      <label>模型服务<select value={providerId} required onChange={e => { setProvider(e.target.value); setModel(""); setSecretKind("key"); setKey(""); }}><option value="" disabled>选择模型服务</option>{catalog.providers.map(p => <option key={p.id} value={p.id}>{p.name}</option>)}</select></label>
      <label>初始模型<select value={modelId} required onChange={e => setModel(e.target.value)}><option value="" disabled>选择模型</option>{provider?.models.map(m => <option key={m.id} value={m.id}>{m.name} · {m.id}</option>)}</select></label>
      <label>模型密钥<select value={secretKind} onChange={e => { setSecretKind(e.target.value); setKey(""); }}>
        {configuration?.hasSavedKey && providerId === configuration.provider && <option value="keep">继续使用本机保存的密钥</option>}
        <option value="key">在这台电脑上填写密钥</option><option value="env">使用本机环境变量</option>
      </select></label>
      {secretKind === "key" && <label>API Key<input value={key} onChange={e => setKey(e.target.value)} type="password" autoComplete="new-password" spellCheck={false} required placeholder="仅保存在本机" /></label>}
      {secretKind === "env" && <label>环境变量名<input name="apiKeyEnv" defaultValue={configuration?.apiKeyEnv ?? ""} required pattern="[A-Za-z_][A-Za-z0-9_]*" spellCheck={false} placeholder="例如 API_KEY_CHATECNU" /></label>}
      <details className="cloud-local-advanced" key={providerId}><summary>连接地址与协议</summary><p>使用服务默认值即可。需要兼容网关或本机服务时，再填写服务地址和协议。</p><label>服务地址<input name="baseUrl" type="url" defaultValue={providerId === configuration?.provider ? configuration.baseUrl ?? "" : ""} placeholder="使用服务默认地址" /></label><label>接口协议<select name="protocol" defaultValue={providerId === configuration?.provider ? configuration.protocol ?? "" : ""}><option value="">服务默认</option><option value="openai">OpenAI Chat Completions</option><option value="openai_responses">OpenAI Responses</option><option value="anthropic">Anthropic Messages</option><option value="gemini">Google Gemini</option><option value="ollama">Ollama</option></select></label></details>
      <p>密钥只交给本机 Agent，不经过社交服务器。初始模型用于新配对的 AI；已配对 AI 的角色与模型在“管理 → 角色与模型”中调整。</p>
      <button className="social-button primary">{busy ? "正在保存…" : "保存本机配置"}</button>
    </fieldset>
  </form>;
}

export function LocalAgentPanel({ ai, changed }: { ai: ManagedAi; changed(): Promise<unknown> }) {
  const [status, setStatus] = useState<LocalStatus | null>(null);
  const [catalog, setCatalog] = useState<Catalog | null>(null);
  const [editing, setEditing] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const alive = useRef(true);
  const reads = useRef(0);
  const command = useRef({ signature: "", id: "" });
  const statusRef = useRef(status); statusRef.current = status;
  const load = async () => {
    const ticket = ++reads.current;
    const result = await invoke<LocalStatus>("local_agent_status");
    if (alive.current && ticket === reads.current) {
      const paired = statusRef.current?.pairing && !result.pairing;
      setStatus(result);
      if (paired) await changed();
    }
  };
  useEffect(() => {
    alive.current = true;
    const refresh = () => void load().catch(e => { if (alive.current) setError(String(e)); });
    refresh();
    void invoke<Catalog>("local_agent_models").then(value => { if (alive.current) setCatalog(value); }, e => { if (alive.current) setError(String(e)); });
    const timer = setInterval(refresh, 2000);
    return () => { alive.current = false; reads.current++; clearInterval(timer); };
  }, []);
  const act = async (commandName: string, args?: Record<string, unknown>) => {
    setBusy(true); setError(""); setNotice(""); reads.current++;
    try {
      const result = await invoke<LocalStatus>(commandName, args);
      if (alive.current) { setStatus(result); if (commandName === "local_agent_setup") setEditing(false); }
      await changed(); return true;
    } catch (e) { if (alive.current) setError(String(e)); return false; }
    finally { if (alive.current) setBusy(false); }
  };
  const boundHere = !!status?.nodeId && ai.node?.id === status.nodeId && status.aiIds.includes(ai.account.id);
  return <div className="cloud-local-agent">
    <p>让 {ai.account.name} 在这台电脑上运行。聊天保存在云端，角色、模型密钥与意识资料保存在本机。</p>
    {error && <p className="social-error" role="alert">{error}</p>}
    {status?.error && <p className="social-error" role="alert">{status.error}</p>}
    {notice && <p role="status">{notice}</p>}
    {!status && <p role="status">正在检查本机 Agent…</p>}
    {status && <>
      <div className="cloud-local-agent-state"><strong>{status.running ? "本机 Agent 正在运行" : status.configured ? "本机 Agent 已停止" : "尚未配置本机 Agent"}</strong><span>{boundHere ? "这个 AI 已连接本机节点" : "这个 AI 尚未连接本机节点"}</span></div>
      {status.running && <><p>关闭窗口后，Agent 可以继续在后台回复。电脑休眠、关机或停止 Agent 后，本机 AI 会离线。</p><button className="social-button secondary" disabled={busy} onClick={() => void act("local_agent_stop")}>停止本机 Agent</button><p>停止会影响这个账号在本机节点运行的所有 AI；其他设备上的节点继续运行。</p></>}
      {!status.running && !status.pairing && <>
        {(!status.configured || editing) && catalog && <LocalSetup key={status.configuration?.provider ?? "new"} configuration={status.configuration} catalog={catalog} busy={busy} save={value => act("local_agent_setup", { options: value })} />}
        {status.configured && !editing && <div className="cloud-actions"><button className="social-button secondary" disabled={busy} onClick={() => setEditing(true)}>修改本机配置与密钥</button><button className="social-button secondary" disabled={busy} onClick={() => void act("local_agent_pair")}>{boundHere ? "重新配对这个 AI" : "准备连接这个 AI"}</button>{status.aiIds.length > 0 && <button className="social-button primary" disabled={busy} onClick={() => void act("local_agent_start")}>启动本机 Agent</button>}</div>}
        {editing && <button className="text-button" disabled={busy} onClick={() => setEditing(false)}>取消修改</button>}
      </>}
      {status.pairing && <div className="cloud-local-pairing"><p>本机节点已准备好，请授权它以 {ai.account.name} 的身份运行。</p>{ai.node && ai.node.id !== status.nodeId && <p>授权后，旧节点将失去这个 AI 的执行资格。更换节点不会自动迁移它的角色和意识资料。</p>}<p>配对码 <strong>{status.pairing.userCode}</strong> · {new Date(status.pairing.expiresAt).toLocaleTimeString()} 到期</p>
        <button className="social-button primary" disabled={busy} onClick={async () => {
          const body = { pairingCode: status.pairing!.userCode, expectedBindingVersion: ai.bindingVersion };
          const signature = JSON.stringify(body);
          if (command.current.signature !== signature) command.current = { signature, id: crypto.randomUUID() };
          setBusy(true); setError("");
          try { await request(`/ai/${ai.account.id}/node`, "POST", { ...body, requestId: command.current.id }); command.current = { signature: "", id: "" }; if (alive.current) setNotice("已授权，正在等待本机保存连接。完成后可启动 Agent。"); await load(); await changed(); }
          catch (e) { if (alive.current) setError(e instanceof Error ? e.message : String(e)); }
          finally { if (alive.current) setBusy(false); }
        }}>授权本机节点</button>
      </div>}
    </>}
  </div>;
}
