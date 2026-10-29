import type { ReactNode } from "react"
import { LockKeyhole, MessageCircle, Moon, Settings, Sun, Users } from "lucide-react"
import { BrandMark } from "@/components/BrandMark"
import { CUSTOM_WINDOW_CHROME } from "@/lib/desktop"
import { useUiStore } from "@/store/ui"
import { Avatar, IconButton } from "./common"
import type { AvatarStyle } from "../live-types"

export function SocialSidebar({ page, navigate, onAccount, onSettings, onLock, self, name, unread, children }: {
  page: string
  navigate(page: "messages" | "contacts"): void
  onAccount?(): void
  onSettings?(): void
  onLock?(): void
  self?: AvatarStyle
  name?: string
  unread?: boolean
  children: ReactNode
}) {
  const theme = useUiStore(s => s.theme), toggleTheme = useUiStore(s => s.toggleTheme)
  const showSettings = useUiStore(s => s.showSettings)
  return <aside className="social-sidebar" data-tauri-drag-region={CUSTOM_WINDOW_CHROME ? "deep" : undefined}>
    <nav className="social-rail" aria-label="主导航">
      <div className="rail-brand" data-tauri-drag-region={CUSTOM_WINDOW_CHROME || undefined}>
        <BrandMark className="pointer-events-none" /><span className="pointer-events-none">LLM Nest</span>
      </div>
      <div className="rail-nav">
        <button className={page === "messages" ? "selected" : ""} aria-current={page === "messages" ? "page" : undefined} onClick={() => navigate("messages")}><span><MessageCircle />{unread && <i />}</span><small>消息</small></button>
        <button className={page === "contacts" ? "selected" : ""} aria-current={page === "contacts" ? "page" : undefined} onClick={() => navigate("contacts")}><Users /><small>通讯录</small></button>
      </div>
    </nav>
    {children}
    <div className="rail-bottom">
      {onAccount ? <button className={`sidebar-account ${["account", "ai-management", "account-view"].includes(page) ? "selected" : ""}`} aria-label="我的账号" onClick={onAccount}><Avatar contact={self} self={!self} size="small" /><span>{name || "我的账号"}</span></button> : <span className="sidebar-account"><Avatar self size="small" /><span>我的空间</span></span>}
      <IconButton label={theme === "light" ? "切换深色主题" : "切换浅色主题"} onClick={toggleTheme}>{theme === "light" ? <Moon /> : <Sun />}</IconButton>
      <IconButton label="设置" onClick={() => onSettings ? onSettings() : showSettings()}><Settings /></IconButton>
      {onLock && <IconButton label="锁定 LLM Nest" onClick={onLock}><LockKeyhole /></IconButton>}
    </div>
  </aside>
}
