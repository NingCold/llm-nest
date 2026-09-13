import { useEffect, useRef, useState } from "react"
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
import { MODEL_SETTINGS_ACTION, ModelSetupOptions } from "@/components/layout/ModelSetupOptions"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
} from "@/components/ui/select"
import { useConfigStore } from "@/store/config"
import { useSessionStore } from "@/store/session"
import { useUiStore, clampEffort } from "@/store/ui"
import { IS_DESKTOP } from "@/lib/desktop"
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
  const theme = useUiStore((s) => s.theme)
  const toggleTheme = useUiStore((s) => s.toggleTheme)
  const showSettings = useUiStore(s => s.showSettings)

  const config = useConfigStore((s) => s.config)
  const providers = useConfigStore((s) => s.providers)
  const sessions = useSessionStore((s) => s.sessions)
  const currentSessionId = useSessionStore((s) => s.currentSessionId)
  const renameSession = useSessionStore((s) => s.renameSession)
  const createSession = useSessionStore((s) => s.createSession)

  const current = sessions.find((s) => s.id === currentSessionId)
  const [error, setError] = useState<string | null>(null)
  const [creating, setCreating] = useState(false)
  const [editing, setEditing] = useState(false)
  const [draft, setDraft] = useState("")
  const inputRef = useRef<HTMLInputElement>(null)

  // 当前模型支持的思考级别（来自模型快照/配置），用于渲染强度选项
  const selectedModel = (() => {
    const p = providers.find((x) => x.id === config?.currentModel.provider)
    return p?.models.find((m) => m.id === config?.currentModel.model)
  })()
  const hasModels = providers.some(p => p.models.length > 0)
  const reasoningLevels = selectedModel?.reasoningLevels
  const supportsReasoning = Boolean(reasoningLevels?.some(level => level !== "off"))
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
      try { await renameSession(currentSessionId, next); setError(null) }
      catch (e) { setError(`重命名失败：${String(e)}`); setEditing(true) }
    }
  }

  return (
    <header data-tauri-drag-region={IS_DESKTOP || undefined} className={cn("relative flex h-14 shrink-0 items-center gap-1 border-b border-border bg-background/80 px-3 backdrop-blur-sm", IS_DESKTOP && "pr-[140px]")}>
      {error && <p role="alert" className="absolute top-14 left-0 z-50 max-w-full border bg-background p-3 text-sm text-red-500">{error}<button className="ml-2 underline" onClick={() => setError(null)}>关闭</button></p>}
      {/* Left: collapse + title */}
      <button
        type="button"
        onClick={toggleSidebar}
        className="flex h-9 w-9 shrink-0 items-center justify-center rounded-lg text-muted-foreground transition-colors hover:bg-accent hover:text-foreground"
        aria-label={collapsed ? "展开侧边栏" : "收起侧边栏"}
      >
        <PanelLeft className="h-4.5 w-4.5" />
      </button>

      <button
        type="button"
        disabled={creating}
        onClick={async () => { setCreating(true); setError(null); try { await createSession() } catch (e) { setError(`创建失败：${String(e)}`) } finally { setCreating(false) } }}
        className="flex h-9 w-9 shrink-0 items-center justify-center rounded-lg text-muted-foreground transition-colors hover:bg-accent hover:text-foreground"
        aria-label="新建对话"
      >
        <Plus className="h-4.5 w-4.5" />
      </button>

      <div className="mx-1 h-5 w-px shrink-0 bg-border" />

      <div data-tauri-drag-region={IS_DESKTOP || undefined} className="group relative flex min-w-0 flex-1 items-center">
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
            className="h-8 w-full min-w-0 max-w-64 rounded-md border border-input bg-card px-2 text-sm font-medium outline-none focus:ring-2 focus:ring-ring"
          />
        ) : (
          <>
            <h1
              className="truncate text-[15px] font-semibold tracking-tight"
              onDoubleClick={() => {
                if (current) {
                  setDraft(current.title)
                  setEditing(true)
                }
              }}
              title={current ? `${current.title}（双击重命名）` : undefined}
            >
              {current?.title ?? "LLM-Nest"}
            </h1>
            {current && (
              <button
                type="button"
                onClick={() => {
                  setDraft(current.title)
                  setEditing(true)
                }}
                className="ml-1.5 flex h-7 w-7 shrink-0 items-center justify-center rounded-md text-muted-foreground opacity-0 transition-opacity group-hover:opacity-100 focus-visible:opacity-100 hover:bg-accent hover:text-foreground"
                aria-label="重命名对话"
              >
                <Pencil className="h-3.5 w-3.5" />
              </button>
            )}
          </>
        )}
      </div>

      {/* Right: model + toggles */}
      <div className="flex shrink-0 items-center gap-0.5 whitespace-nowrap">
        <ModelPicker className={cn("hidden sm:flex", IS_DESKTOP && "max-w-[min(18vw,14rem)]")} />

        <div className="mx-1.5 hidden h-5 w-px bg-border sm:block" />

        {/* 思考强度：选项来自当前模型的 reasoning levels（模型快照/配置） */}
        <Select
          value={supportsReasoning ? effectiveEffort : ""}
          onValueChange={value => {
            if (value === MODEL_SETTINGS_ACTION) showSettings("models")
            else setReasoningEffort(value)
          }}
        >
          <SelectTrigger aria-label="思考强度" className={cn("hidden h-9 w-auto gap-2 border border-transparent bg-transparent px-2.5 text-sm text-muted-foreground shadow-none hover:bg-accent hover:text-accent-foreground focus:ring-0 focus:ring-offset-0 md:flex", IS_DESKTOP && "max-lg:gap-1 max-lg:px-1.5")}>
            <Brain className="h-4 w-4 shrink-0" />
            <span className="hidden text-muted-foreground xl:inline">思考</span>
            <span className="font-medium text-foreground">
              {supportsReasoning ? EFFORT_LABEL[effectiveEffort] ?? effectiveEffort : selectedModel ? "不支持" : "未配置"}
            </span>
          </SelectTrigger>
          <SelectContent className={supportsReasoning ? "w-36" : "w-72"}>
            {!supportsReasoning && <ModelSetupOptions
              message={!hasModels ? "暂无可用模型。添加模型后可查看其支持的思考模式。" : !selectedModel ? "请先选择一个可用模型，再设置思考模式。" : "当前模型未提供可用的思考模式，可切换支持思考的模型。"}
              action={hasModels ? "管理模型" : "添加模型"}
            />}
            {supportsReasoning && (reasoningLevels ?? []).map((l) => (
              <SelectItem key={l} value={l} className="py-1.5">
                {EFFORT_LABEL[l] ?? l}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>

        <button disabled aria-label="联网搜索（未接入）" title="联网搜索尚未接入" className="hidden h-9 shrink-0 items-center gap-2 px-2 font-sans text-sm font-medium leading-5 text-muted-foreground opacity-50 xl:flex">
          <Globe className="h-4 w-4 shrink-0" /><span className="hidden 2xl:inline">联网搜索（未接入）</span>
        </button>

        <div className="mx-1.5 hidden h-5 w-px bg-border md:block" />

        <button
          type="button"
          onClick={toggleTheme}
          className="flex h-9 w-9 shrink-0 items-center justify-center rounded-lg text-muted-foreground transition-colors hover:bg-accent hover:text-foreground"
          aria-label="切换主题"
        >
          {theme === "dark" ? <Sun className="h-4.5 w-4.5" /> : <Moon className="h-4.5 w-4.5" />}
        </button>
      </div>
    </header>
  )
}
