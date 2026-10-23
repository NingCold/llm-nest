import { cryptoDevice, matrixRoom, matrixUser, seal, unseal, signEnvelope, recoveryConfirmed } from "./engine";
import type { SignedEnvelope } from "./envelope";
import { request, server } from "../api";
import type { Conversation, Message, Page, Snapshot, Attachment, ReferenceSelection, ReferencePreview, ContactIntent } from "../types";
export type Sealed = SignedEnvelope;
export interface Content { version: 1; server: string; kind: "message" | "delta" | "resource" | "contactIntent"; conversationId: string | null; messageId: string; senderId: string; revision: string; text: string; replyTo: string | null; mentions: string[]; runId: string | null; sequence: number | null; conversationRevision?: string; attachments?: Attachment[]; reference?: ReferenceSelection | null; aiId?: string; dueAt?: string; expiresAt?: string; utcOffsetMinutes?: number; scope?: string; id?: string; expectedRevision?: string }
export async function encryptMessage(conversation: Conversation, sender: string, messageId: string, text: string, revision = "1", replyTo: string | null = null, mentions: string[] = [], offline = false, attachments: Attachment[] = [], reference: ReferenceSelection | null = null): Promise<Sealed> {
  const deviceId = cryptoDevice(sender);
  if (!deviceId || !recoveryConfirmed(sender)) throw new Error("请先解锁可信设备并确认恢复文件已保存");
  const roomId = matrixRoom(conversation.id), eventId = `$${messageId}.${revision}.final`;
  const inner: Content = { version: 1, server, kind: "message", conversationId: conversation.id, messageId, senderId: sender, revision, text, replyTo, mentions, runId: null, sequence: null, conversationRevision: conversation.revision, attachments, reference };
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
    if (event.type !== "com.llmn.content.v1" || inner.version !== 1 || typeof inner.text !== "string" || inner.server !== server || inner.conversationId !== message.conversationId || inner.messageId !== message.id || inner.senderId !== message.senderId || inner.revision !== message.revision || (inner.kind !== "message" && inner.kind !== "delta") || inner.replyTo !== message.replyTo || JSON.stringify(inner.mentions) !== JSON.stringify(message.mentions)) throw new Error("消息完整性校验失败，不能显示被替换的内容");
    if ((message.audienceRevision != null && inner.conversationRevision !== message.audienceRevision) || (conversation && "kind" in conversation && conversation.kind === "group" && !inner.conversationRevision)) throw new Error("消息成员版本不匹配");
    const reference = message.contextReference ? { conversationId: message.contextReference.conversationId, sources: message.contextReference.sources } : null;
    if (JSON.stringify(inner.reference ?? null) !== JSON.stringify(reference)) throw new Error("引用完整性校验失败");
    const attachments = inner.attachments ?? [];
    if (attachments.length !== message.attachments.length || attachments.some((a, index) => a.index !== index || typeof a.mediaEncryptionInfo !== "string" || !a.mediaEncryptionInfo || typeof a.mime !== "string" || (a.name != null && typeof a.name !== "string") || !["image", "file"].includes(a.kind) || !Number.isSafeInteger(a.byteLength) || a.byteLength < 0 || !message.attachments.some(outer => outer.index === a.index && (outer.kind === "encrypted" || outer.mediaEncryptionInfo === a.mediaEncryptionInfo)))) throw new Error("附件完整性校验失败");
    const images = attachments.filter(a => a.kind === "image"), files = attachments.filter(a => a.kind === "file");
    if (images.length > 4 || files.length > 4 || images.some(a => a.byteLength === 0 || a.byteLength > 4 * 1024 * 1024 || !["image/png", "image/jpeg", "image/webp"].includes(a.mime)) || images.reduce((n, a) => n + a.byteLength, 0) > 8 * 1024 * 1024 || files.some(a => a.byteLength > 10 * 1024 * 1024) || files.reduce((n, a) => n + a.byteLength, 0) > 20 * 1024 * 1024) throw new Error("附件超出格式或大小限制");
    if (files.some(a => !a.mime || new TextEncoder().encode(a.mime).length > 255 || /\p{Cc}/u.test(a.mime) || (a.name != null && (!a.name.trim() || new TextEncoder().encode(a.name).length > 240 || /[\\/:*?"<>|\p{Cc}]/u.test(a.name) || /[ .]$/.test(a.name) || /^(CON|PRN|AUX|NUL|COM[1-9]|LPT[1-9])(?:\.|$)/i.test(a.name))))) throw new Error("文件名或类型无效");
    const expectedEvent = `$${message.id}.${inner.revision}.${inner.kind === "delta" ? inner.sequence : "final"}`;
    if (encrypted.eventId !== expectedEvent || (inner.kind === "delta" && (!inner.runId || !Number.isSafeInteger(inner.sequence)))) throw new Error("流式片段标识不匹配");
    return { ...message, text: inner.text, attachments, cryptoError: undefined };
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
      if (saved && !c.encryption) c = { ...c, encryption: JSON.parse(saved) };
      else if (c.encryption) localStorage.setItem(key, JSON.stringify(c.encryption));
    }
    return { ...c, lastMessage: c.lastMessage ? await projectMessage(c.lastMessage, c) : null };
  }));
}
export async function projectSnapshot(snapshot: Snapshot): Promise<Snapshot> { return { ...snapshot, conversations: await projectConversations(snapshot.conversations, snapshot.self.id) }; }
export async function enableEncryption(conversation: Conversation, account: string) {
  const deviceId = cryptoDevice(account);
  if (!deviceId || !recoveryConfirmed(account)) throw new Error("请先解锁可信设备、核对双方指纹，并确认恢复文件已保存");
  // Preparing a room checks trusted recipients and distributes SDK session keys.
  await seal(matrixRoom(conversation.id), conversation.members.map(m => m.account.id), "com.llmn.content.v1", { version: 1, kind: "activation", conversationId: conversation.id });
  return request(`/chat/conversations/${conversation.id}/encryption`, "POST", { deviceId, requestId: crypto.randomUUID(), expectedConversationRevision: conversation.revision });
}

export async function projectReference(preview: ReferencePreview): Promise<ReferencePreview> {
  const messages = await Promise.all(preview.messages.map(async m => {
    const result = await projectMessage({ ...m, conversationId: preview.conversation.id, status: "committed", aiResponses: [], contextReference: m.contextReference ?? null }, preview.conversation);
    if (result.cryptoError) throw new Error(result.cryptoError);
    return result;
  }));
  return { ...preview, messages };
}
export async function encryptIntent(conversation: Conversation, sender: string, intentId: string, input: { aiId: string; content: string; dueAt: string; expiresAt: string; utcOffsetMinutes: number }) {
  const deviceId = cryptoDevice(sender);
  if (!deviceId || !recoveryConfirmed(sender)) throw new Error("请先解锁可信设备并保存恢复文件");
  const roomId = matrixRoom(conversation.id);
  const inner: Content = { version: 1, server, kind: "contactIntent", conversationId: conversation.id, conversationRevision: conversation.revision, messageId: intentId, senderId: sender, revision: "1", text: input.content, aiId: input.aiId, dueAt: input.dueAt, expiresAt: input.expiresAt, utcOffsetMinutes: input.utcOffsetMinutes, replyTo: null, mentions: [], runId: null, sequence: null };
  return signEnvelope({ version: 1, roomId, sender: matrixUser(sender), deviceId, eventId: `$${intentId}.1.final`, content: await seal(roomId, conversation.members.map(m => m.account.id), "com.llmn.content.v1", inner, false, conversation.members.map(m => m.account.id), true) });
}
export async function projectIntent(intent: ContactIntent, conversation: Conversation): Promise<ContactIntent> {
  if (!intent.sealed) return conversation.encryption && (intent.audienceRevision != null || ["pending", "claimed", "executing"].includes(intent.status)) ? { ...intent, content: "", cryptoError: "约定缺少密文" } : intent;
  try {
    const sealed = intent.sealed;
    if (sealed.roomId !== matrixRoom(conversation.id) || sealed.sender !== matrixUser(intent.creatorId) || sealed.eventId !== `$${intent.id}.1.final`) throw new Error("约定密文身份不匹配");
    const event = await unseal(sealed), inner = event.content as unknown as Content;
    if (event.type !== "com.llmn.content.v1" || inner.version !== 1 || inner.kind !== "contactIntent" || inner.server !== server || inner.conversationId !== conversation.id || inner.messageId !== intent.id || inner.senderId !== intent.creatorId || inner.revision !== "1" || inner.aiId !== intent.aiId || new Date(inner.dueAt!).getTime() !== new Date(intent.dueAt).getTime() || new Date(inner.expiresAt!).getTime() !== new Date(intent.expiresAt).getTime() || inner.utcOffsetMinutes !== intent.utcOffsetMinutes || inner.conversationRevision !== intent.audienceRevision || typeof inner.text !== "string") throw new Error("约定完整性校验失败");
    return { ...intent, content: inner.text };
  } catch (error) { return { ...intent, content: "", cryptoError: error instanceof Error ? error.message : String(error) }; }
}
