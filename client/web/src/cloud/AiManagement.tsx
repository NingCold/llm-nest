import { useEffect, useRef, useState } from "react";
import { ArrowLeft, MessageCircle, Plus, RefreshCw, Server, Unplug } from "lucide-react";
import { CUSTOM_WINDOW_CHROME } from "@/lib/desktop";
import { Dialog, DialogContent, DialogTitle } from "@/components/ui/dialog";
import { Avatar, IconButton } from "../social/components/common";
import { request } from "./api";
import type { Account } from "./types";

interface ManagedAi {
  account: Account;
  role: "initial" | "admin";
  bindingVersion: string;
  statusRevision: string;
  node: { id: string; name: string; online: boolean; lastSeenAt: string | null } | null;
}
interface Props {
  back(): void;
  openDirect(account: Account): Promise<unknown>;
  refresh(): Promise<unknown>;
}

export function AiManagement({ back, openDirect, refresh }: Props) {
  const [accounts, setAccounts] = useState<ManagedAi[] | null>(null);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const [dialog, setDialog] = useState<
    { kind: "create" } | { kind: "bind" | "revoke"; ai: ManagedAi } | null
  >(null);
  const command = useRef({ signature: "", id: "" });
  const alive = useRef(true);
  const loadVersion = useRef(0);
  const load = async () => {
    const version = ++loadVersion.current;
    const result = await request<ManagedAi[]>("/ai");
    if (alive.current && version === loadVersion.current) setAccounts(result);
  };
  useEffect(() => {
    alive.current = true;
    const update = () => void load().catch((e: Error) => {
      if (alive.current) setError(e.message);
    });
    update();
    const timer = setInterval(update, 10000);
    return () => { alive.current = false; clearInterval(timer); };
  }, []);

  const act = async (fn: () => Promise<unknown>) => {
    setBusy(true);
    setError("");
    try { await fn(); }
    catch (e) { if (alive.current) setError(e instanceof Error ? e.message : String(e)); }
    finally { if (alive.current) setBusy(false); }
  };
  const submit = async (path: string, body: object, method = "POST") => {
    const signature = JSON.stringify([path, method, body]);
    if (command.current.signature !== signature)
      command.current = { signature, id: crypto.randomUUID() };
    await request(path, method, { ...body, requestId: command.current.id });
    command.current = { signature: "", id: "" };
    if (alive.current) setDialog(null);
    await load();
    await refresh();
  };

  return (
    <section className="contact-view cloud-ai-management">
      <header className="contact-header" data-tauri-drag-region={CUSTOM_WINDOW_CHROME ? "deep" : undefined}>
        <IconButton label="返回我的账号" onClick={back}><ArrowLeft /></IconButton>
        <span>我管理的 AI</span>
        <IconButton label="刷新 AI 状态" disabled={busy} onClick={() => void act(load)}><RefreshCw /></IconButton>
      </header>
      <div className="contact-scroll">
        <div className="profile-content">
          <div className="cloud-ai-intro">
            <div><h1>我管理的 AI</h1><p>为 AI 建立自己的账号，连接它运行的设备。</p></div>
            <button className="social-button primary" disabled={busy} onClick={() => { setError(""); setDialog({ kind: "create" }); }}><Plus />创建 AI</button>
          </div>
          {error && !dialog && <p className="social-error" role="alert">{error}</p>}
          {accounts === null ? <p className="list-empty">正在读取…</p> : accounts.length === 0 ? (
            <div className="cloud-ai-empty"><Server /><h2>让一位 AI 加入这里</h2><p>创建后，你仍使用自己的账号聊天和管理。角色设定与模型密钥保存在运行 Agent 的设备上。</p></div>
          ) : (
            <div className="cloud-ai-list">
              {accounts.map((ai) => (
                <section className="cloud-ai-card" key={ai.account.id}>
                  <div className="cloud-ai-heading">
                    <Avatar contact={{ avatar: ai.account.name.slice(0, 1), color: "sage" }} />
                    <div><h2>{ai.account.name}{ai.account.status === "disabled" && " · 已停用"}</h2><p>{ai.role === "initial" ? "初始管理员" : "管理员"}</p></div>
                    <span className={`cloud-node-state ${ai.node?.online ? "online" : ""}`}><i />{!ai.node ? "尚未连接" : ai.node.online ? "在线" : "离线"}</span>
                  </div>
                  {ai.account.bio && <p className="cloud-ai-bio">{ai.account.bio}</p>}
                  <div className="cloud-node-description"><Server /><span>{ai.node?.name || "连接 Agent 后，AI 就能回复消息。"}</span></div>
                  <div className="cloud-actions">
                    <button className="social-button secondary" disabled={busy} onClick={() => void act(() => submit(`/ai/${ai.account.id}/status`, { expectedStatusRevision: ai.statusRevision, status: ai.account.status === "disabled" ? "active" : "disabled" }, "PUT"))}>{ai.account.status === "disabled" ? "恢复账号" : "停用账号"}</button>
                    <button className="social-button secondary" disabled={busy} onClick={() => void act(() => openDirect(ai.account))}><MessageCircle />发消息</button>
                    <button className="social-button secondary" disabled={busy} onClick={() => { setError(""); setDialog({ kind: "bind", ai }); }}>{ai.node ? "更换节点" : "连接节点"}</button>
                    {ai.node && <button className="text-button" disabled={busy} onClick={() => { setError(""); setDialog({ kind: "revoke", ai }); }}><Unplug />撤销连接</button>}
                  </div>
                </section>
              ))}
            </div>
          )}
        </div>
      </div>
      <Dialog open={dialog !== null} onOpenChange={(open) => { if (!open && !busy) { setDialog(null); setError(""); } }}>
        <DialogContent className="social-dialog cloud-ai-dialog" aria-describedby={undefined}>
          <DialogTitle>{dialog?.kind === "create" ? "创建 AI 账号" : dialog?.kind === "bind" ? `连接 ${dialog.ai.account.name}` : "撤销节点连接"}</DialogTitle>
          {dialog?.kind === "create" ? (
            <form className="cloud-form" onSubmit={(e) => {
              e.preventDefault();
              const data = new FormData(e.currentTarget);
              void act(() => submit("/ai", { name: String(data.get("name")).trim(), bio: String(data.get("bio")).trim() }));
            }}>
              <p>这里填写所有人都能看到的资料。创建后，你将成为它的初始管理员。</p>
              <label>名字<input name="name" required maxLength={80} disabled={busy} autoFocus /></label>
              <label>公开简介<textarea name="bio" maxLength={2000} disabled={busy} placeholder="简单介绍一下它" /></label>
              {error && <p className="social-error" role="alert">{error}</p>}
              <button className="social-button primary" disabled={busy}>{busy ? "正在创建…" : "创建账号"}</button>
            </form>
          ) : dialog?.kind === "bind" ? (
            <form className="cloud-form" onSubmit={(e) => {
              e.preventDefault();
              const code = String(new FormData(e.currentTarget).get("code")).trim();
              void act(() => submit(`/ai/${dialog.ai.account.id}/node`, { pairingCode: code, expectedBindingVersion: dialog.ai.bindingVersion }));
            }}>
              <p>在运行 Agent 的设备上发起连接，再把配对码填在这里。</p>
              {dialog.ai.node && <p>确认后，旧节点将停止为这个账号回复。更换节点不会自动迁移角色设定和记忆。</p>}
              <label>配对码<input name="code" required maxLength={64} disabled={busy} autoFocus autoComplete="off" spellCheck={false} /></label>
              {error && <p className="social-error" role="alert">{error}</p>}
              <button className="social-button primary" disabled={busy}>{busy ? "正在连接…" : "确认连接"}</button>
            </form>
          ) : dialog?.kind === "revoke" ? (
            <div className="cloud-form">
              <p>撤销后，{dialog.ai.account.name} 将停止通过这个节点回复。账号和聊天历史会保留。</p>
              {error && <p className="social-error" role="alert">{error}</p>}
              <button className="social-button secondary" disabled={busy} onClick={() => void act(() => submit(`/ai/${dialog.ai.account.id}/node/revoke`, { expectedBindingVersion: dialog.ai.bindingVersion }))}>{busy ? "正在撤销…" : "确认撤销"}</button>
            </div>
          ) : null}
        </DialogContent>
      </Dialog>
    </section>
  );
}
