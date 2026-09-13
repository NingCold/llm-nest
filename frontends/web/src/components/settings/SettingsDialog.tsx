import { useState } from "react"
import { Moon, Settings, Sun } from "lucide-react"
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
} from "@/components/ui/dialog"
import { ProviderManager } from "@/components/settings/ProviderManager"
import { BrandMark } from "@/components/BrandMark"
import { useConfigStore } from "@/store/config"
import { useUiStore } from "@/store/ui"
import { cn } from "@/lib/utils"

export function SettingsDialog() {
  const theme = useUiStore((s) => s.theme)
  const setTheme = useUiStore((s) => s.setTheme)
  const apiMode = useUiStore((s) => s.apiMode)
  const version = useConfigStore((s) => s.version)
  const model = useConfigStore((s) => s.config?.currentModel)

  return (
    <Dialog>
      <DialogTrigger asChild>
        <button
          type="button"
          className="flex h-9 w-9 items-center justify-center rounded-lg text-sidebar-foreground/70 transition-colors hover:bg-accent hover:text-sidebar-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
          aria-label="设置"
        >
          <Settings className="h-4.5 w-4.5" />
        </button>
      </DialogTrigger>
      <DialogContent className="max-w-md max-h-[85vh] overflow-y-auto">
        <DialogHeader>
          <DialogTitle>设置</DialogTitle>
          <DialogDescription>模型、生成参数与外观</DialogDescription>
        </DialogHeader>

        <div className="space-y-6 py-2">
          {/* Profile */}
          <div className="flex items-center gap-3">
            <BrandMark className="h-12 w-12" />
            <div className="min-w-0">
              <p className="font-medium">LLM Nest</p>
              <p className="truncate text-sm text-muted-foreground">
                {apiMode === "tauri"
                  ? "桌面模式"
                  : apiMode === "http"
                    ? "Web 模式 · 真实后端"
                    : "本地演示模式 · 数据保存在浏览器"}
              </p>
            </div>
          </div>

          {/* 模型供应商（添加/编辑/删除，DSH ui-settings-models 风格） */}
          <ProviderManager />

          <GenerationSettings />

          {/* Appearance */}
          <div className="space-y-2.5">
            <p className="text-sm font-medium">外观</p>
            <div className="grid grid-cols-2 gap-2">
              {(
                [
                  { key: "light", label: "浅色", icon: Sun },
                  { key: "dark", label: "深色", icon: Moon },
                ] as const
              ).map(({ key, label, icon: Icon }) => (
                <button
                  key={key}
                  type="button"
                  onClick={() => setTheme(key)}
                  className={cn(
                    "flex items-center justify-center gap-2 rounded-lg border px-3 py-2.5 text-sm font-medium transition-colors",
                    theme === key
                      ? "border-zinc-900 bg-zinc-900 text-white dark:border-zinc-100 dark:bg-zinc-100 dark:text-zinc-900"
                      : "border-border hover:bg-accent",
                  )}
                >
                  <Icon className="h-4 w-4" />
                  {label}
                </button>
              ))}
            </div>
          </div>

          {/* About */}
          <div className="space-y-2.5">
            <p className="text-sm font-medium">关于</p>
            <div className="rounded-lg border border-border bg-muted/40 p-3 text-sm text-muted-foreground">
              <p>
                <span className="font-medium text-foreground">LLM Nest</span> · v
                {version || "0.1.0"}
              </p>
              <p className="mt-1">
                当前模型：
                {model ? `${model.provider} / ${model.model}` : "未选择"}
              </p>
            </div>
          </div>
        </div>
      </DialogContent>
    </Dialog>
  )
}

function GenerationSettings() {
  const config = useConfigStore(s => s.config)
  const saving = useConfigStore(s => s.saving)
  const saveError = useConfigStore(s => s.saveError)
  const update = useConfigStore(s => s.updateConfig)
  const [temperature, setTemperature] = useState(String(Number((config?.temperature ?? 0.7).toFixed(6))))
  const [maxTokens, setMaxTokens] = useState(String(config?.maxTokens ?? ""))
  const [notice, setNotice] = useState("")
  return <form className="space-y-3" onSubmit={async e => {
    e.preventDefault()
    const t = Number(temperature), m = maxTokens.trim() ? Number(maxTokens) : undefined
    if (!temperature.trim() || !Number.isFinite(t) || t < 0 || t > 2 || (m !== undefined && (!Number.isSafeInteger(m) || m < 1 || m > 4294967295))) {
      setNotice("温度应为 0–2；最大输出应为正整数或留空。")
      return
    }
    setNotice("")
    if (await update({ temperature: t, maxTokens: m })) setNotice("已保存，下一次生成生效。")
  }}>
    <p className="text-sm font-medium">生成设置</p>
    <p className="text-xs text-muted-foreground">模型选择与参数保存在配置文件中。已有会话恢复上次使用的模型，新会话使用保存的默认模型。</p>
    <label className="block text-sm">温度（0–2）<input aria-label="温度" type="number" min="0" max="2" step="any" required value={temperature} onChange={e => {setTemperature(e.target.value);setNotice("")}} className="mt-1 w-full rounded border bg-background p-2" /></label>
    <label className="block text-sm">最大输出 tokens<input aria-label="最大输出 tokens" type="number" min="1" max="4294967295" step="1" placeholder="留空使用模型默认值" value={maxTokens} onChange={e => {setMaxTokens(e.target.value);setNotice("")}} className="mt-1 w-full rounded border bg-background p-2" /></label>
    <button disabled={saving} className="rounded border px-3 py-2 text-sm disabled:opacity-50">{saving ? "正在保存…" : "保存生成设置"}</button>
    {(saveError || notice) && <p role="status" className="break-words text-sm">{saveError || notice}</p>}
  </form>
}
