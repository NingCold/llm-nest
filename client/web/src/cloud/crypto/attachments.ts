import { download, downloadBytes, saveBlob } from "../api";
import { decryptAttachment, encryptAttachment } from "./engine";
import type { Attachment, Message } from "../types";
import type { DraftImage } from "../../social/images";
import type { DraftFile } from "../../social/files";
import { validateFiles } from "../../social/files";
import { invoke } from "@tauri-apps/api/core";

export function prepareAttachments(images: DraftImage[] = [], files: DraftFile[] = []) {
  validateFiles(files);
  const descriptors: Attachment[] = [], encryptedAttachments: { index: number; data: string }[] = [];
  for (const [index, file] of [...images.map(i => ({ ...i, kind: "image" })), ...files.map(f => ({ ...f, kind: "file" }))].entries()) {
    const encrypted = encryptAttachment(Uint8Array.from(atob(file.data), c => c.charCodeAt(0)));
    descriptors.push({ index, kind: file.kind, name: file.name, mime: file.mime, byteLength: file.byteLength, mediaEncryptionInfo: encrypted.mediaEncryptionInfo });
    encryptedAttachments.push({ index, data: encrypted.data });
  }
  return { descriptors, encryptedAttachments };
}
export async function attachmentBytes(message: Message, attachment: Attachment, path: string): Promise<Uint8Array> {
  const data = message.attachmentRoute === "local" || message.local ? Uint8Array.from(atob(await invoke<string>("local_chat_attachment", { conversationId: message.conversationId, messageId: message.id, index: attachment.index, revision: message.revision })), c => c.charCodeAt(0)) : await downloadBytes(message.sealed ? path.replace(/&inline=true$/, "") : path);
  if (!message.sealed) return data;
  if (!attachment.mediaEncryptionInfo || message.cryptoError) throw new Error("附件缺少已验证的解密信息");
  const bytes = decryptAttachment(data, attachment.mediaEncryptionInfo);
  if (bytes.length !== attachment.byteLength) throw new Error("附件长度与签名内容不匹配");
  if (attachment.kind === "file") validateFiles([{ byteLength: bytes.length }]);
  else {
    if (bytes.length > 4 * 1024 * 1024 || !["image/png", "image/jpeg", "image/webp"].includes(attachment.mime)) throw new Error("图片格式或大小超过限制");
    const png = bytes[0] === 137 && bytes[1] === 80 && bytes[2] === 78 && bytes[3] === 71;
    const jpeg = bytes[0] === 255 && bytes[1] === 216 && bytes[2] === 255;
    const webp = new TextDecoder().decode(bytes.slice(0, 4)) === "RIFF" && new TextDecoder().decode(bytes.slice(8, 12)) === "WEBP";
    if (!(attachment.mime === "image/png" ? png : attachment.mime === "image/jpeg" ? jpeg : webp)) throw new Error("图片实际格式与签名类型不匹配");
    const image = await createImageBitmap(new Blob([new Uint8Array(bytes)], { type: attachment.mime }));
    try { if (image.width > 4096 || image.height > 4096) throw new Error("图片边长超过 4096"); } finally { image.close(); }
  }
  return bytes;
}
export async function downloadAttachment(message: Message, attachment: Attachment, path: string, name: string, preview = false) {
  if (!message.sealed && message.attachmentRoute !== "local" && !message.local) return download(path, name, preview);
  const bytes = await attachmentBytes(message, attachment, path);
  return saveBlob(new Blob([new Uint8Array(bytes)], { type: attachment.mime }), name, preview);
}
