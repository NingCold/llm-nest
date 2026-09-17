import { useEffect, useState } from "react"
import { RefreshCw } from "lucide-react"
import type { CatalogStatus } from "@/api/types"
import { useConfigStore } from "@/store/config"

async function getApi() { return (await import("@/api")).getApi() }

export function CatalogSync({ disabled }: { disabled: boolean }) {
  const [status, setStatus] = useState<CatalogStatus | null>(null)
  const [loading, setLoading] = useState(true)
  const [syncing, setSyncing] = useState(false)
  const [error, setError] = useState("")
  const [notice, setNotice] = useState("")
  const [retry, setRetry] = useState(0)
  const setProviders = useConfigStore(s => s.setProviders)

  useEffect(() => {
    let active = true
    setLoading(true)
    setError("")
    getApi().then(api => api.catalogStatus())
      .then(value => { if (active) setStatus(value) })
      .catch(e => { if (active) setError(`目录状态加载失败：${String(e)}`) })
      .finally(() => { if (active) setLoading(false) })
    return () => { active = false }
  }, [retry])

  const sync = async () => {
    if (syncing || disabled) return
    setSyncing(true)
    setError("")
    setNotice("")
    try {
      const { report, providers } = await (await getApi()).syncCatalog()
      setStatus(report.status)
      setProviders(providers)
      setNotice(`已更新目录：${report.status.providerCount} 个提供商，${report.status.modelCount} 条记录。本次新增 ${report.added} 条，更新 ${report.updated} 条。${report.skipped ? `另有 ${report.skipped} 条尚未适配，已跳过。` : ""}`)
    } catch (e) {
      setError(`同步失败：${String(e)}`)
    } finally { setSyncing(false) }
  }

  return <section aria-label="模型目录同步" className="space-y-2 rounded-lg border border-border bg-muted/30 p-3">
    <div className="flex flex-wrap items-center justify-between gap-3">
      <div className="min-w-0">
        <p className="text-sm font-medium">模型目录</p>
        <p className="mt-1 text-xs text-muted-foreground">从 Pi 更新已适配提供商的模型名称和容量信息。</p>
      </div>
      <button type="button" disabled={disabled || loading || syncing || !status?.cachePath}
        onClick={() => void sync()}
        className="flex shrink-0 items-center gap-1.5 rounded-md border border-border bg-background px-3 py-2 text-sm transition-colors hover:bg-accent disabled:cursor-not-allowed disabled:opacity-50">
        <RefreshCw className={`h-3.5 w-3.5 ${syncing ? "animate-spin" : ""}`} />
        {syncing ? "正在同步…" : "同步模型目录"}
      </button>
    </div>
    <p className="text-xs text-muted-foreground">
      {loading ? "正在读取目录状态…" : status?.cachePath
        ? status.updatedAt ? `上次同步：${new Date(status.updatedAt).toLocaleString()}` : "尚未同步，当前使用内置目录。"
        : status ? "当前模式不支持同步，请连接 Web 后端或使用桌面应用。" : "暂时无法读取目录状态。"}
    </p>
    <p className="text-xs text-muted-foreground">保留自定义配置和默认模型。新增提供商仍需单独添加，模型是否可用取决于你的账号。</p>
    {disabled && <p className="text-xs text-muted-foreground">请先完成或关闭供应商编辑，再同步目录。</p>}
    {notice && <p role="status" className="text-sm">{notice}</p>}
    {error && <div role="alert" className="break-words text-sm text-red-600 dark:text-red-400">
      {error}
      {!status && !loading && <button type="button" onClick={() => setRetry(value => value + 1)} className="ml-2 underline">重试加载</button>}
    </div>}
    {!!status?.warnings.length && <div role="status" className="space-y-1 text-xs text-amber-700 dark:text-amber-400">
      {status.warnings.map(warning => <p className="break-words" key={warning}>{warning}</p>)}
    </div>}
  </section>
}
