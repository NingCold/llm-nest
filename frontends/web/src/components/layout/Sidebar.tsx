import { useEffect, useMemo, useRef, useState } from "react"
import {
  MessageSquare,
  PanelLeftClose,
  Pencil,
  Plus,
  Search,
  Sparkles,
  Trash2,
  X,
} from "lucide-react"
import { useSessionStore } from "@/store/session"
import { useUiStore } from "@/store/ui"
import { bucketSessions, formatRelativeTime, initials } from "@/lib/format"
import { SettingsDialog } from "@/components/settings/SettingsDialog"
import { cn } from "@/lib/utils"
import type { SessionSummary } from "@/api/types"

/* ---------------- session row (rename / delete inline) ---------------- */

function SessionRow({
  session,
  active,
  onSelect,
}: {
  session: SessionSummary
  active: boolean
  onSelect: () => void
}) {
  const [error, setError] = useState<string | null>(null)
  const [editing, setEditing] = useState(false)
  const [draft, setDraft] = useState(session.title)
  const [confirming, setConfirming] = useState(false)
  const renameSession = useSessionStore((s) => s.renameSession)
  const deleteSession = useSessionStore((s) => s.deleteSession)
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null)

  useEffect(() => () => {
    if (timer.current) clearTimeout(timer.current)
  }, [])

  const commitRename = async () => {
    const next = draft.trim()
    setEditing(false)
    if (next && next !== session.title) {
      try { await renameSession(session.id, next); setError(null) }
      catch (err) { setError(`重命名失败：${String(err)}`); setEditing(true) }
    } else {
      setDraft(session.title)
    }
  }

  const askDelete = () => {
    if (confirming) {
      void deleteSession(session.id).catch(err => setError(`删除失败：${String(err)}`))
      return
    }
    setConfirming(true)
    timer.current = setTimeout(() => setConfirming(false), 3000)
  }

  if (editing) {
    return (
      <div className="flex flex-col rounded-lg px-1 py-1">
        {error && <p role="alert" className="text-xs text-red-500">{error}</p>}
        <input
          autoFocus
          value={draft}
          onChange={(e) => setDraft(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") void commitRename()
            if (e.key === "Escape") {
              setDraft(session.title)
              setEditing(false)
            }
          }}
          onBlur={() => void commitRename()}
          className="h-8 w-full rounded-md border border-input bg-card px-2 text-sm outline-none focus:ring-2 focus:ring-ring"
        />
      </div>
    )
  }

  return (
    <div
      className={cn(
        "group relative flex items-center rounded-lg transition-colors",
        active ? "bg-accent" : "hover:bg-accent/60",
      )}
    >
      {error && <p role="alert" className="text-xs text-red-500">{error}</p>}
      <button
        type="button"
        onClick={onSelect}
        className={cn(
          "flex min-w-0 flex-1 items-center gap-2.5 px-2.5 py-2 text-left text-sm",
          active ? "font-medium text-accent-foreground" : "text-sidebar-foreground",
        )}
      >
        <MessageSquare className="h-4 w-4 shrink-0 text-sidebar-muted" />
        <span className="min-w-0 flex-1 truncate">{session.title}</span>
        <span className="shrink-0 text-[11px] text-sidebar-muted group-hover:hidden">
          {formatRelativeTime(session.updatedAt)}
        </span>
      </button>
      <div className="absolute right-1.5 hidden items-center gap-0.5 rounded-md bg-card p-0.5 shadow-sm group-hover:flex">
        <button
          type="button"
          onClick={() => {
            setDraft(session.title)
            setEditing(true)
          }}
          className="rounded p-1.5 text-sidebar-foreground transition-colors hover:bg-accent"
          aria-label="重命名"
        >
          <Pencil className="h-3.5 w-3.5" />
        </button>
        {confirming ? (
          <button
            type="button"
            onMouseLeave={() => setConfirming(false)}
            onClick={askDelete}
            className="rounded px-2 py-1 text-xs font-semibold text-red-500 transition-colors hover:bg-red-50 dark:hover:bg-red-950/40"
          >
            删除
          </button>
        ) : (
          <button
            type="button"
            onClick={askDelete}
            className="rounded p-1.5 text-sidebar-foreground transition-colors hover:bg-accent hover:text-red-500"
            aria-label="删除"
          >
            <Trash2 className="h-3.5 w-3.5" />
          </button>
        )}
      </div>
    </div>
  )
}

/* ---------------- sidebar ---------------- */

export function Sidebar() {
  const [error, setError] = useState<string | null>(null)
  const [creating, setCreating] = useState(false)
  const collapsed = useUiStore((s) => s.sidebarCollapsed)
  const toggleSidebar = useUiStore((s) => s.toggleSidebar)
  const apiMode = useUiStore((s) => s.apiMode)
  const searchQuery = useUiStore((s) => s.searchQuery)
  const setSearchQuery = useUiStore((s) => s.setSearchQuery)

  const sessions = useSessionStore((s) => s.sessions)
  const currentSessionId = useSessionStore((s) => s.currentSessionId)
  const setCurrentSession = useSessionStore((s) => s.setCurrentSession)
  const createSession = useSessionStore((s) => s.createSession)

  const buckets = useMemo(() => {
    const q = searchQuery.trim().toLowerCase()
    const list = q
      ? sessions.filter((s) => s.title.toLowerCase().includes(q))
      : sessions
    return bucketSessions(list)
  }, [sessions, searchQuery])

  const hasResults = buckets.some((b) => b.items.length > 0)

  return (
    <aside
      className={cn(
        "relative h-full shrink-0 border-r border-sidebar-border bg-sidebar transition-[width] duration-300 ease-in-out",
        collapsed ? "w-0 border-r-0" : "w-[260px]",
      )}
    >
      <div
        className={cn(
          "flex h-full w-[260px] flex-col overflow-hidden transition-opacity duration-200",
          collapsed && "opacity-0",
        )}
      >
        {/* Brand */}
        <div className="flex items-center gap-2.5 px-4 pb-1 pt-4">
          <div className="flex h-8 w-8 items-center justify-center rounded-lg bg-zinc-900 text-white dark:bg-zinc-100 dark:text-zinc-900">
            <Sparkles className="h-4.5 w-4.5" />
          </div>
          <span className="text-[15px] font-semibold tracking-tight">LLM Nest</span>
        </div>

        {/* New chat */}
        <div className="px-3 pt-3">
          <button
            type="button"
            disabled={creating}
            onClick={async () => { setCreating(true); setError(null); try { await createSession() } catch (err) { setError(`创建失败：${String(err)}`) } finally { setCreating(false) } }}
            className="flex w-full items-center justify-center gap-2 rounded-lg bg-zinc-900 px-3 py-2.5 text-sm font-medium text-white shadow-sm transition-colors hover:bg-zinc-700 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring dark:bg-zinc-100 dark:text-zinc-900 dark:hover:bg-zinc-300"
          >
            <Plus className="h-4 w-4" />
            新建对话
          </button>
        </div>

        {error && <p role="alert" className="px-3 pt-2 text-xs text-red-500">{error}</p>}
        {/* Search */}
        <div className="px-3 pt-3">
          <div className="relative">
            <Search className="pointer-events-none absolute left-2.5 top-1/2 h-4 w-4 -translate-y-1/2 text-sidebar-muted" />
            <input
              value={searchQuery}
              onChange={(e) => setSearchQuery(e.target.value)}
              placeholder="搜索对话…"
              className="h-9 w-full rounded-lg border border-transparent bg-muted pl-8 pr-8 text-sm outline-none transition-colors placeholder:text-sidebar-muted focus:border-input focus:bg-card"
            />
            {searchQuery && (
              <button
                type="button"
                onClick={() => setSearchQuery("")}
                className="absolute right-2 top-1/2 -translate-y-1/2 rounded p-0.5 text-sidebar-muted transition-colors hover:text-sidebar-foreground"
                aria-label="清除搜索"
              >
                <X className="h-4 w-4" />
              </button>
            )}
          </div>
        </div>

        {/* Session list */}
        <div className="mt-2 flex-1 overflow-y-auto px-3 pb-3">
          {!hasResults ? (
            <p className="px-2.5 pt-6 text-center text-sm text-sidebar-muted">
              未找到相关对话
            </p>
          ) : (
            buckets.map((bucket) => (
              <div key={bucket.label} className="mb-0.5">
                <p className="px-2.5 pb-1 pt-3.5 text-[11px] font-medium uppercase tracking-wider text-sidebar-muted">
                  {bucket.label}
                </p>
                {bucket.items.map((s) => (
                  <SessionRow
                    key={s.id}
                    session={s}
                    active={s.id === currentSessionId}
                    onSelect={() => setCurrentSession(s.id)}
                  />
                ))}
              </div>
            ))
          )}
        </div>

        {/* User footer */}
        <div className="flex items-center gap-2 border-t border-sidebar-border px-3 py-2.5">
          <div className="flex h-8 w-8 shrink-0 items-center justify-center rounded-full bg-gradient-to-br from-zinc-500 to-zinc-800 text-xs font-semibold text-white">
            {initials("LLM Nest")}
          </div>
          <div className="min-w-0 flex-1">
            <p className="truncate text-sm font-medium leading-tight">LLM Nest</p>
            <p className="truncate text-[11px] leading-tight text-sidebar-muted">
              {apiMode === "tauri"
                ? "桌面模式"
                : apiMode === "http"
                  ? "Web 模式 · 真实后端"
                  : "本地演示模式 · 数据在浏览器"}
            </p>
          </div>
          <SettingsDialog />
          <button
            type="button"
            onClick={toggleSidebar}
            className="flex h-9 w-9 items-center justify-center rounded-lg text-sidebar-foreground/70 transition-colors hover:bg-accent hover:text-sidebar-foreground"
            aria-label="收起侧边栏"
          >
            <PanelLeftClose className="h-4.5 w-4.5" />
          </button>
        </div>
      </div>
    </aside>
  )
}
