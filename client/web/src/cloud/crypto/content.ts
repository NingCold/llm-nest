import { cryptoDevice, matrixRoom, matrixUser, seal, unseal, signEnvelope, recoveryConfirmed } from "./engine";
import type { SignedEnvelope } from "./envelope";
import { request, server } from "../api";
import type { Conversation, Message, Page, Snapshot } from "../types";
export type Sealed = SignedEnvelope;
export interface Content { version: 1; server: string; kind: "message" | "delta" | "resource"; conversationId: string | null; messageId: string; senderId: string; revision: string; text: string; replyTo: string | null; mentions: string[]; runId: string | null; sequence: number | null; scope?: string; id?: string; expectedRevision?: string }
export async function encryptMessage(conversation: Conversation, sender: string, messageId: string, text: string, revision = "1", replyTo: string | null = null, mentions: string[] = [], offline = false): Promise<Sealed> {
  const deviceId = cryptoDevice(sender);
  if (!deviceId || !recoveryConfirmed(sender)) throw new Error("请先解锁可信设备并确认恢复文件已保存");
  const roomId = matrixRoom(conversation.id), eventId = `$${messageId}.${revision}.final`;
  const inner: Content = { version: 1, server, kind: "message", conversationId: conversation.id, messageId, senderId: sender, revision, text, replyTo, mentions, runId: null, sequence: null };
  return signEnvelope({ version: 1, roomId, sender: matrixUser(sender), deviceId, eventId, content: await seal(roomId, conversation.members.map(m => m.account.id), "com.llmn.content.v1", inner, offline, conversation.members.map(m => m.account.id), true) });
}
export async function projectMessage(message: Message, conversation?: Pick<Conversation, "encryption">): Promise<Message> {
  if (!message.sealed) {
    if (conversation?.encryption && BigInt(message.position) > BigInt(conversation.encryption.activationPosition) && message.status !== "withdrawn") return { ...message, text: "", attachments: [], cryptoError: "此位置要求密文，但服务器未提供密文；已阻止明文回退" };
    return message;
  }
  const encrypted = message.sealed;
  try {
    if (encrypted.version !== 1 || encrypted.roomId !== matrixRoom(message.conversationId) || encrypted.sender !== matrixUser(message.senderId) || encrypted.deviceId !== encrypted.content.device_id) throw new Error("密文路由或发送设备身份不匹配");
    const event = await unseal(encrypted);
    const inner = event.content as unknown as Content;
    if (event.type !== "com.llmn.content.v1" || inner.version !== 1 || typeof inner.text !== "string" || inner.server !== server || inner.conversationId !== message.conversationId || inner.messageId !== message.id || inner.senderId !== message.senderId || inner.revision !== message.revision || (inner.kind !== "message" && inner.kind !== "delta") || inner.replyTo !== message.replyTo || JSON.stringify(inner.mentions) !== JSON.stringify(message.mentions) || message.attachments.length) throw new Error("消息完整性校验失败，不能显示被替换的内容");
    const expectedEvent = `$${message.id}.${inner.revision}.${inner.kind === "delta" ? inner.sequence : "final"}`;
    if (encrypted.eventId !== expectedEvent || (inner.kind === "delta" && (!inner.runId || !Number.isSafeInteger(inner.sequence)))) throw new Error("流式片段标识不匹配");
    return { ...message, text: inner.text, cryptoError: undefined };
  } catch (error) {
    return { ...message, text: "", attachments: [], cryptoError: error instanceof Error ? error.message : "密文暂不可解密" };
  }
}
export async function projectPage(page: Page, conversation?: Pick<Conversation, "encryption">): Promise<Page> { return { ...page, messages: await Promise.all(page.messages.map(m => projectMessage(m, conversation))) }; }
export async function projectConversations(conversations: Conversation[], account?: string) {
  return Promise.all(conversations.map(async original => {
    let c = original;
    if (account) {
      const key = `llmn-encrypted-window:${JSON.stringify([server, account, c.id])}`, saved = localStorage.getItem(key);
      if (saved) c = { ...c, encryption: JSON.parse(saved) };
      else if (c.encryption) localStorage.setItem(key, JSON.stringify(c.encryption));
    }
    return { ...c, lastMessage: c.lastMessage ? await projectMessage(c.lastMessage, c) : null };
  }));
}
export async function projectSnapshot(snapshot: Snapshot): Promise<Snapshot> { return { ...snapshot, conversations: await projectConversations(snapshot.conversations, snapshot.self.id) }; }
export async function enableEncryption(conversation: Conversation, account: string) {
  if (conversation.kind !== "direct") throw new Error("本批只开放私聊加密");
  const deviceId = cryptoDevice(account);
  if (!deviceId || !recoveryConfirmed(account)) throw new Error("请先解锁可信设备、核对双方指纹，并确认恢复文件已保存");
  // Preparing a room checks trusted recipients and distributes SDK session keys.
  await seal(matrixRoom(conversation.id), conversation.members.map(m => m.account.id), "com.llmn.content.v1", { version: 1, kind: "activation", conversationId: conversation.id });
  return request(`/chat/conversations/${conversation.id}/encryption`, "POST", { deviceId, requestId: crypto.randomUUID() });
}
