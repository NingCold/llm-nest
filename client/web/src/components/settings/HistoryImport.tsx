import { useState } from "react"
import { socialClient } from "@/social/client"
import type { LegacyPreview } from "@/social/live-types"

export function HistoryImport() {
  const [preview, setPreview] = useState<LegacyPreview>()
  const [selected, setSelected] = useState<string[]>([])
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState("")
  const [result, setResult] = useState("")
  const inspect = async () => {
    setBusy(true); setError(""); setResult("")
    try { const next = await socialClient.previewLegacy(); setPreview(next); setSelected([]) }
    catch (reason) { setError(String(reason)) }
    finally { setBusy(false) }
  }
  const importSelected = async () => {
    if (!preview) return
    setBusy(true); setError("")
    try {
      const next = await socialClient.importLegacy({ sessions: preview.sessions.filter(s => selected.includes(s.source_id)) })
      setResult(`已导入 ${next.imported} 个历史窗口，${next.already_imported} 个此前已导入。返回消息列表查看。`)
      setPreview(undefined); setSelected([])
    } catch (reason) { setError(String(reason)) }
    finally { setBusy(false) }
  }
  return <section className="space-y-3 border-t border-border pt-5" aria-label="导入历史聊天">
    <h2 className="text-sm font-medium">导入历史聊天</h2>
    <p className="text-sm leading-relaxed text-muted-foreground">读取当前数据目录内的旧版 JSON，导入为只读历史窗口。原文件保留，重复导入不会复制记录。不会恢复旧任务或自动调用模型。</p>
    <button className="social-button secondary" disabled={busy} onClick={() => void inspect()}>检查可导入记录</button>
    {preview && <div className="space-y-3">
      {preview.sessions.length ? preview.sessions.map(s => <label key={s.source_id} className="flex items-start gap-3 text-sm">
        <input type="checkbox" disabled={busy} checked={selected.includes(s.source_id)} onChange={e => setSelected(ids => e.target.checked ? [...ids, s.source_id] : ids.filter(id => id !== s.source_id))} />
        <span>{s.title || "未命名聊天"}<small className="block text-muted-foreground">{s.message_count} 条记录{s.interrupted_checkpoint ? " · 含中断片段" : ""}</small></span>
      </label>) : <p className="text-sm text-muted-foreground">没有找到旧版聊天记录。</p>}
      {!!preview.sessions.length && <button className="social-button primary" disabled={busy || !selected.length} onClick={() => void importSelected()}>导入所选（{selected.length}）</button>}
    </div>}
    {error && <p role="alert" className="text-sm text-destructive">{error}</p>}
    {result && <p role="status" className="text-sm">{result}</p>}
  </section>
}
