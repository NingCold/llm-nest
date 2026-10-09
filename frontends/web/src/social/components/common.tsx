import type { ButtonHTMLAttributes, ReactNode } from "react"
import { MessageSquare, Users } from "lucide-react"
import type { AiContact } from "../types"

export function Avatar({ contact, group = false, self = false, size = "normal" }: { contact?: Pick<AiContact,"avatar"|"color">; group?: boolean; self?: boolean; size?: "small" | "normal" | "large" }) {
  return <span aria-hidden="true" className={`social-avatar avatar-${group ? "group" : self ? "self" : contact?.color ?? "sage"} avatar-${size}`}>
    {group ? <Users /> : self ? "我" : contact?.avatar ?? <MessageSquare />}
  </span>
}

export function IconButton({ label, children, className = "", ...props }: ButtonHTMLAttributes<HTMLButtonElement> & { label: string; children: ReactNode }) {
  return <button type="button" className={`social-icon-button ${className}`} aria-label={label} title={label} {...props}>{children}</button>
}

export function EmptyState({ title, description, children }: { title: string; description: string; children?: ReactNode }) {
  return <div className="social-empty"><span className="empty-illustration"><MessageSquare /></span><h2>{title}</h2><p>{description}</p>{children}</div>
}

export function shortTime(value: string) {
  const date = new Date(value)
  return date.toDateString() === new Date().toDateString()
    ? date.toLocaleTimeString("zh-CN", { hour: "2-digit", minute: "2-digit", hour12: false })
    : date.toLocaleDateString("zh-CN", { month: "numeric", day: "numeric" })
}
