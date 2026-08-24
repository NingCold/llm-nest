import { Sparkles } from "lucide-react"
import { PROMPT_TEMPLATES } from "@/lib/prompts"

export function EmptyState({ onPick }: { onPick: (prompt: string) => void }) {
  return (
    <div className="flex flex-1 flex-col items-center justify-center px-4 py-16 animate-fade-up">
      <div className="flex h-14 w-14 items-center justify-center rounded-2xl bg-gradient-to-br from-zinc-700 to-zinc-950 text-white shadow-lg dark:from-zinc-200 dark:to-zinc-500 dark:text-zinc-900">
        <Sparkles className="h-7 w-7" />
      </div>
      <h2 className="mt-5 text-2xl font-semibold tracking-tight">有什么可以帮你？</h2>
      <p className="mt-1.5 text-sm text-muted-foreground">
        选择一个快捷模板，或直接输入你的问题
      </p>

      <div className="mt-8 grid w-full max-w-2xl grid-cols-1 gap-3 sm:grid-cols-2">
        {PROMPT_TEMPLATES.map((t) => {
          const Icon = t.icon
          return (
            <button
              key={t.id}
              type="button"
              onClick={() => onPick(t.prompt)}
              className="group flex items-start gap-3 rounded-xl border border-border bg-card p-4 text-left transition-all hover:border-zinc-400 hover:shadow-md hover:shadow-zinc-900/5 dark:hover:border-zinc-500"
            >
              <div className="flex h-9 w-9 shrink-0 items-center justify-center rounded-lg bg-muted text-muted-foreground transition-colors group-hover:bg-zinc-900 group-hover:text-white dark:group-hover:bg-zinc-100 dark:group-hover:text-zinc-900">
                <Icon className="h-4.5 w-4.5" />
              </div>
              <div className="min-w-0">
                <p className="text-sm font-medium">{t.title}</p>
                <p className="mt-0.5 truncate text-xs text-muted-foreground">
                  {t.description}
                </p>
              </div>
            </button>
          )
        })}
      </div>
    </div>
  )
}
