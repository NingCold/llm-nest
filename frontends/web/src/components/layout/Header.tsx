import { useEffect, useRef, useState, type ReactNode } from "react"
import {
  Brain,
  Globe,
  Moon,
  PanelLeft,
  Pencil,
  Plus,
  Sun,
} from "lucide-react"
import { ModelPicker } from "@/components/layout/ModelPicker"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
} from "@/components/ui/select"
import { Switch } from "@/components/ui/switch"
import { useConfigStore } from "@/store/config"
import { useSessionStore } from "@/store/session"
import { useUiStore, clampEffort } from "@/store/ui"
import { cn } from "@/lib/utils"

const EFFORT_LABEL: Record<string, string> = {
  off: "关闭",
  low: "低",
  medium: "中",
  high: "高",
  max: "最强",
}

export function Header() {
  const collapsed = useUiStore((s) => s.sidebarCollapsed)
  const toggleSidebar = useUiStore((s) => s.toggleSidebar)
  const reasoningEffort = useUiStore((s) => s.reasoningEffort)
  const setReasoningEffort = useUiStore((s) => s.setReasoningEffort)
  const webSearchEnabled = useUiStore((s) => s.webSearchEnabled)
  const toggleWebSearch = useUiStore((s) => s.toggleWebSearch)
  const theme = useUiStore((s) => s.theme)
  const toggleTheme = useUiStore((s) => s.toggleTheme)

  const config = useConfigStore((s) => s.config)
  const providers = useConfigStore((s) => s.providers)
  const sessions = useSessionStore((s) => s.sessions)
  const currentSessionId = useSessionStore((s) => s.currentSessionId)
  const renameSession = useSessionStore((s) => s.renameSession)
  const createSession = useSessionStore((s) => s.createSession)

  const current = sessions.find((s) => s.id === currentSessionId)
  const [editing, setEditing] = useState(false)
  const [draft, setDraft] = useState("")
  const inputRef = useRef<HTMLInputElement>(null)

  // 当前模型支持的思考级别（来自模型快照/配置），用于渲染强度选项
  const reasoningLevels = (() => {
    const p = providers.find((x) => x.id === config?.currentModel.provider)
    return p?.models.find((m) => m.id === config?.currentModel.model)
      ?.reasoningLevels
  })()
  const effectiveEffort = clampEffort(reasoningEffort, reasoningLevels)

  useEffect(() => {
    if (editing) {
      inputRef.current?.focus()
      inputRef.current?.select()
    }
  }, [editing])

  const commit = async () => {
    setEditing(false)
    const next = draft.trim()
    if (next && currentSessionId && next !== current?.title) {
      await renameSession(currentSessionId, next)
    }
  }

  return (
    <header className="flex h-14 shrink-0 items-center gap-1 border-b border-border bg-background/80 px-3 backdrop-blur-sm">
      {/* Left: collapse + title */}
      <button
        type="button"
        onClick={toggleSidebar}
        className="flex h-9 w-9 items-center justify-center rounded-lg text-muted-foreground transition-colors hover:bg-accent hover:text-foreground"
        aria-label={collapsed ? "展开侧边栏" : "收起侧边栏"}
      >
        <PanelLeft className="h-4.5 w-4.5" />
      </button>

      <button
        type="button"
        onClick={() => void createSession()}
        className="flex h-9 w-9 items-center justify-center rounded-lg text-muted-foreground transition-colors hover:bg-accent hover:text-foreground"
        aria-label="新建对话"
      >
        <Plus className="h-4.5 w-4.5" />
      </button>

      <div className="mx-1 h-5 w-px bg-border" />

      <div className="group relative flex min-w-0 items-center">
        {editing ? (
          <input
            ref={inputRef}
            value={draft}
            onChange={(e) => setDraft(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") void commit()
              if (e.key === "Escape") setEditing(false)
            }}
            onBlur={() => void commit()}
            className="h-8 w-64 max-w-[40vw] rounded-md border border-input bg-card px-2 text-sm font-medium outline-none focus:ring-2 focus:ring-ring"
          />
        ) : (
          <>
            <h1
              className="max-w-[28vw] truncate text-[15px] font-semibold tracking-tight"
              onDoubleClick={() => {
                if (current) {
                  setDraft(current.title)
                  setEditing(true)
                }
              }}
              title={current ? `${current.title}（双击重命名）` : undefined}
            >
              {current?.title ?? "LLM Nest"}
            </h1>
            {current && (
              <button
                type="button"
                onClick={() => {
                  setDraft(current.title)
                  setEditing(true)
                }}
                className="ml-1.5 flex h-7 w-7 items-center justify-center rounded-md text-muted-foreground opacity-0 transition-opacity group-hover:opacity-100 hover:bg-accent hover:text-foreground"
                aria-label="重命名对话"
              >
                <Pencil className="h-3.5 w-3.5" />
              </button>
            )}
          </>
        )}
      </div>

      <div className="flex-1" />

      {/* Right: model + toggles */}
      <div className="flex items-center gap-0.5">
        <ModelPicker className="hidden sm:flex" />

        <div className="mx-1.5 hidden h-5 w-px bg-border sm:block" />

        {/* 思考强度：选项来自当前模型的 reasoning levels（模型快照/配置） */}
        <Select
          value={effectiveEffort}
          onValueChange={setReasoningEffort}
          disabled={!reasoningLevels || reasoningLevels.length === 0}
        >
          <SelectTrigger className="hidden h-9 w-auto gap-2 border border-transparent bg-transparent px-2.5 text-sm text-muted-foreground shadow-none hover:bg-accent hover:text-accent-foreground focus:ring-0 focus:ring-offset-0 md:flex">
            <Brain className="h-4 w-4 shrink-0" />
            <span className="text-muted-foreground">思考</span>
            <span className="font-medium text-foreground">
              {EFFORT_LABEL[effectiveEffort] ?? effectiveEffort}
            </span>
          </SelectTrigger>
          <SelectContent className="w-36">
            {(reasoningLevels ?? []).map((l) => (
              <SelectItem key={l} value={l} className="py-1.5">
                {EFFORT_LABEL[l] ?? l}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>

        <ToggleItem
          icon={<Globe className="h-4 w-4" />}
          label="联网搜索"
          checked={webSearchEnabled}
          onChange={toggleWebSearch}
          className="hidden lg:flex"
        />

        <div className="mx-1.5 hidden h-5 w-px bg-border md:block" />

        <button
          type="button"
          onClick={toggleTheme}
          className="flex h-9 w-9 items-center justify-center rounded-lg text-muted-foreground transition-colors hover:bg-accent hover:text-foreground"
          aria-label="切换主题"
        >
          {theme === "dark" ? <Sun className="h-4.5 w-4.5" /> : <Moon className="h-4.5 w-4.5" />}
        </button>
      </div>
    </header>
  )
}

function ToggleItem({
  icon,
  label,
  checked,
  onChange,
  className,
}: {
  icon: ReactNode
  label: string
  checked: boolean
  onChange: () => void
  className?: string
}) {
  return (
    <div
      role="button"
      tabIndex={0}
      onClick={onChange}
      onKeyDown={(e) => {
        if (e.key === "Enter" || e.key === " ") {
          e.preventDefault()
          onChange()
        }
      }}
      className={cn(
        "flex h-9 cursor-pointer select-none items-center gap-2 rounded-lg px-2.5 text-sm text-muted-foreground transition-colors hover:bg-accent hover:text-foreground",
        className,
      )}
    >
      {icon}
      <span>{label}</span>
      <Switch checked={checked} onCheckedChange={onChange} size="sm" />
    </div>
  )
}
