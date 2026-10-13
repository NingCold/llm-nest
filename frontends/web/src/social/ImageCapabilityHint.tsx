import { imageSupportLabels } from "@/lib/image-input"
import type { Draft } from "./drafts"
import type { Conversation, Message, Snapshot } from "./live-types"

export function imageRecipients(snapshot: Snapshot, conversation: Conversation, draft?: Draft, reply?: Message) {
  const targets = new Set(draft?.mentions ?? [])
  if (reply && reply.status !== "withdrawn" && snapshot.accounts.some(a => a.id === reply.senderId && a.kind === "ai")) targets.add(reply.senderId)
  return snapshot.accounts.filter(a => a.kind === "ai" && conversation.memberIds.includes(a.id)
    && (conversation.kind === "direct" || !targets.size || targets.has(a.id)))
    .map(a => ({ id: a.id, name: a.name, support: snapshot.imageInputs[a.id] }))
}

export function ImageCapabilityHint({ snapshot, conversation, draft, reply }: { snapshot: Snapshot; conversation: Conversation; draft?: Draft; reply?: Message }) {
  if (!draft?.images?.length && !draft?.reference && !reply?.images.length) return null
  const recipients = imageRecipients(snapshot, conversation, draft, reply)
  if (!recipients.length) return null
  return <div className="image-capability-hint" role="status" aria-label="聊天图片能力">
    <div>{recipients.map(a => <span key={a.id}>{a.name} · {imageSupportLabels[a.support]}</span>)}</div>
    <p>{draft?.reference && "引用中如有图片，也受这些能力限制。"}图片会照常保存。{recipients.some(a => a.support === "unsupported") && "不支持图片的 AI 无法回应需要看图的内容。"}{recipients.some(a => a.support === "unknown") && "能力未知的 AI 可以尝试回复，尚未确认能否看图。"}{conversation.kind === "group" && "是否接话仍按本群规则。"}</p>
  </div>
}
