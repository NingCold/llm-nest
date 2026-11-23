import { request, saveBlob } from "../api";
import type { Page, Snapshot } from "../types";
import { projectMessage } from "./content";
import { recoveryReader, type RecoveryFile } from "./engine";

// The archive uses only the selected file and its backup generation, never local drafts or current room keys.
export async function exportRecoveryHistory(file: RecoveryFile) {
  const reader = await recoveryReader(file, true);
  try {
    const snapshot = await request<Snapshot>("/chat/init");
    reader.check();
    const conversations = [];
    for (const conversation of snapshot.conversations) {
      const pages = [];
      let before: string | undefined;
      for (;;) {
        reader.check();
        const page: Page = await request(`/chat/conversations/${encodeURIComponent(conversation.id)}/messages?limit=100${before ? "&before="+encodeURIComponent(before) : ""}`);
        const entries = [];
        for (const row of page.messages) {
          reader.check();
          if (row.conversationId !== conversation.id) throw new Error("恢复历史窗口不匹配");
          if (!row.sealed || row.status === "withdrawn") continue;
          const message = await projectMessage(row, conversation, reader.unseal);
          entries.push(message.cryptoError
            ? { id: row.id, error: "此文件无法验证或解密该记录" }
            : { id: message.id, sender: message.senderId, text: message.text });
        }
        pages.push(entries);
        if (!page.hasMore) break;
        const cursor = page.messages[0]?.position;
        if (!cursor || (before && BigInt(cursor) >= BigInt(before))) throw new Error("历史分页未前进");
        before = cursor;
      }
      const messages = pages.reverse().flat();
      if (messages.length) conversations.push({ conversationId: conversation.id, messages });
    }
    reader.check();
    const archive = {format:"llmn-verified-history-text-v1",accountId:file.accountId,recoveryVersion:file.version,conversations};
    await saveBlob(new Blob([JSON.stringify(archive,null,2)],{type:"application/json"}),`llm-nest-history-${file.version}.json`);
  } finally { reader.close(); }
}
