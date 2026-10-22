import { useEffect, useState } from "react"
import { FolderOpen } from "lucide-react"
import { invoke } from "@tauri-apps/api/core"
import { IS_DESKTOP } from "@/lib/desktop"
import { HistoryImport } from "./HistoryImport"

interface AppPaths {
  configFile: string
  dataDir: string
}

export function StorageSettings() {
  const [paths, setPaths] = useState<AppPaths>()
  const [error, setError] = useState("")
  const [attempt, setAttempt] = useState(0)
  const [opening, setOpening] = useState(false)

  useEffect(() => {
    if (!IS_DESKTOP) return
    let active = true
    setError("")
    void invoke<AppPaths>("get_app_paths").then(value => {
      if (active) setPaths(value)
    }).catch(reason => { if (active) setError(`无法读取配置路径：${String(reason)}`) })
    return () => { active = false }
  }, [attempt])

  const open = async (kind: "config" | "data") => {
    setOpening(true)
    setError("")
    try { await invoke("open_app_directory", { kind }) }
    catch (reason) { setError(String(reason)) }
    finally { setOpening(false) }
  }

  return <div className="space-y-4">
    <h2 className="text-sm font-medium">配置与数据</h2>
    {!IS_DESKTOP ? <p className="text-sm text-muted-foreground">Web 版的配置和聊天保存在运行服务的机器上，请在该机器管理文件。桌面版可在这里直接打开目录。</p> : <>
      {paths ? <div className="space-y-4">
        {([
          { kind: "config", label: "配置文件", path: paths.configFile, action: "打开配置目录" },
          { kind: "data", label: "聊天数据目录", path: paths.dataDir, action: "打开数据目录" },
        ] as const).map(item => <div key={item.kind} className="space-y-2">
          <p className="text-xs text-muted-foreground">{item.label}</p>
          <p className="select-text break-all rounded-md bg-muted px-3 py-2 font-mono text-xs leading-relaxed">{item.path}</p>
          <button type="button" disabled={opening} onClick={() => void open(item.kind)} className="inline-flex items-center gap-2 rounded-md border border-border px-3 py-2 text-sm hover:bg-accent disabled:opacity-50 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring">
            <FolderOpen className="h-4 w-4" />{item.action}
          </button>
        </div>)}
        <p className="text-xs leading-relaxed text-muted-foreground">配置保存为 llmn.toml，聊天与账号保存在 social.sqlite3。备份或迁移聊天前，请先关闭应用。</p>
      </div> : !error && <p role="status" className="text-sm text-muted-foreground">正在读取路径…</p>}
      {error && <div role="alert" className="space-y-2 text-sm text-red-500"><p className="break-words">{error}</p>{!paths && <button type="button" onClick={() => setAttempt(value => value + 1)} className="underline">重试</button>}</div>}
    </>}
    <HistoryImport />
  </div>
}
