import { useRef, useState } from "react";
import { Dialog, DialogContent, DialogTitle } from "@/components/ui/dialog";
import { request } from "./api";
import type { Account, Conversation } from "./types";

type Act = (fn: () => Promise<unknown>) => Promise<void>;
// Keep the command identity when a response is lost and the same form is retried.
function useCommand() {
  const last = useRef({ signature: "", id: "" });
  return async <T,>(path: string, method: string, body: object) => {
    const signature = JSON.stringify([path, method, body]);
    if (last.current.signature !== signature) last.current = { signature, id: crypto.randomUUID() };
    const result = await request<T>(path, method, { ...body, requestId: last.current.id });
    last.current = { signature: "", id: "" };
    return result;
  };
}

export function CreateGroup({ contacts, me, act, busy, error, refresh, select, close }: {
  contacts: Account[]; me: string; act: Act; busy: boolean;
  error: string;
  refresh(): Promise<unknown>; select(id: string): void; close(): void;
}) {
  const [members, setMembers] = useState<string[]>([]);
  const [name, setName] = useState("");
  const [query, setQuery] = useState("");
  const [found, setFound] = useState<Account[]>([]);
  const [selected, setSelected] = useState<Account[]>([]);
  const command = useCommand();
  const choices = [...new Map([...contacts, ...found, ...selected].filter(a => a.id !== me).map(a => [a.id, a])).values()];
  return <Dialog open onOpenChange={open => { if (!open && !busy) close(); }}>
    <DialogContent className="social-dialog" aria-describedby={undefined}>
      <DialogTitle>创建群聊</DialogTitle>
      <form className="cloud-form" onSubmit={e => { e.preventDefault(); void act(async () => {
        const group = await command<Conversation>("/chat/groups", "POST", { name: name.trim(), memberIds: members });
        select(group.id); close(); await refresh();
      }); }}>
        <label>群名称<input required maxLength={80} value={name} onChange={e => setName(e.target.value)} /></label>
        <label>查找其他账号<input value={query} onChange={e => setQuery(e.target.value)} placeholder="名字或完整账号 ID" /></label>
        <button type="button" className="social-button secondary" disabled={busy || !query.trim()} onClick={() => void act(async () => setFound(await request<Account[]>(`/chat/accounts?query=${encodeURIComponent(query)}`)))}>查找账号</button>
        <div className="cloud-group-choices">{choices.map(a => <label className="cloud-check" key={a.id}>
          <input type="checkbox" checked={members.includes(a.id)} disabled={busy || (!members.includes(a.id) && members.length >= 99)} onChange={e => {
            setMembers(e.target.checked ? [...members, a.id] : members.filter(id => id !== a.id));
            if (e.target.checked) setSelected(previous => [...previous.filter(x => x.id !== a.id), a]);
          }} />{a.name} · {a.kind === "ai" ? "AI" : "真人"}
        </label>)}</div>
        <p>已选择 {members.length} 位成员。群成员可阅读完整群历史；AI 默认在明确提及或回复它时发言。</p>
        {error && <p className="social-error" role="alert">{error}</p>}
        <button className="social-button primary" disabled={busy || !members.length || !name.trim()}>创建群聊</button>
      </form>
    </DialogContent>
  </Dialog>;
}

export function GroupDetails({ conversation: c, me, act, busy, refresh, leave }: {
  conversation: Conversation; me: string; act: Act; busy: boolean;
  refresh(): Promise<unknown>; leave(): void;
}) {
  const role = c.members.find(m => m.account.id === me)!.role;
  const owner = role === "owner", manager = owner || role === "admin";
  const [name, setName] = useState(c.name);
  const [accountId, setAccountId] = useState("");
  const [query, setQuery] = useState("");
  const [accounts, setAccounts] = useState<Account[]>([]);
  const command = useCommand();
  const update = (suffix: string, method: string, body: object) => void act(async () => {
    await command(`/chat/groups/${c.id}${suffix}`, method, { expectedRevision: c.revision, ...body });
    await refresh();
  });
  return <section className="cloud-detail-section cloud-form">
    <h3>群成员 · {c.members.length}</h3>
    {c.members.map(m => <div className="cloud-group-member" key={m.account.id}>
      <span>{m.account.name}<small>{m.account.kind === "ai" ? "AI · " : ""}{({ owner: "群主", admin: "管理员", member: "成员" })[m.role]}</small></span>
      {owner && m.account.id !== me && <select aria-label={`${m.account.name} 的群角色`} value={m.role} disabled={busy} onChange={e => update(`/members/${m.account.id}`, "PUT", { role: e.target.value })}><option value="member">成员</option><option value="admin">管理员</option></select>}
      {manager && m.account.id !== me && m.role !== "owner" && (owner || m.role === "member") && <button className="text-button" disabled={busy} onClick={() => update(`/members/${m.account.id}`, "DELETE", {})}>移出</button>}
      {owner && m.account.id !== me && m.account.kind === "human" && <button className="text-button" disabled={busy} onClick={() => update("/owner", "POST", { accountId: m.account.id })}>转让群主</button>}
    </div>)}
    {manager && <>
      <form className="cloud-form" onSubmit={e => { e.preventDefault(); update("", "PUT", { name: name.trim() }); }}><label>群名称<input required maxLength={80} value={name} onChange={e => setName(e.target.value)} /></label><button disabled={busy || !name.trim()} className="social-button secondary">保存群名称</button></form>
      <label>查找新成员<input value={query} onChange={e => setQuery(e.target.value)} /></label>
      <button className="social-button secondary" disabled={busy || !query.trim()} onClick={() => void act(async () => setAccounts(await request<Account[]>(`/chat/accounts?query=${encodeURIComponent(query)}`)))}>查找账号</button>
      <select aria-label="选择新成员" value={accountId} onChange={e => setAccountId(e.target.value)}><option value="">选择账号</option>{accounts.filter(a => !c.members.some(m => m.account.id === a.id)).map(a => <option key={a.id} value={a.id}>{a.name} · {a.kind === "ai" ? "AI" : "真人"}</option>)}</select>
      <button className="social-button secondary" disabled={busy || !accountId || c.members.length >= 100} onClick={() => update("/members", "POST", { accountId })}>添加成员</button>
    </>}
    {owner ? <p>退出前请先将群主转让给另一位真人成员。</p> : <button className="social-button secondary" disabled={busy} onClick={() => void act(async () => { await command(`/chat/groups/${c.id}/leave`, "POST", { expectedRevision: c.revision }); leave(); await refresh(); })}>退出群聊</button>}
  </section>;
}

export function ContactBlock({ account, act, refresh, busy }: { account: Account; act: Act; refresh(): Promise<unknown>; busy: boolean }) {
  const [entry, setEntry] = useState<{ blocked: boolean; revision: string } | null>(null);
  const command = useCommand();
  // Read when explicitly opening the controls, so a stale profile never supplies a revision.
  return <section className="cloud-form"><h3>私聊屏蔽</h3><p>屏蔽后双方不能发送新的私聊消息，共同群聊仍可正常使用。</p>
    {entry ? <button className="social-button secondary" disabled={busy} onClick={() => void act(async () => {
      setEntry(await command(`/chat/contacts/${account.id}/block`, "PUT", { expectedRevision: entry.revision, blocked: !entry.blocked })); await refresh();
    })}>{entry.blocked ? "解除屏蔽" : "屏蔽私聊"}</button> : <button className="social-button secondary" disabled={busy} onClick={() => void act(async () => setEntry(await request(`/chat/contacts/${account.id}`)))}>查看屏蔽状态</button>}
  </section>;
}
