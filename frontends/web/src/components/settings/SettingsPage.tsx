import { useEffect, useRef, type ReactNode } from "react"
import { ArrowLeft, Check, Info, Moon, Palette, SlidersHorizontal, Sun, Cpu } from "lucide-react"
import { BrandMark } from "@/components/BrandMark"
import { ProviderManager } from "@/components/settings/ProviderManager"
import { GenerationSettings } from "@/components/settings/GenerationSettings"
import { useConfigStore } from "@/store/config"
import { useUiStore, type SettingsSection } from "@/store/ui"
import { IS_DESKTOP } from "@/lib/desktop"
import { cn } from "@/lib/utils"

const sections = [
  { id: "models", label: "模型服务", icon: Cpu, description: "连接模型供应商，管理可用模型与访问密钥。" },
  { id: "generation", label: "生成参数", icon: SlidersHorizontal, description: "调整回答的随机性和最大输出长度。" },
  { id: "appearance", label: "外观", icon: Palette, description: "选择适合当前环境的界面主题。" },
  { id: "about", label: "关于", icon: Info, description: "查看应用版本与当前运行状态。" },
] as const

export function SettingsPage({ notices }: { notices?: ReactNode }) {
  const page = useUiStore(s => s.page)
  const section = useUiStore(s => s.settingsSection)
  const showSettings = useUiStore(s => s.showSettings)
  const showChat = useUiStore(s => s.showChat)
  const theme = useUiStore(s => s.theme)
  const setTheme = useUiStore(s => s.setTheme)
  const apiMode = useUiStore(s => s.apiMode)
  const version = useConfigStore(s => s.version)
  const model = useConfigStore(s => s.config?.currentModel)
  const heading = useRef<HTMLHeadingElement>(null)
  const scroll = useRef<HTMLDivElement>(null)
  const current = sections.find(item => item.id === section) ?? sections[0]

  useEffect(() => {
    if (page !== "settings") return
    heading.current?.focus({ preventScroll: true })
    scroll.current?.scrollTo({ top: 0 })
  }, [page, section])

  const panel = (id: SettingsSection, children: ReactNode) => (
    <section hidden={section !== id} aria-label={sections.find(item => item.id === id)?.label}>
      {children}
    </section>
  )
  return (
    <div className="flex h-full min-h-0 w-full">
      <aside className="flex w-48 shrink-0 flex-col border-r border-sidebar-border bg-sidebar md:w-56">
        <div data-tauri-drag-region={IS_DESKTOP || undefined} className="flex h-14 shrink-0 select-none items-center gap-2.5 px-5">
          <BrandMark className="pointer-events-none" /><span className="pointer-events-none text-[15px] font-semibold">LLM Nest</span>
        </div>
        <div className="px-3 py-4">
          <button type="button" onClick={showChat} className="flex w-full items-center gap-2 rounded-lg px-3 py-2.5 text-sm text-muted-foreground hover:bg-accent hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring">
            <ArrowLeft className="h-4 w-4" />返回聊天
          </button>
        </div>
        <p className="px-6 pb-2 text-xs font-medium text-sidebar-muted">设置</p>
        <nav aria-label="设置分类" className="space-y-1 px-3">
          {sections.map(({ id, label, icon: Icon }) => (
            <button key={id} type="button" aria-current={section === id ? "page" : undefined} onClick={() => showSettings(id)} className={cn("flex w-full items-center gap-3 rounded-lg px-3 py-3 text-sm transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring", section === id ? "bg-accent font-medium text-foreground" : "text-muted-foreground hover:bg-accent/60 hover:text-foreground")}>
              <Icon className="h-4 w-4 shrink-0" />{label}
            </button>
          ))}
        </nav>
        <div className="mt-auto px-6 py-5 text-xs text-sidebar-muted">LLM Nest · {version || "0.1.0"}</div>
      </aside>
      <main className="flex min-w-0 flex-1 flex-col">
        <header data-tauri-drag-region={IS_DESKTOP || undefined} className={cn("flex h-14 shrink-0 select-none items-center gap-2 border-b border-border px-6 text-sm", IS_DESKTOP && "pr-[140px]")}>
          <span className="pointer-events-none text-muted-foreground">设置</span><span className="pointer-events-none text-muted-foreground/50">/</span><span className="pointer-events-none font-medium">{current.label}</span>
        </header>
        {notices}
        <div ref={scroll} className="min-h-0 flex-1 overflow-y-auto">
          <div className="mx-auto max-w-4xl px-6 py-8 lg:px-10">
            <h1 ref={heading} tabIndex={-1} className="text-2xl font-semibold tracking-tight outline-none">{current.label}</h1>
            <p className="mt-2 text-sm leading-relaxed text-muted-foreground">{current.description}</p>
            <div className="mt-7 rounded-xl border border-border bg-card p-5 sm:p-6">
              {panel("models", <ProviderManager />)}
              {panel("generation", <GenerationSettings />)}
              {panel("appearance", <div className="space-y-4">
                <h2 className="text-sm font-medium">界面主题</h2>
                <div className="grid grid-cols-2 gap-4">
                  {([{ key: "light", label: "浅色", icon: Sun }, { key: "dark", label: "深色", icon: Moon }] as const).map(({ key, label, icon: Icon }) => (
                    <button key={key} type="button" aria-pressed={theme === key} onClick={() => setTheme(key)} className={cn("overflow-hidden rounded-xl border-2 text-left transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring", theme === key ? "border-foreground" : "border-border hover:border-muted-foreground")}>
                      <div aria-hidden="true" className={cn("flex h-28 gap-3 p-4", key === "light" ? "bg-zinc-50" : "bg-zinc-950")}>
                        <div className={cn("w-9 rounded", key === "light" ? "bg-zinc-200" : "bg-zinc-800")} />
                        <div className="flex-1 space-y-2 pt-1"><div className={cn("h-3 w-2/3 rounded", key === "light" ? "bg-zinc-300" : "bg-zinc-600")} /><div className={cn("h-8 rounded", key === "light" ? "bg-white shadow-sm" : "bg-zinc-800")} /><div className={cn("ml-auto h-4 w-1/2 rounded", key === "light" ? "bg-zinc-200" : "bg-zinc-700")} /></div>
                      </div>
                      <div className="flex items-center gap-2 px-4 py-3 text-sm"><Icon className="h-4 w-4" />{label}{theme === key && <Check className="ml-auto h-4 w-4" />}</div>
                    </button>
                  ))}
                </div>
                <p className="text-xs text-muted-foreground">立即生效，并在下次启动时保留。</p>
              </div>)}
              {panel("about", <div className="space-y-6">
                <div className="flex items-center gap-4"><BrandMark className="h-16 w-16" /><div><h2 className="text-xl font-semibold">LLM Nest</h2><p className="mt-1 text-sm text-muted-foreground">版本 {version || "0.1.0"}</p></div></div>
                <dl className="space-y-3 text-sm">
                  <div className="flex flex-wrap justify-between gap-2 border-t border-border pt-3"><dt className="text-muted-foreground">运行模式</dt><dd>{apiMode === "tauri" ? "桌面应用" : apiMode === "http" ? "Web 应用" : "本地演示"}</dd></div>
                  <div className="flex flex-wrap justify-between gap-2 border-t border-border pt-3"><dt className="text-muted-foreground">当前模型</dt><dd className="break-all">{model?.model ? `${model.provider} / ${model.model}` : "尚未配置"}</dd></div>
                </dl>
              </div>)}
            </div>
          </div>
        </div>
      </main>
    </div>
  )
}
