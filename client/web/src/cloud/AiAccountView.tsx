import { useEffect, useRef, useState } from "react";
import { Download, RefreshCw } from "lucide-react";
import { MarkdownView } from "@/components/chat/MarkdownView";
import { Dialog, DialogContent, DialogTitle } from "@/components/ui/dialog";
import { DownloadCancelled, download, request } from "./api";
import { fileSize } from "../social/files";
import type { Account, Conversation, Message, Page } from "./types";
import { projectPage, projectConversations } from "./crypto/content";

export function AiAccountView({ aiId, operatorId }: { aiId: string; operatorId?: string }) {
  const [view, setView] = useState<{ account: Account; conversations: Conversation[] } | null>(null);
  const [active, setActive] = useState("");
  const [page, setPage] = useState<Page | null>(null);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const [preview, setPreview] = useState<string>();
  const sequence = useRef(0);
  const root = `/ai/${aiId}/view`;
  useEffect(() => () => { if (preview) URL.revokeObjectURL(preview); }, [preview]);
  const load = async () => {
    const ticket = ++sequence.current;
    setBusy(true); setError(""); setView(null); setPage(null); setActive(""); setPreview(undefined);
    try { const result = await request<{ account: Account; conversations: Conversation[] }>(root); if (operatorId || result.conversations.some(c => c.encryption || c.lastMessage?.sealed)) result.conversations = await projectConversations(result.conversations, operatorId); if (ticket === sequence.current) setView(result); }
    catch (e) { if (ticket === sequence.current) setError(e instanceof Error ? e.message : String(e)); }
    finally { if (ticket === sequence.current) setBusy(false); }
  };
  useEffect(() => { void load(); return () => { sequence.current++; }; }, [root]);
  const history = async (conversationId: string, older = false) => {
    const ticket = ++sequence.current;
    const previous = older ? page : null;
    setBusy(true); setError(""); setActive(conversationId); setPreview(undefined);
    if (!older) setPage(null);
    try {
      const before = previous?.messages[0]?.position;
      const result = await projectPage(await request<Page>(`${root}/conversations/${conversationId}/messages${before ? `?before=${before}` : ""}`), view?.conversations.find(c => c.id === conversationId));
      if (ticket === sequence.current) setPage({ ...result, messages: [...result.messages, ...(previous?.messages ?? [])] });
    } catch (e) {
      if (ticket === sequence.current) { setView(null); setPage(null); setError(e instanceof Error ? e.message : String(e)); }
    } finally { if (ticket === sequence.current) setBusy(false); }
  };
  const attachment = async (message: Message, index: number, name: string, image: boolean) => {
    const ticket = sequence.current;
    setError("");
    try {
      const url = await download(`${root}/conversations/${message.conversationId}/messages/${message.id}/attachments/${index}?revision=${message.revision}${image ? "&inline=true" : ""}`, name, image);
      if (ticket === sequence.current) setPreview(url);
      else if (url) URL.revokeObjectURL(url);
    } catch (e) {
      if (!(e instanceof DownloadCancelled) && ticket === sequence.current) { setPage(null); setView(null); setPreview(undefined); setError(e instanceof Error ? e.message : String(e)); }
    }
  };
  return <div className="cloud-ai-section">
    <div className="cloud-ai-section-heading"><h2>只读查看账号</h2><button className="text-button" disabled={busy} onClick={() => void load()}><RefreshCw />刷新快照</button></div>
    <p>你仍使用自己的账号，以管理员身份查看 AI 当前可访问的聊天。这里没有代发入口，也不会改变它的已读状态。</p>
    {error && <p className="social-error" role="alert">{error}</p>}
    {view && <label className="cloud-ai-window-select">聊天窗口<select aria-label="选择只读聊天窗口" value={active} disabled={busy} onChange={e => void history(e.target.value)}>
      <option value="" disabled>选择一个窗口</option>
      {view.conversations.map(c => <option key={c.id} value={c.id}>{c.name}</option>)}
    </select></label>}
    {view?.conversations.length === 0 && <p>这个 AI 还没有聊天记录。</p>}
    {busy && <p role="status">正在读取…</p>}
    {page && <div className="cloud-ai-readonly-messages">
      <div className="cloud-actions"><button className="text-button" disabled={busy} onClick={() => void history(active)}>刷新当前窗口</button>{page.hasMore && <button className="text-button" disabled={busy} onClick={() => void history(active, true)}>读取更早消息</button>}</div>
      {page.messages.length === 0 && <p>还没有消息。</p>}
      {page.messages.map(message => <article className="cloud-ai-readonly-message" key={message.id}>
        <div className="live-message-label"><strong>{message.senderName}</strong><time>{new Date(message.createdAt).toLocaleString()}</time>{message.importOrigin && <small>导入历史</small>}</div>
        {message.status === "withdrawn" ? <p className="withdrawn-message">消息已撤回</p> : <>
          {message.replyTo && <small>回复消息 {message.replyTo}</small>}
          {message.cryptoError ? <p role="status">加密内容暂不可读：{message.cryptoError}</p> : <MarkdownView content={message.text} />}
          {message.status === "generating" && <p>正在回复，刷新后查看最新内容。</p>}
          {["failed", "interrupted"].includes(message.status) && <p>这条回复未完成。</p>}
          {message.attachments.map(a => <div className="cloud-actions" key={a.index}>
            {a.kind === "image" && <button className="text-button" onClick={() => void attachment(message, a.index, a.name || "图片", true)}>查看图片 · {fileSize(a.byteLength)}</button>}
            <button className="text-button" onClick={() => void attachment(message, a.index, a.name || `image-${message.id}-${a.index}`, false)}><Download />{a.kind === "image" ? "保存原图" : `${a.name || "附件"} · ${fileSize(a.byteLength)}`}</button>
          </div>)}
        </>}
      </article>)}
    </div>}
    <Dialog open={!!preview} onOpenChange={open => { if (!open) setPreview(undefined); }}><DialogContent className="social-dialog cloud-image-dialog" aria-describedby={undefined}><DialogTitle className="sr-only">聊天图片</DialogTitle>{preview && <img src={preview} alt="聊天图片" />}</DialogContent></Dialog>
  </div>;
}
