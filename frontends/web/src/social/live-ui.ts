import type { Account, Conversation } from "./live-types"

export function contactAvatar(contact?: Account) {
  return contact ? { avatar: contact.name.slice(0, 1), color: (["sage", "clay", "blue", "lavender"] as const)[parseInt(contact.id[0], 16) % 4] } : undefined
}

export function conversationPreview(conversation: Conversation) {
  const message = conversation.latestMessage
  if (message?.text) return `${message.images.length ? "[图片] " : ""}${message.files.length ? "[文件] " : ""}${message.text}`
  if (message?.images?.length) return "[图片]"
  if (message?.files.length) return "[文件]"
  if (conversation.activeRun) return "正在回复…"
  if (!message) return "开始你们的第一段对话"
  if (message.status === "interrupted") return "回复已中断"
  if (message.status === "withdrawn") return "消息已撤回"
  return "这条消息没有正文"
}
