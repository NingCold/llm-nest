import type { SessionSummary } from "@/api/types"

/** 思考耗时：'2.8s' / '1m 05s' / '843ms' */
export function formatThinking(ms: number | undefined): string {
  if (ms === undefined || Number.isNaN(ms)) return ""
  if (ms < 1000) return `${Math.round(ms)}ms`
  if (ms < 60_000) return `${(ms / 1000).toFixed(1)}s`
  const m = Math.floor(ms / 60_000)
  const s = Math.round((ms % 60_000) / 1000)
  return `${m}m ${String(s).padStart(2, "0")}s`
}

/** 会话列表相对时间：刚刚 / 5 分钟前 / 2 小时前 / 昨天 / 3 天前 / 2026-08-01 */
export function formatRelativeTime(iso: string, now = Date.now()): string {
  const t = +new Date(iso)
  if (Number.isNaN(t)) return ""
  const diff = now - t
  const MIN = 60_000
  const HOUR = 60 * MIN
  const DAY = 24 * HOUR
  if (diff < MIN) return "刚刚"
  if (diff < HOUR) return `${Math.floor(diff / MIN)} 分钟前`
  if (diff < DAY) return `${Math.floor(diff / HOUR)} 小时前`
  if (diff < 2 * DAY) return "昨天"
  if (diff < 7 * DAY) return `${Math.floor(diff / DAY)} 天前`
  const d = new Date(t)
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(
    d.getDate(),
  ).padStart(2, "0")}`
}

export interface SessionBucket {
  label: string
  items: SessionSummary[]
}

/** 把会话按时间分桶：今天 / 昨天 / 过去 7 天 / 更早 */
export function bucketSessions(
  sessions: SessionSummary[],
  now = Date.now(),
): SessionBucket[] {
  const DAY = 24 * 3600_000
  const startOfToday = new Date(now)
  startOfToday.setHours(0, 0, 0, 0)
  const startOfTodayMs = startOfToday.getTime()

  const buckets: SessionBucket[] = [
    { label: "今天", items: [] },
    { label: "昨天", items: [] },
    { label: "过去 7 天", items: [] },
    { label: "更早", items: [] },
  ]

  for (const s of sessions) {
    const t = +new Date(s.updatedAt)
    const dayDiff = Math.floor((startOfTodayMs - t) / DAY)
    if (dayDiff <= 0) buckets[0].items.push(s)
    else if (dayDiff === 1) buckets[1].items.push(s)
    else if (dayDiff < 7) buckets[2].items.push(s)
    else buckets[3].items.push(s)
  }

  return buckets.filter((b) => b.items.length > 0)
}

/** 复制文本（带剪贴板降级） */
export async function copyText(text: string): Promise<boolean> {
  try {
    await navigator.clipboard.writeText(text)
    return true
  } catch {
    try {
      const ta = document.createElement("textarea")
      ta.value = text
      ta.style.position = "fixed"
      ta.style.opacity = "0"
      document.body.appendChild(ta)
      ta.select()
      document.execCommand("copy")
      document.body.removeChild(ta)
      return true
    } catch {
      return false
    }
  }
}

/** 首字母缩写（头像用） */
export function initials(name: string): string {
  const parts = name.trim().split(/\s+/)
  if (parts.length >= 2) return (parts[0][0] + parts[1][0]).toUpperCase()
  return name.slice(0, 2).toUpperCase()
}

/** 平均 token 速度：'45 tok/s' / '-' */
export function formatTokenSpeed(
  tokens: number | undefined,
  totalMs: number | undefined,
): string {
  if (!tokens || !totalMs || totalMs <= 0) return "-"
  const secs = totalMs / 1000
  if (secs <= 0) return "-"
  return `${Math.round(tokens / secs)} tok/s`
}

/** 缓存命中率：'30.1%' / '-'；命中率 = cached / (prompt + cached) */
export function cacheHitRate(
  cached: number | undefined,
  prompt: number | undefined,
): string {
  if (cached == null || !prompt) return "-"
  return `${((Math.max(0, Math.min(cached, prompt)) / prompt) * 100).toFixed(1)}%`
}

/** 消息时间（epoch ms）→ '14:32:05' */
export function formatMessageTime(epochMs: number): string {
  const d = new Date(epochMs)
  if (Number.isNaN(d.getTime())) return "-"
  return d.toLocaleTimeString("zh-CN", { hour12: false })
}
