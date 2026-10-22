import { useState } from "react"
import { useConfigStore } from "@/store/config"
import { imageSourceLabels, imageSupportLabels } from "@/lib/image-input"

export function ModelImageInput({ disabled }: { disabled: boolean }) {
  const providers = useConfigStore(s => s.providers), setProviders = useConfigStore(s => s.setProviders)
  const [selection, setSelection] = useState("")
  const [override, setOverride] = useState<string | null>(null)
  const [busy, setBusy] = useState(false), [error, setError] = useState(""), [notice, setNotice] = useState("")
  const models = providers.flatMap(p => p.models.map(m => ({ ...m, provider: p.id, key: JSON.stringify([p.id, m.id]) })))
  const selected = models.find(m => m.key === selection)
  const value = override ?? (selected?.imageInput.source === "config" ? selected.imageInput.support : "auto")
  const save = async () => {
    if (!selected || busy) return
    setBusy(true); setError(""); setNotice("")
    try {
      const api = await (await import("@/api")).getApi()
      setProviders(await api.setModelImageInput({ provider: selected.provider, model: selected.id, imageInput: value === "auto" ? null : value === "supported" }))
      setOverride(null); setNotice("图片能力设置已保存，新的回复会使用新设置。")
    } catch (e) { setError(`保存失败：${String(e)}`) }
    finally { setBusy(false) }
  }
  const selectClass = "w-full rounded-lg border border-border bg-background px-2.5 py-2 text-sm"
  return <section aria-label="模型图片能力" className="space-y-2 rounded-lg border border-border p-3">
    <p className="text-sm font-medium">模型图片能力</p>
    <p className="text-xs text-muted-foreground">默认跟随模型目录。自建服务或中转服务有差异时，可以在这里修正；此标识表示配置声明，未经实际识图测试。</p>
    <fieldset disabled={disabled || busy} className="space-y-2">
      <label className="block text-xs">选择模型<select className={selectClass} value={selection} onChange={e => { setSelection(e.target.value); setOverride(null); setError(""); setNotice("") }}><option value="">选择要查看或调整的模型…</option>{models.map(m => <option key={m.key} value={m.key}>{m.displayName} · {m.provider} · {imageSupportLabels[m.imageInput.support]}</option>)}</select></label>
      {selected && <>
        <p className="text-xs text-muted-foreground">当前：{imageSupportLabels[selected.imageInput.support]} · 来源：{imageSourceLabels[selected.imageInput.source]}</p>
        <label className="block text-xs">图片输入<select className={selectClass} value={value} onChange={e => { setOverride(e.target.value); setNotice("") }}><option value="auto">跟随目录</option><option value="supported">支持图片</option><option value="unsupported">不支持图片</option></select></label>
        <button type="button" onClick={() => void save()} className="rounded-lg border border-border px-3 py-2 text-sm hover:bg-accent">{busy ? "正在保存…" : "保存图片能力"}</button>
      </>}
    </fieldset>
    {error && <p role="alert" className="text-xs text-red-500">{error}</p>}
    {notice && <p role="status" className="text-xs text-muted-foreground">{notice}</p>}
  </section>
}
