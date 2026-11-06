import { zipSync } from "fflate";
import { request, saveBlob } from "../api";
import { projectPage } from "./content";
import { attachmentBytes } from "./attachments";
import type { Conversation, Message, Page } from "../types";

// Pages arrive newest first; messages inside each page retain chronological order.
export async function* historyPages(conversation: Conversation, current: () => boolean = () => true): AsyncGenerator<Page> {
  let before: string | undefined;
  for (;;) {
    if (!current()) throw new Error("读取历史已取消");
    const raw = await request<Page>(`/chat/conversations/${conversation.id}/messages?limit=100${before ? `&before=${encodeURIComponent(before)}` : ""}`);
    if (!current()) throw new Error("读取历史已取消");
    const page = await projectPage(raw, conversation);
    if (!current()) throw new Error("读取历史已取消");
    yield page;
    if (!page.hasMore) return;
    const next = page.messages[0]?.position;
    if (!next || next === before) throw new Error("历史分页未前进，无法完成全文读取");
    before = next;
  }
}
export async function completeHistory(conversation: Conversation, current: () => boolean = () => true): Promise<Message[]> {
  const pages: Message[][] = [];
  for await (const page of historyPages(conversation, current)) pages.push(page.messages);
  return pages.reverse().flat();
}
export async function searchHistory(conversations: Conversation[], query: string, current: () => boolean): Promise<{ messages: Message[]; failed: number }> {
  const matches: Message[][] = new Array(conversations.length);
  const normalized = query.toLocaleLowerCase();
  let next = 0, failed = 0, stopped = false;
  const scanning = () => !stopped && current();
  const scan = async () => {
    while (next < conversations.length && scanning()) {
      const index = next++, pages: Message[][] = [];
      for await (const page of historyPages(conversations[index], scanning)) {
        const found: Message[] = [];
        for (const message of page.messages) {
          if (message.cryptoError) failed++;
          else if (message.status !== "withdrawn" && message.text.toLocaleLowerCase().includes(normalized)) found.push(message);
        }
        // Only matching messages survive beyond this page; keep stable tie ordering.
        if (found.length) pages.push(found);
      }
      matches[index] = pages.reverse().flat();
    }
  };
  try {
    await Promise.all(Array.from({ length: Math.min(3, conversations.length) }, scan));
  } finally { stopped = true; }
  if (!current()) throw new Error("读取历史已取消");
  return { messages: matches.flat().sort((a, b) => b.createdAt.localeCompare(a.createdAt)), failed };
}
function zip(files: { name: string; data: Uint8Array }[]) {
  // fflate writes ZIP32. Reject format overflow before its 16/32-bit fields truncate.
  if (files.length > 65535 || files.reduce((n, f) => n + f.data.length + 76 + 2 * new TextEncoder().encode(f.name).length, 22) > 0xffffffff) throw new Error("导出超过 ZIP32 格式限制，请缩小附件范围");
  return new Blob([new Uint8Array(zipSync(Object.fromEntries(files.map(f => [f.name, f.data])), { level: 0 }))], { type: "application/zip" });
}
export async function exportHistory(conversation: Conversation, format: string, images: boolean, files: boolean) {
  const messages = await completeHistory(conversation);
  const failed = messages.find(m => m.cryptoError);
  if (failed) throw new Error(`消息 ${failed.id} 无法解密，完整导出已停止：${failed.cryptoError}`);
  const entries: { name: string; data: Uint8Array }[] = [];
  const exported = [];
  for (const message of messages) {
    const attachments = [];
    for (const attachment of message.attachments) {
      const { mediaEncryptionInfo: _secret, ...metadata } = attachment;
      if (!(attachment.kind === "image" ? images : files)) { attachments.push(metadata); continue; }
      const name = `attachments/${message.id}-${attachment.index}-${(attachment.name || "attachment").replace(/[\\/:*?"<>|\x00-\x1f]/g, "_")}`;
      const data = await attachmentBytes(message, attachment, `/chat/messages/${message.id}/attachments/${attachment.index}?revision=${message.revision}`);
      entries.push({ name, data }); attachments.push({ ...metadata, archivePath: name });
    }
    const { sealed: _sealed, attachmentRoute: _attachmentRoute, ...plain } = message;
    exported.push({ ...plain, attachments });
  }
  const text = format === "json" ? JSON.stringify({ conversationId: conversation.id, name: conversation.name, messages: exported }, null, 2) : `# ${conversation.name}\n\n` + exported.map(m => `## ${m.senderName} · ${m.createdAt}\n\n${m.text}\n\n${m.attachments.map(a => "archivePath" in a ? `[${a.name || a.mime}](${a.archivePath})` : `[${a.kind}: ${a.name || a.mime} · ${a.byteLength} bytes]`).join("\n")}\n`).join("\n");
  const name = format === "json" ? "chat.json" : "chat.md", data = new TextEncoder().encode(text);
  if (images || files) await saveBlob(zip([{ name, data }, ...entries]), "chat.zip");
  else await saveBlob(new Blob([data], { type: format === "json" ? "application/json" : "text/markdown" }), name);
}
