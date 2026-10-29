import { useEffect, useRef, useState } from "react";
import { ArrowLeft } from "lucide-react";
import { CUSTOM_WINDOW_CHROME } from "@/lib/desktop";
import { IconButton } from "../chat/components/common";
import { request } from "./api";
import { AiConfiguration } from "./AiConfiguration";
import { AiAccountView } from "./AiAccountView";
import { ConsciousnessPanel } from "./Consciousness";
import type { Account, Conversation, ManagedAi } from "./types";

type Administrators = { revision: string; administrators: { account: Account; role: "initial" | "admin" }[] };

function PublicProfile({ ai, changed }: { ai: ManagedAi; changed(): Promise<unknown> }) {
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const [saved, setSaved] = useState(false);
  const command = useRef({ signature: "", id: "" });
  return <div className="cloud-ai-section"><h2>公开资料</h2><p>这些资料会显示在通讯录和聊天里。</p>
    <form key={ai.profileRevision} className="cloud-form cloud-ai-form" onSubmit={async e => {
      e.preventDefault(); const data = new FormData(e.currentTarget);
      const body = { expectedProfileRevision: ai.profileRevision, name: String(data.get("name")).trim(), bio: String(data.get("bio")).trim() };
      const signature = JSON.stringify(body);
      if (command.current.signature !== signature) command.current = { signature, id: crypto.randomUUID() };
      setBusy(true); setError(""); setSaved(false);
      try { await request(`/ai/${ai.account.id}/profile`, "PUT", { ...body, requestId: command.current.id }); command.current = { signature: "", id: "" }; await changed(); setSaved(true); }
      catch (e) { setError(e instanceof Error ? e.message : String(e)); }
      finally { setBusy(false); }
    }}>
      <fieldset disabled={busy} className="social-form-fields">
        <label>名字<input name="name" defaultValue={ai.account.name} required maxLength={80} /></label>
        <label>公开简介<textarea name="bio" defaultValue={ai.account.bio} maxLength={2000} rows={4} /></label>
        <p className="cloud-field-help">账号 ID：{ai.account.id}</p>
        <button className="social-button primary">{busy ? "正在保存…" : "保存资料"}</button>
      </fieldset>
      {error && <p className="social-error" role="alert">{error}</p>}{saved && <p role="status">已保存公开资料。</p>}
    </form>
  </div>;
}

function AdministratorsPanel({ ai, changed }: { ai: ManagedAi; changed(): Promise<unknown> }) {
  const [view, setView] = useState<Administrators | null>(null);
  const [query, setQuery] = useState("");
  const [results, setResults] = useState<Account[]>([]);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const [removing, setRemoving] = useState<Account | null>(null);
  const sequence = useRef(0);
  const command = useRef({ signature: "", id: "" });
  const path = `/ai/${ai.account.id}/administrators`;
  const load = async () => {
    const ticket = ++sequence.current; setError(""); setView(null);
    try { const result = await request<Administrators>(path); if (ticket === sequence.current) setView(result); }
    catch (e) { if (ticket === sequence.current) setError(e instanceof Error ? e.message : String(e)); }
  };
  useEffect(() => { void load(); return () => { sequence.current++; }; }, [path, ai.administratorsRevision]);
  const update = async (account: Account, enabled: boolean) => {
    const body = { expectedRevision: view!.revision, enabled };
    const signature = JSON.stringify([account.id, body]);
    if (command.current.signature !== signature) command.current = { signature, id: crypto.randomUUID() };
    const ticket = ++sequence.current; setBusy(true); setError("");
    try {
      const result = await request<Administrators>(`${path}/${account.id}`, "PUT", { ...body, requestId: command.current.id });
      if (ticket === sequence.current) { setView(result); setRemoving(null); setResults([]); }
      command.current = { signature: "", id: "" }; await changed();
    } catch (e) { if (ticket === sequence.current) { setView(null); setError(e instanceof Error ? e.message : String(e)); } }
    finally { setBusy(false); }
  };
  return <div className="cloud-ai-section"><div className="cloud-ai-section-heading"><h2>管理员</h2><button className="text-button" disabled={busy} onClick={() => void load()}>刷新名单</button></div>
    <p>所有管理员都可以管理这个 AI 的资料、角色、模型和运行节点。只有初始管理员可以调整管理员名单。</p>
    {error && <p className="social-error" role="alert">{error}</p>}
    {view?.administrators.map(({ account, role }) => <div className="cloud-group-member" key={account.id}><span>{account.name}<small>{role === "initial" ? "初始管理员" : "管理员"} · {account.id}</small></span>{ai.role === "initial" && role !== "initial" && <button className="text-button" disabled={busy} onClick={() => setRemoving(account)}>移除管理权限</button>}</div>)}
    {removing && view && <div className="cloud-participation-confirm"><p>移除 {removing.name} 的管理权限后，对方仍可正常聊天。</p><div className="cloud-actions"><button className="social-button secondary" disabled={busy} onClick={() => void update(removing, false)}>确认移除</button><button className="text-button" disabled={busy} onClick={() => setRemoving(null)}>取消</button></div></div>}
    {ai.role === "initial" && view && <form className="cloud-form cloud-ai-form" onSubmit={async e => {
      e.preventDefault(); const ticket = ++sequence.current; setBusy(true); setError(""); setResults([]);
      try { const accounts = await request<Account[]>(`/chat/accounts?query=${encodeURIComponent(query.trim())}`); if (ticket === sequence.current) setResults(accounts.filter(a => a.kind === "human" && !view.administrators.some(admin => admin.account.id === a.id))); }
      catch (e) { if (ticket === sequence.current) setError(e instanceof Error ? e.message : String(e)); }
      finally { setBusy(false); }
    }}>
      <label>添加管理员<input value={query} onChange={e => { setQuery(e.target.value); setResults([]); }} placeholder="搜索真人名字或完整账号 ID" required disabled={busy} /></label>
      <button className="social-button secondary" disabled={busy}>查找账号</button>
      {results.map(account => <div className="cloud-group-member" key={account.id}><span>{account.name}<small>{account.id}</small></span><button className="social-button secondary" type="button" disabled={busy} onClick={() => void update(account, true)}>授予管理权限</button></div>)}
    </form>}
  </div>;
}

export function AiDetails({ ai, operatorId, conversations, cursor, back, changed }: { ai: ManagedAi; operatorId: string; conversations: Conversation[]; cursor: string; back(): void; changed(): Promise<unknown> }) {
  const [tab, setTab] = useState("profile");
  const [windowId, setWindowId] = useState("");
  const common = conversations.filter(c => c.members.some(m => m.account.id === ai.account.id));
  const conversation = common.find(c => c.id === windowId);
  const tabs = [["profile", "公开资料"], ["configuration", "角色与模型"], ["administrators", "管理员"], ["consciousness", "意识"], ["view", "只读查看"]];
  return <section className="contact-view cloud-ai-management">
    <header className="contact-header" data-tauri-drag-region={CUSTOM_WINDOW_CHROME ? "deep" : undefined}><IconButton label="返回我管理的 AI" onClick={back}><ArrowLeft /></IconButton><span>{ai.account.name}</span><span className="cloud-node-state">{ai.node?.online ? "节点在线" : "节点离线"}</span></header>
    <div className="contact-scroll"><div className="profile-content cloud-ai-detail">
      <nav className="cloud-ai-tabs" aria-label="AI 管理内容">{tabs.map(([id, name]) => <button key={id} className={tab === id ? "active" : ""} aria-current={tab === id ? "page" : undefined} onClick={() => setTab(id)}>{name}</button>)}</nav>
      {tab === "profile" && <PublicProfile ai={ai} changed={changed} />}
      {tab === "configuration" && <AiConfiguration ai={ai} operatorId={operatorId} />}
      {tab === "administrators" && <AdministratorsPanel ai={ai} changed={changed} />}
      {tab === "view" && <AiAccountView aiId={ai.account.id} operatorId={operatorId} />}
      {tab === "consciousness" && <div className="cloud-ai-section"><h2>意识</h2><p>选择你和这个 AI 共同参与的聊天，管理该窗口里的记忆与它自己的摘要。</p><label className="cloud-ai-window-select">聊天窗口<select value={windowId} onChange={e => setWindowId(e.target.value)}><option value="">选择一个窗口</option>{common.map(c => <option key={c.id} value={c.id}>{c.name}</option>)}</select></label>{common.length === 0 && <p>先与这个 AI 开始聊天，再管理你们之间的记忆。</p>}{conversation && <ConsciousnessPanel key={conversation.id} conversation={conversation} cursor={cursor} operatorId={operatorId} onlyAi={ai.account.id} summaryAiIds={[ai.account.id]} summaryError="" />}</div>}
    </div></div>
  </section>;
}
