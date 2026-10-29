import { useEffect, useRef, useState } from "react";
import { Dialog, DialogContent, DialogTitle } from "@/components/ui/dialog";
import { projectReference } from "./crypto/content";
import { request } from "./api";
import type { Account, Conversation, DraftReference, Message, ReferencePreview, ReferenceSelection } from "./types";

export function ReferenceCard({ directId, selection, cursor, stale = false, remove }: { directId: string; selection: ReferenceSelection; cursor: string; stale?: boolean; remove?: () => void }) {
  const [open, setOpen] = useState(false);
  const [retry, setRetry] = useState(0);
  const [result, setResult] = useState<{ stamp: string; data?: ReferencePreview; error?: string } | null>(null);
  const stamp = JSON.stringify([directId, selection, cursor, stale]);
  useEffect(() => {
    let active = true;
    if (open && !stale) void request<ReferencePreview>(`/chat/conversations/${directId}/reference-preview`, "POST", { reference: selection }).then(projectReference).then(
      data => { if (active) setResult({ stamp, data }); },
      error => { if (active) setResult({ stamp, error: error instanceof Error ? error.message : String(error) }); },
    );
    return () => { active = false; };
  }, [stamp, open, retry]);
  const visible = result?.stamp === stamp ? result : null;
  return <div className={`live-reference ${stale ? "stale" : ""}`}><button className="text-button" onClick={() => setOpen(true)}>{stale ? "群话题引用已失效" : `群话题引用 · ${selection.sources.length} 条原文`}</button>{remove && <button className="text-button" onClick={remove}>移除引用</button>}<Dialog open={open} onOpenChange={setOpen}><DialogContent className="social-dialog cloud-reference-dialog" aria-describedby={undefined}><DialogTitle>群话题来源</DialogTitle>{stale ? <p role="status">来源或成员权限已变化，请回到群聊重新选择。重新入群不会恢复旧引用。</p> : visible?.error ? <p className="social-error" role="alert">{visible.error}<button className="text-button" onClick={() => setRetry(n => n + 1)}>重新核对来源</button></p> : visible?.data ? <div className="reference-preview"><h3>{visible.data.conversation.name}</h3>{visible.data.messages.map(message => <blockquote key={message.id}><small>{message.senderName} · {new Date(message.createdAt).toLocaleString()}</small><p>{message.text}</p>{message.attachments.map(attachment => <p key={attachment.index}>[{attachment.kind === "image" ? "图片" : "文件"}：{attachment.name || attachment.mime} · {attachment.byteLength} 字节]</p>)}</blockquote>)}</div> : <p role="status">正在核对当前成员和来源版本…</p>}</DialogContent></Dialog></div>;
}

export function ContinuePrivately({ conversation, message, messages, continueWith }: { conversation: Conversation; message: Message; messages: Message[]; continueWith(ai: Account, reference: DraftReference): Promise<void> }) {
  const [open, setOpen] = useState(false);
  const [selected, setSelected] = useState<string[]>([message.id]);
  const [aiId, setAiId] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const version = useRef(0);
  const [preview, setPreview] = useState<{ stamp: string; data: ReferencePreview } | null>(null);
  useEffect(() => () => { version.current++; }, [conversation.id]);
  const ais = conversation.members.filter(m => m.account.kind === "ai" && m.account.status === "active").map(m => m.account);
  const ai = ais.find(a => a.id === aiId) || ais.find(a => a.id === message.senderId) || ais[0];
  const choices = messages.filter(m => m.status === "committed");
  const sources = selected.map(id => choices.find(m => m.id === id)).filter((m): m is Message => !!m).map(m => ({ messageId: m.id, revision: m.revision }));
  const valid = ai && selected.length > 0 && selected.length <= 8 && sources.length === selected.length;
  const stamp = JSON.stringify([conversation.id, conversation.revision, ai?.id, sources]);
  const currentPreview = preview?.stamp === stamp ? preview.data : null;
  return <><button className="text-button" onClick={() => { setSelected([message.id]); setError(""); setPreview(null); setOpen(true); }}>私聊继续</button><Dialog open={open} onOpenChange={value => { if (!busy) setOpen(value); }}><DialogContent className="social-dialog cloud-reference-dialog" aria-describedby={undefined}><DialogTitle>与 AI 私聊继续</DialogTitle><p>选择 1–8 条共同群原文，核对后放入已有的唯一私聊草稿。保留私聊文字和附件，不会自动发送；不携带群共享摘要。</p><div className="cloud-form"><label>目标 AI<select value={ai?.id || ""} disabled={busy} onChange={e => setAiId(e.target.value)}>{ais.map(a => <option key={a.id} value={a.id}>{a.name}</option>)}</select></label><div className="reference-choices">{choices.map(m => <label key={m.id}><input type="checkbox" checked={selected.includes(m.id)} disabled={busy || (!selected.includes(m.id) && selected.length >= 8)} onChange={e => setSelected(ids => e.target.checked ? [...ids, m.id] : ids.filter(id => id !== m.id))} /><span><small>{m.senderName}</small><span>{m.text || "[附件消息]"}</span></span></label>)}</div>{!sources.length && <p>请选择至少一条有效原文。</p>}{!ai && <p>当前群没有可接续话题的 AI。</p>}{error && <p className="social-error" role="alert">{error}</p>}
    <button className="social-button secondary" disabled={busy || !valid} onClick={async () => {
      const ticket = ++version.current; setBusy(true); setError(""); setPreview(null);
      try {
        const direct = await request<Conversation>("/chat/direct", "POST", { accountId: ai.id });
        const data = await projectReference(await request<ReferencePreview>(`/chat/conversations/${direct.id}/reference-preview`, "POST", { reference: { conversationId: conversation.id, sources } }));
        if (ticket === version.current) setPreview({ stamp, data });
      } catch (e) { if (ticket === version.current) setError(e instanceof Error ? e.message : String(e)); }
      finally { setBusy(false); }
    }}>预览所选来源</button>
    {currentPreview && <div className="reference-preview"><h3>{currentPreview.conversation.name}</h3>{currentPreview.messages.map(source => <blockquote key={source.id}><small>{source.senderName} · 版本 {source.revision}</small><p>{source.text || "[附件消息]"}</p></blockquote>)}</div>}
    <button className="social-button primary" disabled={busy || !valid || !currentPreview} onClick={async () => {
    const ticket = ++version.current; setBusy(true); setError("");
    try { await continueWith(ai, { actorId: ai.id, selection: { conversationId: conversation.id, sources } }); if (ticket === version.current) setOpen(false); }
    catch (e) { if (ticket === version.current) setError(e instanceof Error ? e.message : String(e)); }
    finally { setBusy(false); }
  }}>{busy ? "核对来源并保存草稿…" : "确认预览，带入私聊草稿"}</button><button className="social-button secondary" disabled={busy} onClick={() => setOpen(false)}>取消</button></div></DialogContent></Dialog></>;
}
