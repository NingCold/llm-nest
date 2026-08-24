import { Moon, Settings, Sun } from "lucide-react"
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
} from "@/components/ui/dialog"
import { useConfigStore } from "@/store/config"
import { useUiStore } from "@/store/ui"
import { initials } from "@/lib/format"
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
      <DialogContent className="max-w-md">
        <DialogHeader>
          <DialogTitle>设置</DialogTitle>
          <DialogDescription>外观与应用信息</DialogDescription>
        </DialogHeader>

        <div className="space-y-6 py-2">
          {/* Profile */}
          <div className="flex items-center gap-3">
            <div className="flex h-12 w-12 items-center justify-center rounded-full bg-gradient-to-br from-zinc-600 to-zinc-900 text-sm font-semibold text-white dark:from-zinc-400 dark:to-zinc-700">
              {initials("Demo User")}
            </div>
            <div className="min-w-0">
              <p className="font-medium">Demo User</p>
              <p className="truncate text-sm text-muted-foreground">
                {apiMode === "tauri"
                  ? "桌面模式"
                  : apiMode === "http"
                    ? "Web 模式 · 真实后端"
                    : "本地演示模式 · 数据保存在浏览器"}
              </p>
            </div>
          </div>

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
